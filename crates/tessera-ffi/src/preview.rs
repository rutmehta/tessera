use super::*;

#[cfg(test)]
mod tests {
    use super::*;
    use engine_api::{id::JobId, jobs::CancellationToken};

    struct Events(Mutex<Vec<EngineEvent>>);
    impl EngineEventListener for Events {
        fn on_event(&self, event: EngineEvent) {
            self.0.lock().unwrap().push(event);
        }
    }

    fn pending_job(engine: &Arc<Engine>) -> Box<PreviewJob> {
        let request = RequestKey {
            image_id: "1".into(),
            max_px: 64,
            recipe_hash: String::new(),
            revision: [0; 32],
            render: [0; 32],
        };
        engine
            .preview_states
            .lock()
            .unwrap()
            .insert(request.clone(), State::Pending);
        Box::new(PreviewJob {
            engine: Arc::downgrade(engine),
            request,
            path: "missing.raw".into(),
            completed: false,
        })
    }

    #[test]
    fn cancelled_preview_publishes_terminal_state_and_completion() {
        let dir = tempfile::tempdir().unwrap();
        let engine = Engine::open(dir.path().to_string_lossy().into_owned()).unwrap();
        let events = Arc::new(Events(Mutex::new(Vec::new())));
        engine.set_event_listener(Some(events.clone()));
        let job = pending_job(&engine);
        let key = job.request.clone();
        let token = CancellationToken::new();
        token.cancel();
        assert!(matches!(
            job.run(&JobContext::new(JobId(0), token, None)),
            Err(engine_api::EngineError::Cancelled)
        ));
        assert!(matches!(
            engine.preview_states.lock().unwrap().get(&key),
            Some(State::Failed(_))
        ));
        assert_eq!(events.0.lock().unwrap().len(), 1);
    }

