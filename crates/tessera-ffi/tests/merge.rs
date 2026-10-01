use std::sync::{Arc, Mutex};
use tessera_ffi::*;

#[derive(Default)]
struct Listener(Mutex<Vec<PhotoProgress>>);
impl PhotoJobListener for Listener {
    fn on_progress(&self, progress: PhotoProgress) {
        self.0.lock().unwrap().push(progress);
    }
}

#[test]
fn cancel_at_publication_checkpoint_leaves_no_dng_or_catalog_row() {
    struct Paused(std::sync::Barrier);
    impl PhotoJobListener for Paused {
        fn on_progress(&self, p: PhotoProgress) {
            if p.stage == "write" {
                self.0.wait();
                self.0.wait();
            }
        }
    }
    let (dir, engine, ids) = fixture();
    let listener = Arc::new(Paused(std::sync::Barrier::new(2)));
    let job = engine
        .clone()
        .photo_merge(
            ids,
            MergeOptions {
                exposure_values: vec![1., 4.],
                ..Default::default()
            },
            listener.clone(),
        )
        .unwrap();
    listener.0.wait();
    job.cancel();
    listener.0.wait();
    let status = job.wait();
    assert_eq!(status.state, PhotoJobState::Cancelled);
    assert!(status.outputs.is_empty());
    assert_eq!(engine.list_images(ImageQuery::default()).unwrap().len(), 2);
    assert_eq!(
        std::fs::read_dir(dir.path().join("photos"))
            .unwrap()
            .count(),
        2
    );
}

#[test]
fn invalid_options_ids_missing_exposure_and_overlap_are_visible() {
    let (_dir, engine, ids) = fixture();
    for options in [
        MergeOptions {
            boundary_warp: 101,
            ..Default::default()
        },
        MergeOptions {
            exposure_values: vec![f64::NAN, 1.],
            ..Default::default()
        },
        MergeOptions {
            kind: MergeKind::HdrPanorama,
            ..Default::default()
        },
        MergeOptions {
            kind: MergeKind::Panorama,
            projection: MergeProjection::Cylindrical,
            ..Default::default()
        },
    ] {
        assert!(engine.merge_preview(ids.clone(), options).is_err());
    }
    assert!(
        engine
            .merge_preview(vec![ids[0].clone(); 2], Default::default())
            .is_err()
    );
    let missing = engine
        .merge_preview(ids.clone(), Default::default())
        .unwrap();
    assert!(missing.bytes.is_none());
    assert!(missing.warnings.iter().any(|w| w.contains("exposure")));
    let overlap = engine
        .merge_preview(
            ids,
            MergeOptions {
                kind: MergeKind::Panorama,
                ..Default::default()
            },
        )
        .unwrap();
    assert!(overlap.bytes.is_none());
    assert!(!overlap.warnings.is_empty());
}