    #[test]
    fn completed_job_drop_does_not_fail_a_replacement_request() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        struct Replace {
            engine: Weak<Engine>,
            key: RequestKey,
            events: AtomicUsize,
        }
        impl EngineEventListener for Replace {
            fn on_event(&self, _: EngineEvent) {
                let engine = self.engine.upgrade().unwrap();
                // A callback can reenter after an evicted Ready preview and
                // admit a replacement under the same request key.
                if self.events.fetch_add(1, Ordering::SeqCst) == 0 {
                    engine
                        .preview_states
                        .lock()
                        .unwrap()
                        .insert(self.key.clone(), State::Pending);
                }
            }
        }
        let dir = tempfile::tempdir().unwrap();
        let engine = Engine::open(dir.path().to_string_lossy().into_owned()).unwrap();
        let job = pending_job(&engine);
        let listener = Arc::new(Replace {
            engine: Arc::downgrade(&engine),
            key: job.request.clone(),
            events: AtomicUsize::new(0),
        });
        engine.set_event_listener(Some(listener.clone()));
        job.run(&JobContext::new(JobId(0), CancellationToken::new(), None))
            .unwrap();
        assert_eq!(listener.events.load(Ordering::SeqCst), 1);
        assert!(matches!(
            engine.preview_states.lock().unwrap().get(&listener.key),
            Some(State::Pending)
        ));
    }

    #[test]
    fn discarded_preview_publishes_terminal_state_and_completion() {
        let dir = tempfile::tempdir().unwrap();
        let engine = Engine::open(dir.path().to_string_lossy().into_owned()).unwrap();
        let events = Arc::new(Events(Mutex::new(Vec::new())));
        engine.set_event_listener(Some(events.clone()));
        let job = pending_job(&engine);
        let key = job.request.clone();
        drop(job); // Scheduler may discard cancelled queued work without run().
        assert!(matches!(
            engine.preview_states.lock().unwrap().get(&key),
            Some(State::Failed(_))
        ));
        assert_eq!(events.0.lock().unwrap().len(), 1);
    }

    fn cull_proxy_fixture() -> (tempfile::TempDir, index::ImageInfo, core::Recipe) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("proxy.dng");
        std::fs::write(
            &path,
            include_bytes!("../../raw-decode/tests/fixtures/linear-gradient-jxl.dng"),
        )
        .unwrap();
        let id = engine_api::id::ImageId(13);
        let mut recipe = core::Recipe::new(id);
        recipe.unknown.insert(
            "lightroom_smart_preview".into(),
            serde_json::json!({"original_path": dir.path().join("offline.raw")}),
        );
        sidecar::Sidecar::write_recipe(
            sidecar::Sidecar::paths(&path).recipe,
            &sidecar::RecipeDocument {
                recipe: recipe.clone(),
                ..Default::default()
            },
        )
        .unwrap();
        let info = index::ImageInfo {
            id,
            path,
            size: 0,
            capture_seconds: None,
        };
        (dir, info, recipe)
    }

    #[test]
    fn lr13d_cull_hash_is_independent_of_rendered_cache() {
        let (dir, info, recipe) = cull_proxy_fixture();
        let decodes = raw_decode::lossy_dng::pixel_decode_count();
        let lenses = pipeline_cpu::lens_resolution_count();
        let cold = cull_preview_hash(&info, dir.path()).unwrap();
        assert!(cold.is_some());
        assert_eq!(raw_decode::lossy_dng::pixel_decode_count() - decodes, 1);
        assert_eq!(pipeline_cpu::lens_resolution_count() - lenses, 0);

        let store = previews::PreviewStore::new(dir.path().join("previews"), 512 << 20).unwrap();
        let grid =
            image::RgbImage::from_fn(90, 80, |x, y| image::Rgb([(200 - x - y / 4) as u8; 3]));
        let key =
            previews::PreviewKey::for_source(&info.path, 256, 1, recipe.recipe_hash().0.0).unwrap();
        store
            .put_image_cancellable(&key, &grid, 256, &|| Ok(()))
            .unwrap();
        let decodes = raw_decode::lossy_dng::pixel_decode_count();
        let expected = cull::dhash_jpeg(&store.get(&key, previews::Level::Full).unwrap()).unwrap();
        assert_eq!(cull_preview_hash(&info, dir.path()).unwrap(), cold);
        assert_ne!(
            cold,
            Some(expected),
            "fixture must distinguish rendered pixels"
        );
        assert_eq!(raw_decode::lossy_dng::pixel_decode_count(), decodes + 1);
        assert_eq!(pipeline_cpu::lens_resolution_count(), lenses);
    }

    #[test]
    fn lr13d_cull_hash_ignores_mac_cache_tiers_with_catalog_orientation() {
        // The Mac grid is 384px; its loupe tier is 2560px. Since LR-8m the
        // app keys imported thumbnails by the catalog orientation (6 here),
        // as for an ordinary RAW; entries written before LR-8m used 1. Both
        // tiers are planted and neither may feed the cull hash (REV-SP-B S3).
        for max_px in [384, 2560] {
            let (dir, info, mut recipe) = cull_proxy_fixture();
            recipe
                .unknown
                .insert("lightroom_orientation".into(), serde_json::json!(6));
            sidecar::Sidecar::write_recipe(
                sidecar::Sidecar::paths(&info.path).recipe,
                &sidecar::RecipeDocument {
                    recipe: recipe.clone(),
                    ..Default::default()
                },
            )
            .unwrap();
            let cold = cull_preview_hash(&info, dir.path()).unwrap();
            let store =
                previews::PreviewStore::new(dir.path().join("previews"), 512 << 20).unwrap();
            let grid =
                image::RgbImage::from_fn(90, 80, |x, y| image::Rgb([(200 - x - y / 4) as u8; 3]));
            let keys: Vec<_> = [6u8, 1]
                .into_iter()
                .map(|orientation| {
                    let key = previews::PreviewKey::for_source(
                        &info.path,
                        max_px,
                        orientation,
                        recipe.recipe_hash().0.0,
                    )
                    .unwrap();
                    store
                        .put_image_cancellable(&key, &grid, max_px, &|| Ok(()))
                        .unwrap();
                    key
                })
                .collect();
            let decodes = raw_decode::lossy_dng::pixel_decode_count();
            let lenses = pipeline_cpu::lens_resolution_count();
            let hash = cull_preview_hash(&info, dir.path()).unwrap();
            assert_eq!(
                raw_decode::lossy_dng::pixel_decode_count(),
                decodes + 1,
                "stable source pixels must ignore rendered previews"
            );
            assert_eq!(pipeline_cpu::lens_resolution_count(), lenses);
            assert_eq!(hash, cold);
            for key in &keys {
                let planted =
                    cull::dhash_jpeg(&store.get(key, previews::Level::Full).unwrap()).unwrap();
                assert_ne!(hash, Some(planted), "orientation {}", key.orientation);
            }
        }
    }

    #[test]
    fn lr13d_persisted_grouping_is_stable_for_edited_cropped_proxies_when_previews_warm() {
        let (dir, info, mut recipe) = cull_proxy_fixture();
        recipe
            .edit(
                engine_api::recipe::EditMeta::user("Crop and exposure", 1),
                |settings| {
                    settings.geometry.crop.rect.left = 0.25;
                    settings.geometry.crop.rect.bottom = 0.75;
                    settings.tone.exposure = 1.0;
                },
            )
            .unwrap();
        let second = dir.path().join("second.dng");
        std::fs::copy(&info.path, &second).unwrap();
        let support = tempfile::tempdir().unwrap();
        let root = support.path().join("Application Support/App");
        let mut index = index::Index::open(dir.path().join("index.sqlite")).unwrap();
        index
            .scan(
                dir.path(),
                &index::NoopSidecarReader,
                &index::NoopMetadataProvider,
            )
            .unwrap();
        let ids = index.search(&index::Query::default()).unwrap();
        assert_eq!(ids.len(), 2);
        let infos: Vec<_> = ids
            .iter()
            .map(|id| index.image_info(*id).unwrap())
            .collect();
        for info in &infos {
            let mut recipe = recipe.clone();
            recipe.image_id = Some(info.id);
            sidecar::Sidecar::write_recipe(
                sidecar::Sidecar::paths(&info.path).recipe,
                &sidecar::RecipeDocument {
                    recipe,
                    ..Default::default()
                },
            )
            .unwrap();
        }
        drop(index);
        let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let run = |version| {
            let count = calls.clone();
            let support = root.clone();
            let policy = cull::HashCachePolicy::application_support(
                root.clone(),
                "tessera-source-samples",
                version,
                &[],
            )
            .unwrap();
            let mut session = cull::OwnedCullSession::open_owned_with_cached_previews(
                index::Index::open(dir.path().join("index.sqlite")).unwrap(),
                dir.path(),
                policy,
                move |info| {
                    count.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                    cull_preview_hash(info, &support)
                },
            )
            .unwrap();
            let start = std::time::Instant::now();
            while session.previews_pending() {
                session.poll_previews().unwrap();
                assert!(start.elapsed() < std::time::Duration::from_secs(5));
                std::thread::yield_now();
            }
            assert!(session.preview_errors().is_empty());
            session
                .groups()
                .iter()
                .map(|g| g.images.clone())
                .collect::<Vec<_>>()
        };
        let cold = run(1);
        assert_eq!(cold.len(), 1);
        let store = previews::PreviewStore::new(root.join("previews"), 512 << 20).unwrap();
        for (n, info) in infos.iter().enumerate() {
            let recipe = catalog::document(&info.path, info.id).unwrap().recipe;
            let grid = image::RgbImage::from_fn(90, 80, |x, _| {
                image::Rgb([if n == 0 { x as u8 } else { (200 - x) as u8 }; 3])
            });
            let key =
                previews::PreviewKey::for_source(&info.path, 384, 1, recipe.recipe_hash().0.0)
                    .unwrap();
            store
                .put_image_cancellable(&key, &grid, 384, &|| Ok(()))
                .unwrap();
        }
        assert_eq!(
            run(1),
            cold,
            "persisted source hashes remain valid after preview warming"
        );
        assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 2);
        assert_eq!(
            run(2),
            cold,
            "fresh hashes must agree with persisted cold hashes"
        );
        assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 4);
    }

    #[test]
    fn lr13_imported_proxy_thumbnail_renders_at_thumbnail_level() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("proxy.dng");
        std::fs::write(
            &path,
            include_bytes!("../../raw-decode/tests/fixtures/linear-gradient-jxl.dng"),
        )
        .unwrap();
        let recipe = core::Recipe::default();
        let id = engine_api::id::ImageId(13);
        let render = |max_px| {
            render_imported(&path, id, &recipe, dir.path(), max_px)
                .unwrap()
                .0
        };
        let full = render(u32::MAX);
        let (w, h) = full.dimensions();
        let long = w.max(h);
        assert!(long >= 8, "fixture must span several levels");
        // A grid request renders the coarsest level that still covers it.
        let thumb = render(long.div_ceil(4));
        assert_eq!(thumb.dimensions(), (w.div_ceil(4), h.div_ceil(4)));
        // A request is never upsampled or rendered coarser than it needs.
        assert_eq!(
            render(long.div_ceil(4) + 1).dimensions(),
            (w.div_ceil(2), h.div_ceil(2))
        );
        assert_eq!(render(long).dimensions(), (w, h));
        // Same picture: every thumbnail sample lies inside its (partial-edge)
        // source bin. The viewport reduces before the display transform, so
        // the exact average is neither the encoded nor the display-linear mean.
        for (x, y, pixel) in thumb.enumerate_pixels() {
            for c in 0..3 {
                let bin: Vec<u8> = (y * 4..(y * 4 + 4).min(h))
                    .flat_map(|sy| (x * 4..(x * 4 + 4).min(w)).map(move |sx| (sx, sy)))
                    .map(|(sx, sy)| full.get_pixel(sx, sy)[c])
                    .collect();
                let (low, high) = (*bin.iter().min().unwrap(), *bin.iter().max().unwrap());
                assert!(
                    (low.saturating_sub(2)..=high.saturating_add(2)).contains(&pixel[c]),
                    "thumbnail sample {} outside its Develop bin {low}..={high}",
                    pixel[c]
                );
            }
        }
        assert_ne!(
            thumb.get_pixel(0, 0),
            thumb.get_pixel(2, 0),
            "gradient kept"
        );
        // Same-level comparison: each request is exactly Develop's
        // render_region of the full frame at the level it selects.
        let image = catalog::open_image(id, &path).unwrap();
        let settings = crate::develop::session_renderable(&recipe.settings, true, false);
        let renderer = image_core::Renderer::new(Default::default()).for_recipe(&recipe);
        for (max_px, level) in [(long.div_ceil(4), 2), (long.div_ceil(4) + 1, 1), (long, 0)] {
            let extent = image_core::Renderer::output_extent(&image, &settings, level).unwrap();
            let tiles = renderer
                .render_region(
                    &image,
                    &settings,
                    level,
                    image_core::PixelRect::full(extent),
                )
                .unwrap();
            assert_eq!(
                render(max_px),
                crate::lrcat_fidelity::stitch(extent, &tiles).unwrap(),
                "thumbnail for {max_px}px differs from the level-{level} region render"
            );
        }
    }

    #[test]
    fn lr13b_thumbnail_uses_proxy_effect_resources() {
        use engine_api::recipe::{
            EditMeta, MaskComponent, MaskKind,
            mask::{BrushStroke, RetouchKind, RetouchOperation, RetouchTarget},
            settings::{LensBlur, LensBlurDepth},
        };
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("synthetic.dng");
        std::fs::write(
            &path,
            include_bytes!("../../raw-decode/tests/fixtures/linear-gradient-jxl.dng"),
        )
        .unwrap();
        let id = engine_api::id::ImageId(1310);
        let image = catalog::open_image(id, &path).unwrap();
        let extent = image.active_extent();
        let depth = image_core::ml_depth::DepthMap::from_normalized_inverse(
            extent.width,
            extent.height,
            vec![0.; extent.area() as usize],
        )
        .unwrap();
        let store =
            image_core::ml_depth::DepthStore::new(root.path().join("previews/depth-cache"), 0)
                .unwrap();
        depth.store_pinned(&store, &[93; 32]).unwrap();
        for blur in [false, true] {
            let mut recipe = core::Recipe::default();
            recipe
                .edit(EditMeta::user("Proxy effect", 1), |s| {
                    if blur {
                        s.effects.lens_blur = Some(LensBlur {
                            amount: 100.,
                            depth: Some(LensBlurDepth {
                                mask_key: Some([93; 32]),
                                ..Default::default()
                            }),
                            ..Default::default()
                        });
                    } else {
                        s.locals.retouch.push(RetouchOperation {
                            id: engine_api::id::RetouchId(1),
                            kind: RetouchKind::Clone {
                                source_offset: [0.5, 0.],
                            },
                            target: RetouchTarget::Area {
                                components: vec![MaskComponent::new(MaskKind::Brush {
                                    strokes: vec![BrushStroke {
                                        points: vec![[0.25, 0.5, 1.]],
                                        radius: 0.2,
                                        feather: 0.,
                                        ..Default::default()
                                    }],
                                })],
                            },
                            opacity: 100.,
                            feather: 0.,
                            enabled: true,
                        });
                    }
                })
                .unwrap();
            let renderer = image_core::Renderer::new(Default::default())
                .for_recipe(&recipe)
                .with_depth(Arc::new(image_core::depth::DepthProvider::from_map(
                    depth.clone(),
                )))
                .with_retouch_renderer(Arc::new(brush::render_retouch));
            let tiles = renderer
                .render_region(
                    &image,
                    &recipe.settings,
                    0,
                    image_core::PixelRect::full(extent),
                )
                .unwrap();
            let expected = crate::lrcat_fidelity::stitch(extent, &tiles).unwrap();
            let base = render_imported(&path, id, &core::Recipe::default(), root.path(), u32::MAX)
                .unwrap()
                .0;
            assert_ne!(expected, base, "synthetic effect must change pixels");
            let actual = render_imported(&path, id, &recipe, root.path(), u32::MAX)
                .unwrap()
                .0;
            assert_eq!(
                actual, expected,
                "thumbnail dropped an available proxy effect"
            );
        }
    }

    #[test]
    fn lr13b_thumbnail_request_identity_tracks_mask_rasters_and_plan_version() {
        use engine_api::recipe::{
            EditMeta, LocalAdjustment, LocalParams, MaskComponent, MaskKind, mask::AdobeAiMask,
        };
        let root = tempfile::tempdir().unwrap();
        let support = root.path().join("support");
        let engine = Engine::open(support.to_string_lossy().into_owned()).unwrap();
        let path = root.path().join("proxy.dng");
        std::fs::write(
            &path,
            include_bytes!("../../raw-decode/tests/fixtures/linear-gradient-jxl.dng"),
        )
        .unwrap();
        let id = engine_api::id::ImageId(1318);
        let extent = catalog::open_image(id, &path).unwrap().level_extent(0);
        let raster = ml_segment::MaskRaster::new(
            extent.width,
            extent.height,
            vec![1.; extent.area() as usize],
        )
        .unwrap();
        let mut component = MaskComponent::new(MaskKind::Subject { model: None });
        component.adobe_ai = Some(AdobeAiMask {
            resource_id: None,
            category: "Subject".into(),
            mask_key: Some(raster.content_key()),
            regenerate: false,
        });
        let mut recipe = core::Recipe::new(id);
        recipe.unknown.insert(
            "lightroom_smart_preview".into(),
            serde_json::json!({"original_path": root.path().join("offline.raw")}),
        );
        recipe
            .edit(EditMeta::user("Imported mask", 1), |s| {
                s.locals.adjustments.push(LocalAdjustment {
                    components: vec![component],
                    params: LocalParams {
                        exposure: 1.,
                        ..Default::default()
                    },
                    ..Default::default()
                })
            })
            .unwrap();
        sidecar::Sidecar::write_recipe(
            sidecar::Sidecar::paths(&path).recipe,
            &sidecar::RecipeDocument {
                recipe: recipe.clone(),
                ..Default::default()
            },
        )
        .unwrap();
        let request = || {
            engine.request_raw(
                id.to_string(),
                path.to_string_lossy().into_owned(),
                64,
                recipe.recipe_hash().to_string(),
            )
        };
        let settle = || {
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(60);
            while engine
                .preview_states
                .lock()
                .unwrap()
                .values()
                .any(|state| matches!(state, State::Pending))
            {
                assert!(
                    std::time::Instant::now() < deadline,
                    "preview job timed out"
                );
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
        };
        assert!(request().unwrap().pending);
        settle();
        // A-ROUND2 / REV-SP-A B1: a raster that is merely missing (pending
        // regeneration) skips its adjustment, exactly as Develop does.
        let without_mask = request()
            .expect("a missing raster must not fail the thumbnail")
            .bytes
            .expect("the thumbnail renders without the pending mask");
        ml_segment::MaskStore::new(support.join("imported-masks"), 0)
            .unwrap()
            .put_content_pinned(&raster)
            .unwrap();
        let after = request().expect("a stored raster must render");
        assert!(
            after.pending,
            "a newly available raster re-renders the thumbnail"
        );
        settle();
        let with_mask = request().unwrap().bytes.unwrap();
        assert_ne!(
            with_mask, without_mask,
            "the arriving raster applies its adjustment"
        );

        let document = sidecar::Sidecar::read_recipe(sidecar::Sidecar::paths(&path).recipe)
            .unwrap()
            .recipe;
        let current = imported_render_identity(&document, &support);
        assert_ne!(current, [0; 32], "imported sources carry a render identity");
        assert_ne!(
            current,
            render_identity(&document, &support, IMPORTED_RENDER_PLAN_VERSION + 1),
            "the render-plan version is part of the identity"
        );
        ml_segment::MaskStore::new(support.join("imported-masks"), 0)
            .unwrap()
            .remove_pinned(&raster.content_key())
            .unwrap();
        assert_ne!(
            imported_render_identity(&document, &support),
            current,
            "removing a raster changes the identity"
        );
    }

    /// REV-SP-A S4: repeated thumbnail requests (the grid re-polls pending
    /// cells) must not re-read the owner recipe while its hash is unchanged,
    /// for imported proxies and ordinary photos alike.
    #[test]
    fn sp_int2_repeated_thumbnail_requests_do_not_reread_recipes() {
        let root = tempfile::tempdir().unwrap();
        let support = root.path().join("support");
        let engine = Engine::open(support.to_string_lossy().into_owned()).unwrap();
        let proxy = root.path().join("proxy.dng");
        std::fs::write(
            &proxy,
            include_bytes!("../../raw-decode/tests/fixtures/linear-gradient-jxl.dng"),
        )
        .unwrap();
        let id = engine_api::id::ImageId(1320);
        let mut recipe = core::Recipe::new(id);
        recipe.unknown.insert(
            "lightroom_smart_preview".into(),
            serde_json::json!({"original_path": root.path().join("offline.raw")}),
        );
        sidecar::Sidecar::write_recipe(
            sidecar::Sidecar::paths(&proxy).recipe,
            &sidecar::RecipeDocument {
                recipe: recipe.clone(),
                ..Default::default()
            },
        )
        .unwrap();
        let ordinary = root.path().join("ordinary.dng");
        std::fs::write(&ordinary, support_dng()).unwrap();
        for (path, hash) in [
            (&proxy, recipe.recipe_hash().to_string()),
            (&ordinary, String::new()),
        ] {
            // Pending, ready or failed: only the recipe I/O is measured.
            let request = || {
                let _ = engine.request_raw(
                    id.to_string(),
                    path.to_string_lossy().into_owned(),
                    64,
                    hash.clone(),
                );
            };
            request();
            let before = catalog::proxy_recipe_reads();
            for _ in 0..3 {
                request();
            }
            assert_eq!(
                catalog::proxy_recipe_reads(),
                before,
                "re-polling {} read the recipe again",
                path.file_name().unwrap().to_string_lossy()
            );
        }
    }

    fn support_dng() -> Vec<u8> {
        include_bytes!("../../raw-decode/tests/fixtures/linear-gradient-jxl.dng").to_vec()
    }

    /// REV-SP-A S6: a Lens Blur thumbnail that failed for lack of depth must
    /// re-render once its imported depth or the depth model becomes available.
    #[test]
    fn sp_int2_thumbnail_identity_tracks_lens_blur_depth_and_model() {
        let root = tempfile::tempdir().unwrap();
        let support = root.path();
        let id = engine_api::id::ImageId(1319);
        let key = image_core::depth::imported_depth_key(id);
        let mut recipe = core::Recipe::new(id);
        recipe
            .edit(engine_api::recipe::EditMeta::user("Lens Blur", 1), |s| {
                s.effects.lens_blur = Some(engine_api::recipe::settings::LensBlur {
                    amount: 50.,
                    depth: Some(engine_api::recipe::settings::LensBlurDepth {
                        mask_key: Some(key),
                        ..Default::default()
                    }),
                    ..Default::default()
                });
            })
            .unwrap();
        let mut plain = recipe.clone();
        plain
            .edit(engine_api::recipe::EditMeta::user("Off", 2), |s| {
                s.effects.lens_blur = None
            })
            .unwrap();
        let none = imported_render_identity(&recipe, support);
        assert_ne!(
            none,
            imported_render_identity(&plain, support),
            "Lens Blur participates in the identity"
        );
        let store =
            image_core::ml_depth::DepthStore::new(support.join("previews/depth-cache"), 0).unwrap();
        image_core::ml_depth::DepthMap::from_normalized_inverse(2, 2, vec![0.5; 4])
            .unwrap()
            .store_pinned(&store, &key)
            .unwrap();
        let with_depth = imported_render_identity(&recipe, support);
        assert_ne!(with_depth, none, "imported depth arriving re-renders");
        store.remove_pinned(&key).unwrap();
        assert_eq!(imported_render_identity(&recipe, support), none);
        let cache = support.join("models/cache");
        std::fs::create_dir_all(&cache).unwrap();
        std::fs::write(cache.join("synthetic-model.onnx"), b"weights").unwrap();
        assert_ne!(
            imported_render_identity(&recipe, support),
            none,
            "depth-model availability re-renders"
        );
    }

    #[test]
    fn lr13b_thumbnail_reports_invalid_and_skips_missing_imported_mask() {
        use engine_api::recipe::{
            EditMeta, LocalAdjustment, LocalParams, MaskComponent, MaskKind, mask::AdobeAiMask,
        };
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("synthetic.dng");
        std::fs::write(
            &path,
            include_bytes!("../../raw-decode/tests/fixtures/linear-gradient-jxl.dng"),
        )
        .unwrap();
        let mut component = MaskComponent::new(MaskKind::Subject { model: None });
        component.adobe_ai = Some(AdobeAiMask {
            resource_id: None,
            category: "Subject".into(),
            mask_key: Some([94; 32]),
            regenerate: false,
        });
        let mut recipe = core::Recipe::default();
        recipe
            .edit(EditMeta::user("Imported mask", 1), |s| {
                s.locals.adjustments.push(LocalAdjustment {
                    components: vec![component],
                    params: LocalParams {
                        exposure: 1.,
                        ..Default::default()
                    },
                    ..Default::default()
                })
            })
            .unwrap();
        // Missing (never stored, or pending regeneration): the group is
        // skipped, as in Develop (REV-SP-A B1).
        let plain = {
            let mut plain = recipe.clone();
            plain
                .edit(EditMeta::user("No mask", 2), |s| {
                    s.locals = Default::default()
                })
                .unwrap();
            render_imported(&path, engine_api::id::ImageId(1317), &plain, root.path(), 4).unwrap()
        };
        let missing = render_imported(
            &path,
            engine_api::id::ImageId(1316),
            &recipe,
            root.path(),
            4,
        )
        .expect("a missing imported raster renders with its adjustment skipped");
        assert_eq!(missing, plain);
        // Invalid (stored but the wrong extent): an error, never a silent omission.
        let wrong = ml_segment::MaskRaster::new(3, 3, vec![1.; 9]).unwrap();
        ml_segment::MaskStore::new(root.path().join("imported-masks"), 0)
            .unwrap()
            .put_content_pinned(&wrong)
            .unwrap();
        recipe
            .edit(EditMeta::user("Wrong extent", 3), |s| {
                s.locals.adjustments[0].components[0]
                    .adobe_ai
                    .as_mut()
                    .unwrap()
                    .mask_key = Some(wrong.content_key());
            })
            .unwrap();
        let result = render_imported(
            &path,
            engine_api::id::ImageId(1318),
            &recipe,
            root.path(),
            4,
        );
        assert!(
            result.is_err(),
            "thumbnail silently discarded an invalid imported raster"
        );
        assert!(result.unwrap_err().to_string().contains("mask"));
    }
}

use engine_api::jobs::{Job, JobContext, Priority, Scheduler};
use std::sync::Weak;

#[derive(Clone, Debug, uniffi::Record)]
pub struct PreviewResponse {
    pub bytes: Option<Vec<u8>>,
    pub pending: bool,
}
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(super) struct RequestKey {
    image_id: String,
    max_px: u32,
    recipe_hash: String,
    revision: [u8; 32],
    /// Imported sources: render-plan version plus mask-raster availability
    /// ([`imported_render_identity`]). Zero for ordinary originals.
    render: [u8; 32],
}

/// Bump whenever the imported/proxy thumbnail render plan changes which
/// pixels a given recipe produces (dependencies, mask hooks, effect resources).
/// Version 2: LR-13b imported mask hooks, retouch and cached depth on proxies.
pub(crate) const IMPORTED_RENDER_PLAN_VERSION: u32 = 2;

/// Identity of an imported thumbnail beyond its recipe: the render-plan version
/// and, for every imported AI raster the recipe references, whether a valid
/// durable raster is present (and its payload revision). A raster appearing,
/// disappearing or being replaced therefore re-renders instead of serving a
/// stale frame or a cached failure. Reads at most 32 bytes per referenced raster.
pub(crate) fn imported_render_identity(recipe: &core::Recipe, support: &Path) -> [u8; 32] {
    render_identity(recipe, support, IMPORTED_RENDER_PLAN_VERSION)
}