#[test]
fn panorama_and_explicit_hdr_panorama_publish_with_each_projection() {
    let dir = tempfile::tempdir().unwrap();
    let photos = dir.path().join("photos");
    std::fs::create_dir(&photos).unwrap();
    for (i, (offset, gain)) in [(0., 0.25), (0., 1.), (85., 0.25), (85., 1.)]
        .into_iter()
        .enumerate()
    {
        let image = merge::LinearImage {
            width: 240,
            height: 160,
            pixels: (0..240 * 160)
                .map(|i| {
                    let x = (i % 240) as f64 + offset;
                    let y = (i / 240) as f64;
                    let v = (0.7
                        + 0.3 * (x * 0.17 + y * 0.13).sin()
                        + 0.2 * (x * 0.31 - y * 0.23).cos()
                        + 0.2 * ((x * 0.11).sin() * 7. + y * 0.19).cos())
                        as f32;
                    [v, v * 0.8, v * 0.7].map(|v| (v * gain).min(1.))
                })
                .collect(),
            color_matrix: [[1., 0., 0.], [0., 1., 0.], [0., 0., 1.]],
            as_shot_neutral: [1.; 3],
        };
        merge::write_dng(
            &mut std::fs::File::create(photos.join(format!("view{i}.dng"))).unwrap(),
            &image,
            &Default::default(),
        )
        .unwrap();
    }
    let engine = Engine::open(dir.path().join("support").to_string_lossy().into()).unwrap();
    engine
        .index_folder(photos.to_string_lossy().into())
        .unwrap();
    let mut rows = engine.list_images(ImageQuery::default()).unwrap();
    rows.sort_by(|a, b| a.path.cmp(&b.path));
    let ids: Vec<_> = rows.iter().map(|r| r.id.clone()).collect();
    for projection in [
        MergeProjection::Auto,
        MergeProjection::Perspective,
        MergeProjection::Cylindrical,
        MergeProjection::Spherical,
    ] {
        let options = MergeOptions {
            kind: MergeKind::Panorama,
            projection,
            focal_pixels: Some(300.),
            create_stack: false,
            fill_edges: true,
            boundary_warp: 50,
            ..Default::default()
        };
        let selected = vec![ids[0].clone(), ids[2].clone()];
        let preview = engine
            .merge_preview(selected.clone(), options.clone())
            .unwrap();
        assert!(preview.bytes.is_some(), "{:?}", preview.warnings);
        let status = engine
            .clone()
            .photo_merge(selected, options, Arc::new(Listener::default()))
            .unwrap()
            .wait();
        assert_eq!(status.state, PhotoJobState::Completed, "{:?}", status.error);
        assert!(
            engine
                .photo_stack(status.outputs[0].image_id.clone())
                .unwrap()
                .is_empty()
        );
    }
    let status = engine
        .clone()
        .photo_merge(
            ids,
            MergeOptions {
                kind: MergeKind::HdrPanorama,
                bracket_sizes: vec![2, 2],
                exposure_values: vec![0.25, 1., 0.25, 1.],
                auto_align: false,
                deghost: MergeDeghost::None,
                ..Default::default()
            },
            Arc::new(Listener::default()),
        )
        .unwrap()
        .wait();
    assert_eq!(status.state, PhotoJobState::Completed, "{:?}", status.error);
    assert!(status.outputs[0].path.ends_with("view0-HDR-Pano.dng"));
}

fn fixture() -> (tempfile::TempDir, Arc<Engine>, Vec<String>) {
    let dir = tempfile::tempdir().unwrap();
    let photos = dir.path().join("photos");
    std::fs::create_dir(&photos).unwrap();
    for (i, value) in [0.15, 0.6].into_iter().enumerate() {
        let image = merge::LinearImage {
            width: 16,
            height: 12,
            pixels: vec![[value; 3]; 192],
            color_matrix: [[1., 0., 0.], [0., 1., 0.], [0., 0., 1.]],
            as_shot_neutral: [1.; 3],
        };
        merge::write_dng(
            &mut std::fs::File::create(photos.join(format!("frame{i}.dng"))).unwrap(),
            &image,
            &Default::default(),
        )
        .unwrap();
    }
    let engine = Engine::open(dir.path().join("support").to_string_lossy().into()).unwrap();
    engine
        .index_folder(photos.to_string_lossy().into())
        .unwrap();
    let mut images = engine.list_images(ImageQuery::default()).unwrap();
    images.sort_by(|a, b| a.path.cmp(&b.path));
    (dir, engine, images.into_iter().map(|i| i.id).collect())
}