pub(crate) fn render_identity(recipe: &core::Recipe, support: &Path, version: u32) -> [u8; 32] {
    let mut hash = blake3::Hasher::new();
    hash.update(b"tessera-imported-thumbnail\0");
    hash.update(&version.to_le_bytes());
    let mut keys: Vec<[u8; 32]> = recipe
        .settings
        .locals
        .adjustments
        .iter()
        .flat_map(|g| &g.components)
        .flat_map(engine_api::recipe::MaskComponent::active_leaves)
        .filter_map(|c| c.adobe_ai.as_ref().and_then(|a| a.mask_key))
        .collect();
    keys.sort_unstable();
    keys.dedup();
    if !keys.is_empty() {
        // Not MaskStore::new: a listing request must not create directories.
        let root = support.join("imported-masks");
        let store = root
            .is_dir()
            .then(|| ml_segment::MaskStore::new(&root, 0).ok())
            .flatten();
        for key in keys {
            hash.update(&key);
            match store.as_ref().and_then(|s| s.pinned_revision(&key).ok()) {
                Some(revision) => {
                    hash.update(&[1]);
                    hash.update(&revision);
                }
                None => {
                    hash.update(&[0]);
                }
            }
        }
    }
    // Lens Blur (REV-SP-A S6): the imported depth slot and whether any model
    // weights are cached, so a frame that failed for lack of depth re-renders
    // once either arrives. One stat per slot, one directory listing.
    if let Some(blur) = &recipe.settings.effects.lens_blur {
        hash.update(b"lens-blur\0");
        if let Some(key) = blur.depth.as_ref().and_then(|d| d.mask_key) {
            hash.update(&key);
            let root = support.join("previews/depth-cache");
            match root
                .is_dir()
                .then(|| ml_segment::MaskStore::new(&root, 0).ok())
                .flatten()
                .and_then(|s| s.pinned_revision(&key).ok())
            {
                Some(revision) => {
                    hash.update(&[1]);
                    hash.update(&revision);
                }
                None => {
                    hash.update(&[0]);
                }
            }
        }
        let mut models: Vec<_> = std::fs::read_dir(support.join("models/cache"))
            .into_iter()
            .flatten()
            .filter_map(|e| e.ok())
            .map(|e| e.file_name())
            .filter(|n| {
                std::path::Path::new(n)
                    .extension()
                    .is_some_and(|x| x == "onnx")
            })
            .collect();
        models.sort();
        for name in models {
            hash.update(name.as_encoded_bytes());
            hash.update(&[0]);
        }
        hash.update(&[u8::from(std::env::var_os("TESSERA_DEPTH_MODELS").is_some())]);
    }
    *hash.finalize().as_bytes()
}

/// An imported owner's recipe and its catalog original (if recorded).
#[derive(Clone)]
pub(super) struct ImportedSource {
    original: Option<std::path::PathBuf>,
    recipe: Arc<core::Recipe>,
}

/// Per-owner facts for thumbnail requests, valid while the caller's recipe
/// hash is unchanged (a recipe edit changes the hash and re-reads).
#[derive(Default)]
pub(super) struct PreviewSources(
    std::collections::HashMap<std::path::PathBuf, (String, Option<ImportedSource>)>,
);
impl PreviewSources {
    const MAX_ENTRIES: usize = 200_000;
    fn get(&mut self, path: &Path, recipe_hash: &str) -> Option<ImportedSource> {
        if let Some((hash, imported)) = self.0.get(path)
            && hash == recipe_hash
        {
            return imported.clone();
        }
        if self.0.len() >= Self::MAX_ENTRIES {
            self.0.clear();
        }
        let imported = imported_recipe(path).map(|recipe| ImportedSource {
            original: recipe
                .unknown
                .get("lightroom_smart_preview")
                .and_then(|v| v.get("original_path"))
                .and_then(|p| p.as_str())
                .map(std::path::PathBuf::from),
            recipe: Arc::new(recipe),
        });
        self.0.insert(
            path.to_path_buf(),
            (recipe_hash.to_owned(), imported.clone()),
        );
        imported
    }
}