#[test]
#[cfg(target_os = "macos")]
fn merged_dng_develop_edit_persist_preview_and_export() {
    use std::sync::mpsc;
    use std::time::Duration;
    struct Frames(mpsc::Sender<std::result::Result<FrameInfo, String>>);
    impl DevelopListener for Frames {
        fn frame_ready(&self, frame: FrameInfo) {
            if frame.is_final {
                let _ = self.0.send(Ok(frame));
            }
        }
        fn render_failed(&self, message: String) {
            let _ = self.0.send(Err(message));
        }
        fn saved(&self, _: String) {}
    }
    let (dir, engine, ids) = fixture();
    let result = engine
        .clone()
        .photo_merge(
            ids,
            MergeOptions {
                auto_align: false,
                auto_tone: false,
                exposure_values: vec![1., 4.],
                ..Default::default()
            },
            Arc::new(Listener::default()),
        )
        .unwrap()
        .wait();
    assert_eq!(result.state, PhotoJobState::Completed, "{:?}", result.error);
    let id = result.outputs[0].image_id.clone();
    struct Ready(mpsc::Sender<()>);
    impl EngineEventListener for Ready {
        fn on_event(&self, event: EngineEvent) {
            if matches!(event, EngineEvent::PreviewReady { .. }) {
                let _ = self.0.send(());
            }
        }
    }
    let (tx, ready) = mpsc::channel();
    engine.set_event_listener(Some(Arc::new(Ready(tx))));
    let original = loop {
        if let Some(bytes) = engine
            .clone()
            .embedded_preview(id.clone(), 64)
            .unwrap()
            .bytes
        {
            break bytes;
        }
        ready.recv_timeout(Duration::from_secs(60)).unwrap();
    };
    assert!(
        image::load_from_memory(&original)
            .unwrap()
            .to_rgb8()
            .as_raw()
            .iter()
            .any(|v| *v > 30)
    );
    let session = engine.clone().open_develop_session(id.clone()).unwrap();
    let (tx, rx) = mpsc::channel();
    session.set_listener(Some(Arc::new(Frames(tx))));
    let plan = session.plan_surface(64, 64);
    for _ in 0..2 {
        let surface = surface::testing::create_rgba8(plan.width, plan.height);
        session
            .attach_surface(surface, plan.width, plan.height)
            .unwrap();
    }
    rx.recv_timeout(Duration::from_secs(60)).unwrap().unwrap();
    let brightness = || {
        let hist = session.get_histogram().unwrap().luminance;
        hist.iter()
            .enumerate()
            .map(|(i, n)| i as f64 * f64::from(*n))
            .sum::<f64>()
            / hist.iter().map(|n| f64::from(*n)).sum::<f64>()
    };
    let before = brightness();
    assert!(before > 30., "LinearRaw must not render black");
    session
        .set_settings(r#"{"tone":{"exposure":-1.0}}"#.into(), false)
        .unwrap();
    rx.recv_timeout(Duration::from_secs(60)).unwrap().unwrap();
    assert!(brightness() < before);
    session.commit("Exposure".into()).unwrap();
    session.flush().unwrap();
    session.close().unwrap();
    drop(session);
    drop(engine);
    let engine = Engine::open(dir.path().join("support").to_string_lossy().into()).unwrap();
    let session = engine.clone().open_develop_session(id.clone()).unwrap();
    let settings: serde_json::Value =
        serde_json::from_str(&session.get_settings_json().unwrap()).unwrap();
    assert_eq!(settings["tone"]["exposure"], -1.0);
    session.close().unwrap();
    let report = engine.export_batch(ExportTarget::Images { image_ids: vec![id] }, serde_json::json!({
        "format":"jpeg", "destination":dir.path().join("out"), "naming":"{name}", "metadata":"none"
    }).to_string(), None, None).unwrap();
    assert_eq!((report.exported, report.failed), (1, 0), "{report:?}");
    let exported = image::open(dir.path().join("out/frame0-HDR.jpg"))
        .unwrap()
        .to_rgb8();
    assert_eq!(exported.dimensions(), (16, 12));
    assert!(exported.as_raw().iter().any(|v| *v > 20));
}

#[test]
fn unstacked_merge_preserves_all_source_ids() {
    let (_dir, engine, ids) = fixture();
    let status = engine
        .clone()
        .photo_merge(
            ids.clone(),
            MergeOptions {
                auto_align: false,
                exposure_values: vec![1., 4.],
                create_stack: false,
                ..Default::default()
            },
            Arc::new(Listener::default()),
        )
        .unwrap()
        .wait();
    assert_eq!(status.state, PhotoJobState::Completed, "{:?}", status.error);
    assert_eq!(status.outputs[0].source_ids, ids);
    assert!(
        engine
            .photo_stack(status.outputs[0].image_id.clone())
            .unwrap()
            .is_empty()
    );
}

#[test]
fn hdr_preview_and_job_publish_float_dng_index_stack_and_changes() {
    let (_dir, engine, ids) = fixture();
    let options = MergeOptions {
        auto_align: false,
        auto_tone: false,
        exposure_values: vec![1., 4.],
        create_stack: true,
        ..Default::default()
    };
    let preview = engine.merge_preview(ids.clone(), options.clone()).unwrap();
    assert!(preview.bytes.is_some());
    assert_eq!(engine.list_images(ImageQuery::default()).unwrap().len(), 2);
    let before = engine.changes_since(0).unwrap().sequence;
    let listener = Arc::new(Listener::default());
    let job = engine
        .clone()
        .photo_merge(ids.clone(), options, listener.clone())
        .unwrap();
    let status = job.wait();
    assert_eq!(status.state, PhotoJobState::Completed, "{:?}", status.error);
    assert_eq!(status.outputs.len(), 1);
    let output = &status.outputs[0];
    assert!(output.path.ends_with("frame0-HDR.dng"));
    let dng =
        raw_decode::linear_dng::read(&mut std::fs::File::open(&output.path).unwrap()).unwrap();
    assert_eq!((dng.width, dng.height), (16, 12));
    assert!((dng.pixels[0][0] - 0.15).abs() < 1e-5);
    assert_eq!(engine.list_images(ImageQuery::default()).unwrap().len(), 3);
    let stack = engine.photo_stack(output.image_id.clone()).unwrap();
    assert_eq!(stack, [vec![output.image_id.clone()], ids].concat());
    assert!(
        engine
            .changes_since(before)
            .unwrap()
            .changes
            .iter()
            .any(|c| c.image_id == output.image_id && c.kind == LibraryChangeKind::Added)
    );
    let events = listener.0.lock().unwrap();
    assert_eq!(events.iter().filter(|p| p.stage == "completed").count(), 1);
}

#[test]
fn protected_merge_publishes_outside_source() {
    let (dir, _, _) = fixture();
    let protected = dir.path().join("Photos.lrdata");
    std::fs::rename(dir.path().join("photos"), &protected).unwrap();
    let engine = Engine::open(dir.path().join("merge-support").to_string_lossy().into()).unwrap();
    engine
        .index_folder(protected.to_string_lossy().into())
        .unwrap();
    let ids: Vec<_> = engine
        .list_images(ImageQuery::default())
        .unwrap()
        .into_iter()
        .map(|i| i.id)
        .collect();
    let output = dir.path().join("chosen-export");
    engine
        .export_batch(
            ExportTarget::Images {
                image_ids: ids.clone(),
            },
            serde_json::json!({"destination": output, "format":"original"}).to_string(),
            None,
            None,
        )
        .unwrap();
    let output = output.canonicalize().unwrap();
    let unrelated = output.join("unrelated.jpg");
    image::RgbImage::from_pixel(2, 2, image::Rgb([24, 36, 48]))
        .save(&unrelated)
        .unwrap();
    let job = engine
        .clone()
        .photo_merge(
            ids,
            MergeOptions {
                exposure_values: vec![1., 4.],
                ..Default::default()
            },
            Arc::new(Listener::default()),
        )
        .unwrap();
    let status = job.wait();
    assert_eq!(status.state, PhotoJobState::Completed, "{status:?}");
    assert_eq!(status.outputs.len(), 1);
    assert!(std::path::Path::new(&status.outputs[0].path).starts_with(&output));
    assert_eq!(std::fs::read_dir(&protected).unwrap().count(), 2);
    assert_eq!(
        engine.list_images(ImageQuery::default()).unwrap().len(),
        3,
        "only the new DNG is admitted"
    );
    assert!(unrelated.exists());
}