/// The owner's recipe when `path` is an imported (proxy or catalog-oriented)
/// source, read once. Ordinary originals return None.
fn imported_recipe(path: &Path) -> Option<core::Recipe> {
    #[cfg(test)]
    catalog::note_recipe_read();
    let recipe = sidecar::Sidecar::read_recipe(sidecar::Sidecar::paths(path).recipe)
        .ok()?
        .recipe;
    (recipe.unknown.contains_key("lightroom_smart_preview")
        || recipe.unknown.contains_key("lightroom_orientation"))
    .then_some(recipe)
}
pub(super) enum State {
    Pending,
    Ready(previews::PreviewKey),
    Failed(String),
}
impl Engine {
    pub(super) fn request_raw(
        self: &Arc<Self>,
        image_id: String,
        path: String,
        max_px: u32,
        recipe_hash: String,
    ) -> Result<PreviewResponse> {
        // REV-SP-A S4: the owner recipe is read once per recipe hash; a re-poll
        // costs no recipe I/O (one stat of the candidate original, for imports).
        let imported = self
            .preview_sources
            .lock()
            .map_err(failure)?
            .get(Path::new(&path), &recipe_hash);
        let source = imported
            .as_ref()
            .and_then(|i| i.original.clone())
            .filter(|p| p.is_file())
            .unwrap_or_else(|| std::path::PathBuf::from(&path));
        let revision = previews::PreviewKey::for_source(&source, max_px, 0, [0; 32])
            .map_err(failure)?
            .file_hash;
        let default_hash = core::Recipe::default().recipe_hash().to_string();
        let render = match &imported {
            Some(imported) => imported_render_identity(&imported.recipe, self.support_dir()?),
            None => [0; 32],
        };
        let request = RequestKey {
            image_id,
            max_px,
            // Edited recipes key their own previews (written by the develop
            // session or rendered here); unedited images share the default.
            recipe_hash: if recipe_hash.is_empty() {
                default_hash
            } else {
                recipe_hash
            },
            revision,
            render,
        };
        let mut states = self.preview_states.lock().map_err(failure)?;
        match states.get(&request) {
            Some(State::Pending) => {
                return Ok(PreviewResponse {
                    bytes: None,
                    pending: true,
                });
            }
            Some(State::Failed(message)) => return Err(failure(message)),
            Some(State::Ready(key)) => {
                if let Some(bytes) = self.previews.get(key, previews::Level::Full) {
                    return Ok(PreviewResponse {
                        bytes: Some(bytes),
                        pending: false,
                    });
                }
            }
            None => {}
        }
        states.insert(request.clone(), State::Pending);
        drop(states);
        self.jobs.submit(
            Box::new(PreviewJob {
                engine: Arc::downgrade(self),
                request,
                path,
                completed: false,
            }),
            None,
        );
        Ok(PreviewResponse {
            bytes: None,
            pending: true,
        })
    }
}
struct PreviewJob {
    engine: Weak<Engine>,
    request: RequestKey,
    path: String,
    completed: bool,
}
impl PreviewJob {
    /// Default recipes use the embedded-JPEG fast path. Edited recipes use the
    /// preview the develop session stored under the recipe hash, or render
    /// the renderable subset of the settings (bilinear demosaic, as for the
    /// default RAW fallback).
    fn render(
        &self,
        engine: &Engine,
        ctx: &JobContext,
    ) -> std::result::Result<previews::PreviewKey, String> {
        ctx.check_cancelled().map_err(|e| e.to_string())?;
        let path = Path::new(&self.path);
        let id = parse_id(&self.request.image_id).map_err(|e| e.to_string())?;
        let recipe = catalog::document(path, id)
            .map(|d| d.recipe)
            .unwrap_or_default();
        ctx.check_cancelled().map_err(|e| e.to_string())?;
        let hash = recipe.recipe_hash();
        if catalog::lightroom_proxy(path).is_some() || catalog::catalog_orientation(path).is_some()
        {
            let (image, orientation) = render_imported(
                path,
                id,
                &recipe,
                engine.support_dir().map_err(|e| e.to_string())?,
                self.request.max_px,
            )
            .map_err(|e| e.to_string())?;
            let key = previews::PreviewKey::for_source(
                &catalog::source_path(path),
                self.request.max_px,
                orientation as u8,
                hash.0.0,
            )
            .map_err(|e| e.to_string())?;
            engine
                .previews
                .put_image_cancellable(&key, &image, self.request.max_px, &|| ctx.check_cancelled())
                .map_err(|e| e.to_string())?;
            return Ok(key);
        }

        if hash == core::Recipe::default().recipe_hash() {
            return engine
                .previews
                .from_raw_cancellable(path, self.request.max_px, &|| ctx.check_cancelled())
                .map(|(key, _)| key)
                .map_err(|e| e.to_string());
        }
        let mut settings = crate::develop::renderable(&recipe.settings);
        settings.demosaic.method = core::settings::DemosaicMethod::Bilinear;
        engine
            .previews
            .from_raw_settings_cancellable(path, self.request.max_px, &settings, hash.0.0, &|| {
                ctx.check_cancelled()
            })
            .map_err(|e| e.to_string())
    }
}
impl Engine {
    /// Synchronous counterpart of the grid/loupe job for analysis and assist.
    /// Keep source resolution, catalog orientation and recipe dispatch identical.
    pub(crate) fn indexed_preview(
        &self,
        image_id: &str,
        path: &str,
        max_px: u32,
    ) -> Result<previews::PreviewKey> {
        let job = PreviewJob {
            engine: Weak::new(),
            request: RequestKey {
                image_id: image_id.into(),
                max_px,
                recipe_hash: String::new(),
                revision: [0; 32],
                render: [0; 32],
            },
            path: path.into(),
            completed: true,
        };
        job.render(
            self,
            &JobContext::new(
                engine_api::id::JobId(0),
                engine_api::jobs::CancellationToken::new(),
                None,
            ),
        )
        .map_err(failure)
    }

    /// Preview sizes requested so far for an image (the app's tiers).
    pub(super) fn requested_preview_sizes(&self, image_id: &str) -> Vec<u32> {
        let states = self
            .preview_states
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let mut sizes: Vec<u32> = states
            .keys()
            .filter(|k| k.image_id == image_id)
            .map(|k| k.max_px)
            .collect();
        sizes.sort_unstable();
        sizes.dedup();
        sizes
    }
}
// Queued jobs can be discarded without run(); also covers cancellation and
// unwinding before normal publication. Never leave an admitted request Pending.
// The scheduler must drop discarded jobs outside its lock (callbacks may reenter).
impl Drop for PreviewJob {
    fn drop(&mut self) {
        if self.completed {
            return;
        }
        let Some(engine) = self.engine.upgrade() else {
            return;
        };
        let mut states = engine
            .preview_states
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        if !matches!(states.get(&self.request), Some(State::Pending)) {
            return;
        }
        states.insert(
            self.request.clone(),
            State::Failed("RAW preview cancelled or interrupted".into()),
        );
        drop(states);
        engine.emit(EngineEvent::PreviewReady {
            image_id: self.request.image_id.clone(),
            max_px: self.request.max_px,
        });
    }
}

impl Job for PreviewJob {
    fn label(&self) -> &str {
        "RAW preview"
    }
    fn priority(&self) -> Priority {
        Priority::Preview
    }
    fn run(mut self: Box<Self>, ctx: &JobContext) -> engine_api::EngineResult<()> {
        ctx.check_cancelled()?;
        let Some(engine) = self.engine.upgrade() else {
            return Ok(());
        };
        let result = self.render(&engine, ctx);
        ctx.check_cancelled()?;
        let state = match result {
            Ok(key) => State::Ready(key),
            Err(error) => State::Failed(error),
        };
        engine
            .preview_states
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(self.request.clone(), state);
        self.completed = true;
        // Terminal notification also wakes failed requests so callers receive the error.
        // State is published first and no engine/store locks are held during callbacks.
        engine.emit(EngineEvent::PreviewReady {
            image_id: self.request.image_id.clone(),
            max_px: self.request.max_px,
        });
        Ok(())
    }
}

/// Long edge sampled for the source-based 9x8 perceptual hash.
const CULL_HASH_PX: u32 = 256;

/// Stable comparison policy: unedited source samples, independent of rendered
/// grid/loupe cache warmth, crop and local edits. Sample the immutable proxy's
/// camera channels at 256px, without
/// building RawImage/CameraLinearProxy or resolving any lens/CA correction.
/// Keeping the proxy as the hash source also avoids waking an offline original.
pub(crate) fn cull_preview_hash(
    info: &index::ImageInfo,
    _support: &Path,
) -> engine_api::EngineResult<Option<u64>> {
    let recipe = catalog::document(&info.path, info.id)?.recipe;
    let proxy = recipe.unknown.contains_key("lightroom_smart_preview");
    let orientation = recipe
        .unknown
        .get("lightroom_orientation")
        .and_then(serde_json::Value::as_u64)
        .filter(|value| (1..=8).contains(value))
        .map(|value| value as u16);
    if !proxy && orientation.is_none() {
        return cull::preview_hash(info);
    }
    let error = |e: String| engine_api::EngineError::Unsupported { what: e };
    if proxy {
        let mut file = std::fs::File::open(&info.path)?;
        if let Some(thumbnail) = raw_decode::lossy_dng::read_thumbnail(&mut file, CULL_HASH_PX)? {
            let rgb = image::RgbImage::from_fn(thumbnail.width, thumbnail.height, |x, y| {
                let pixel = thumbnail.pixels[(y * thumbnail.width + x) as usize];
                // A monotonic transfer preserves edges without color/lens work.
                image::Rgb(pixel.map(|v| (v.max(0.).sqrt() * 255.).clamp(0., 255.) as u8))
            });
            return Ok(Some(cull::dhash(&crate::assist::orient(
                rgb,
                orientation.unwrap_or(thumbnail.orientation) as u8,
            ))));
        }
    }
    // Imported ordinary originals still use their embedded JPEG, never a full
    // render. Missing embedded pixels remain a nonfatal absent hash.
    let bytes = if info
        .path
        .extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("jpg") || ext.eq_ignore_ascii_case("jpeg"))
    {
        Some(std::fs::read(&info.path)?)
    } else {
        raw_decode::RawSource::open(&info.path)?.embedded_preview()
    };
    bytes
        .map(|bytes| {
            use previews::Codec;
            let rgb = previews::Jpeg
                .decode(&bytes)
                .map_err(|e| error(e.to_string()))?;
            Ok(cull::dhash(&crate::assist::orient(
                rgb,
                orientation.unwrap_or(1) as u8,
            )))
        })
        .transpose()
}

/// Coarsest engine level whose long edge still covers `max_px`: a request is
/// never upsampled and never rendered finer than it needs.
fn thumbnail_level(extent: engine_api::tile::Extent, max_px: u32) -> u8 {
    let long = extent.width.max(extent.height);
    let mut level = 0;
    while level < image_core::render::MAX_LEVEL && long.div_ceil(1 << (level + 1)) >= max_px.max(1)
    {
        level += 1;
    }
    level
}

/// Imported sources use Develop's source boundary, embedded DCP and renderable
/// settings. In particular no LinearRaw source reaches the LibRaw CFA decoder.
/// The frame is produced at the level a `max_px` request needs (the viewport's
/// coarse-level contract), so display encoding and stitching are thumbnail-sized.
fn render_imported(
    path: &Path,
    id: engine_api::id::ImageId,
    recipe: &core::Recipe,
    support: &Path,
    max_px: u32,
) -> Result<(image::RgbImage, u16)> {
    let image = catalog::open_image(id, path)?;
    let settings = crate::develop::session_renderable(&recipe.settings, true, false);
    let mut renderer = image_core::Renderer::new(Default::default())
        .with_host_ignored_native_profiles()
        .for_recipe(recipe)
        .with_retouch_renderer(Arc::new(brush::render_retouch));
    if settings.effects.lens_blur.is_some() {
        renderer = renderer.with_depth(Arc::new(image_core::depth::DepthProvider::from_support(
            support,
        )?));
    }
    let masks = crate::develop::masks::MaskShared::new(&image);
    masks.load_available_imported(support, &settings);
    // A missing (pending) raster zeroes its group, as in Develop; the render
    // identity re-renders when it arrives. Only an invalid raster fails.
    if let Some(reason) = masks.invalid_imported(support, &settings) {
        return Err(failure(format!(
            "imported mask raster is invalid ({reason}); regenerate the mask before rendering this thumbnail"
        )));
    }
    renderer
        .mask_cache()
        .set_hooks(Some(Arc::new(crate::develop::masks::Hooks(masks))));
    let level = thumbnail_level(
        image_core::Renderer::output_extent(&image, &settings, 0)?,
        max_px,
    );
    let extent = image_core::Renderer::output_extent(&image, &settings, level)?;
    let tiles = renderer.render_region(
        &image,
        &settings,
        level,
        image_core::PixelRect::full(extent),
    )?;
    Ok((
        crate::lrcat_fidelity::stitch(extent, &tiles)?,
        image.metadata().orientation,
    ))
}
