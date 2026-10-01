use super::*;
use std::sync::atomic::Ordering;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PreviewSource {
    Embedded,
    Rendered,
}

impl PreviewStore {
    /// Actual pipeline invocations, excluding JPEG fast paths and cache hits.
    pub fn render_count(&self) -> u64 {
        self.renders.load(Ordering::Relaxed)
    }

    /// Blocking worker entry point. Schedule at Priority::Preview, never on UI.
    pub fn from_raw(&self, path: &Path, max_px: u32) -> Result<(PreviewKey, PreviewSource)> {
        self.from_raw_cancellable(path, max_px, &|| Ok(()))
    }

    /// Cooperatively checks between blocking stages; a codec/render already in
    /// progress is not interrupted. Pass `&|| token.check()` or a JobContext check.
    pub fn from_raw_cancellable(
        &self,
        path: &Path,
        max_px: u32,
        check: &dyn Fn() -> engine_api::EngineResult<()>,
    ) -> Result<(PreviewKey, PreviewSource)> {
        check()?;
        let hash = engine_api::recipe::Recipe::default().recipe_hash().0.0;
        let revision = PreviewKey::for_source(path, max_px, 0, hash)?;
        if let Some(hit) = self.raw_alias(&revision) {
            check()?;
            return Ok(hit);
        }
        self.source_work.fetch_add(1, Ordering::Relaxed);
        let (key, source) = self.load_raw_uncached(path, max_px, check)?;
        if revision == PreviewKey::for_source(path, max_px, 0, hash)? {
            check()?;
            self.put_raw_alias(&revision, &key, source)?;
        }
        check()?;
        Ok((key, source))
    }

    fn load_raw_uncached(
        &self,
        path: &Path,
        max_px: u32,
        check: &dyn Fn() -> engine_api::EngineResult<()>,
    ) -> Result<(PreviewKey, PreviewSource)> {
        check()?;
        if max_px == 0 {
            return Err(engine_api::EngineError::invalid("max_px", "must be positive").into());
        }
        if raw_decode::linear_dng::is_linear_dng(&mut fs::File::open(path)?)? {
            let recipe = engine_api::recipe::Recipe::default();
            let key = self.render_linear_dng(
                path,
                max_px,
                &recipe.settings,
                recipe.recipe_hash().0.0,
                check,
            )?;
            return Ok((key, PreviewSource::Rendered));
        }
        check()?;
        let mut raw = raw_decode::RawSource::open(path)?;
        check()?;
        let metadata = raw.metadata();
        let mut identity = fs::read(path)?;
        check()?;
        identity.extend_from_slice(&max_px.to_le_bytes());
        let key = PreviewKey::new(
            &identity,
            metadata.orientation as u8,
            engine_api::recipe::Recipe::default().recipe_hash().0.0,
        );
        check()?;
        let embedded = raw.embedded_preview();
        check()?;
        let embedded = embedded.and_then(|b| Jpeg.decode(&b).ok());
        check()?;
        // Eighth-resolution means one eighth along each sensor axis.
        let embedded = embedded.filter(|im| sufficient(im, metadata.width, metadata.height));
        let source = if embedded.is_some() {
            PreviewSource::Embedded
        } else {
            PreviewSource::Rendered
        };
        if self.get(&key, Level::Full).is_some() {
            return Ok((key, source));
        }
        let img = match embedded {
            Some(img) => img,
            None => {
                check()?;
                let cfa = raw.decode_cfa()?;
                check()?;
                let metadata = raw.metadata();
                let mut settings = engine_api::recipe::DevelopSettings::default();
                settings.demosaic.method = engine_api::recipe::settings::DemosaicMethod::Bilinear;
                let scale = metadata.default_crop[2]
                    .max(metadata.default_crop[3])
                    .div_ceil(max_px)
                    .max(1);
                self.renders.fetch_add(1, Ordering::Relaxed);
                pipeline_cpu::render_scaled_with_context(
                    &settings,
                    &pipeline_cpu::RenderSource::Cfa {
                        image: &cfa,
                        metadata: &metadata,
                    },
                    scale,
                    &self.render_context(),
                )?
            }
        };
        check()?;
        // Preserve the default path's existing thumbnail sizing, including
        // sources smaller than the requested bound.
        let img = image::DynamicImage::ImageRgb8(img)
            .thumbnail(max_px, max_px)
            .to_rgb8();
        self.put_image_cancellable(&key, &img, max_px, check)?;
        check()?;
        Ok((key, source))
    }
}

impl PreviewStore {
    /// Edited RAW preview: renders `settings` (the caller passes only
    /// renderable settings) and stores it under `recipe_hash`, keyed by the
    /// same file identity as [`PreviewStore::from_raw`]. A preview already
    /// stored under that key (e.g. by the develop session) is reused.
    pub fn from_raw_settings(
        &self,
        path: &Path,
        max_px: u32,
        settings: &engine_api::recipe::DevelopSettings,
        recipe_hash: [u8; 32],
    ) -> Result<PreviewKey> {
        self.from_raw_settings_cancellable(path, max_px, settings, recipe_hash, &|| Ok(()))
    }

    /// Edited RAW counterpart of `from_raw_cancellable`.
    pub fn from_raw_settings_cancellable(
        &self,
        path: &Path,
        max_px: u32,
        settings: &engine_api::recipe::DevelopSettings,
        recipe_hash: [u8; 32],
        check: &dyn Fn() -> engine_api::EngineResult<()>,
    ) -> Result<PreviewKey> {
        check()?;
        // A cached frame is not a substitute for the required render capability.
        if !settings.locals.retouch.is_empty() {
            pipeline_cpu::validate_settings_with_retouch(settings, self.retouch.as_deref())?;
        }

        let revision = PreviewKey::for_source(path, max_px, 0, recipe_hash)?;
        if let Some((key, _)) = self.raw_alias(&revision) {
            check()?;
            return Ok(key);
        }
        self.source_work.fetch_add(1, Ordering::Relaxed);
        let key = self.load_raw_settings_uncached(path, max_px, settings, recipe_hash, check)?;
        if revision == PreviewKey::for_source(path, max_px, 0, recipe_hash)? {
            check()?;
            self.put_raw_alias(&revision, &key, PreviewSource::Rendered)?;
        }
        check()?;
        Ok(key)
    }

    fn load_raw_settings_uncached(
        &self,
        path: &Path,
        max_px: u32,
        settings: &engine_api::recipe::DevelopSettings,
        recipe_hash: [u8; 32],
        check: &dyn Fn() -> engine_api::EngineResult<()>,
    ) -> Result<PreviewKey> {
        check()?;
        if max_px == 0 {
            return Err(engine_api::EngineError::invalid("max_px", "must be positive").into());
        }
        if raw_decode::linear_dng::is_linear_dng(&mut fs::File::open(path)?)? {
            return self.render_linear_dng(path, max_px, settings, recipe_hash, check);
        }
        check()?;
        let mut raw = raw_decode::RawSource::open(path)?;
        check()?;
        let metadata = raw.metadata();
        let mut identity = fs::read(path)?;
        check()?;
        identity.extend_from_slice(&max_px.to_le_bytes());
        let key = PreviewKey::new(&identity, metadata.orientation as u8, recipe_hash);
        if self.get(&key, Level::Full).is_some() {
            check()?;
            return Ok(key);
        }
        check()?;
        let cfa = raw.decode_cfa()?;
        check()?;
        let scale = metadata.default_crop[2]
            .max(metadata.default_crop[3])
            .div_ceil(max_px)
            .max(1);
        self.renders.fetch_add(1, Ordering::Relaxed);
        let img = pipeline_cpu::render_scaled_with_context(
            settings,
            &pipeline_cpu::RenderSource::Cfa {
                image: &cfa,
                metadata: &metadata,
            },
            scale,
            &self.render_context(),
        )?;
        self.put_image_cancellable(&key, &img, max_px, check)?;
        check()?;
        Ok(key)
    }

    /// Same calibrated upright working-space boundary as develop ingestion.
    fn render_linear_dng(
        &self,
        path: &Path,
        max_px: u32,
        settings: &engine_api::recipe::DevelopSettings,
        recipe_hash: [u8; 32],
        check: &dyn Fn() -> engine_api::EngineResult<()>,
    ) -> Result<PreviewKey> {
        check()?;
        let mut identity = fs::read(path)?;
        check()?;
        identity.extend_from_slice(&max_px.to_le_bytes());
        let key = PreviewKey::new(&identity, 1, recipe_hash);
        if self.get(&key, Level::Full).is_some() {
            check()?;
            return Ok(key);
        }
        check()?;
        let source = image_core::RawImage::open(engine_api::id::ImageId(0), path)?;
        check()?;
        let rgb = source
            .rgb()
            .ok_or_else(|| engine_api::EngineError::invalid("LinearRaw", "RGB source required"))?;
        let pixels = rgb.pixels();
        let scale = pixels.width().max(pixels.height()).div_ceil(max_px).max(1);
        self.renders.fetch_add(1, Ordering::Relaxed);
        let image = pipeline_cpu::render_scaled_with_context(
            settings,
            &pipeline_cpu::RenderSource::Rgb(pixels),
            scale,
            &self.render_context(),
        )?;
        self.put_image_cancellable(&key, &image, max_px, check)?;
        check()?;
        Ok(key)
    }

    /// Stores a rendered, unoriented image as the pyramid for `key`: fitted
    /// to `max_px`, then oriented by `key.orientation`.
    pub fn put_image(&self, key: &PreviewKey, img: &RgbImage, max_px: u32) -> Result<()> {
        self.put_image_cancellable(key, img, max_px, &|| Ok(()))
    }

    /// Writes complete levels atomically, checking between resize, encode and
    /// publication. Completed levels remain reusable; Full is published last,
    /// so an interrupted pyramid cannot masquerade as a completed RAW hit.
    pub fn put_image_cancellable(
        &self,
        key: &PreviewKey,
        img: &RgbImage,
        max_px: u32,
        check: &dyn Fn() -> engine_api::EngineResult<()>,
    ) -> Result<()> {
        check()?;
        let fitted = if img.width().max(img.height()) > max_px {
            image::DynamicImage::ImageRgb8(img.clone())
                .thumbnail(max_px, max_px)
                .to_rgb8()
        } else {
            img.clone()
        };
        check()?;
        let full = orient(fitted, key.orientation);
        for level in Level::ALL {
            check()?;
            let d = level.divisor();
            let scaled = image::imageops::resize(
                &full,
                (full.width() / d).max(1),
                (full.height() / d).max(1),
                FilterType::Lanczos3,
            );
            check()?;
            let bytes = Jpeg.encode(&scaled)?;
            check()?;
            self.put(key, level, &bytes)?;
            check()?;
        }
        Ok(())
    }
}

fn sufficient(img: &RgbImage, width: u32, height: u32) -> bool {
    u64::from(img.width()) * 8 >= u64::from(width)
        && u64::from(img.height()) * 8 >= u64::from(height)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cancelled_raw_does_no_source_work() {
        let dir = tempfile::tempdir().unwrap();
        let store = PreviewStore::new(dir.path(), u64::MAX).unwrap();
        let token = engine_api::jobs::CancellationToken::new();
        token.cancel();
        let result = store.from_raw_cancellable(Path::new("missing.raw"), 384, &|| token.check());
        assert!(matches!(
            result,
            Err(PreviewError::Render(engine_api::EngineError::Cancelled))
        ));
        assert_eq!(store.source_work_count(), 0);
    }

    #[test]
    fn raw_cancellation_after_source_admission_skips_render() {
        let dir = tempfile::tempdir().unwrap();
        let store = PreviewStore::new(dir.path(), u64::MAX).unwrap();
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/raw/sample.dng");
        let token = engine_api::jobs::CancellationToken::new();
        let result = store.from_raw_cancellable(&path, 64, &|| {
            if store.source_work_count() > 0 {
                token.cancel();
            }
            token.check()
        });
        assert!(matches!(
            result,
            Err(PreviewError::Render(engine_api::EngineError::Cancelled))
        ));
        assert_eq!(store.render_count(), 0);
    }

    #[test]
    fn cancellation_during_alias_publication_is_reported() {
        let dir = tempfile::tempdir().unwrap();
        let store = PreviewStore::new(dir.path(), u64::MAX).unwrap();
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/raw/sony-arw.ARW");
        let hash = engine_api::recipe::Recipe::default().recipe_hash().0.0;
        let revision = PreviewKey::for_source(&path, 64, 0, hash).unwrap();
        let token = engine_api::jobs::CancellationToken::new();
        let result = store.from_raw_cancellable(&path, 64, &|| {
            if store.raw_alias(&revision).is_some() {
                token.cancel();
            }
            token.check()
        });
        assert!(matches!(
            result,
            Err(PreviewError::Render(engine_api::EngineError::Cancelled))
        ));
        // Publication was atomic: cancellation never corrupts completed cache work.
        let (key, _) = store.from_raw(&path, 64).unwrap();
        assert!(Jpeg.decode(&store.get(&key, Level::Full).unwrap()).is_ok());
    }

    #[test]
    fn pyramid_cancellation_keeps_complete_levels_and_retry_finishes() {
        let dir = tempfile::tempdir().unwrap();
        let store = PreviewStore::new(dir.path(), u64::MAX).unwrap();
        let key = PreviewKey::new(b"cancel-pyramid", 6, [0; 32]);
        let image = RgbImage::new(64, 32);
        let token = engine_api::jobs::CancellationToken::new();
        let result = store.put_image_cancellable(&key, &image, 32, &|| {
            if store.get(&key, Level::Eighth).is_some() {
                token.cancel();
            }
            token.check()
        });
        assert!(matches!(
            result,
            Err(PreviewError::Render(engine_api::EngineError::Cancelled))
        ));
        assert!(
            Jpeg.decode(&store.get(&key, Level::Eighth).unwrap())
                .is_ok()
        );
        for level in [Level::Quarter, Level::Half, Level::Full] {
            assert!(store.get(&key, level).is_none());
        }
        // Reopening/retrying sees valid JPEGs, no abandoned temporary writes.
        drop(store);
        let store = PreviewStore::new(dir.path(), u64::MAX).unwrap();
        store.put_image(&key, &image, 32).unwrap();
        for level in Level::ALL {
            assert!(Jpeg.decode(&store.get(&key, level).unwrap()).is_ok());
        }
        assert_eq!(
            Jpeg.decode(&store.get(&key, Level::Full).unwrap())
                .unwrap()
                .dimensions(),
            (16, 32)
        );
        assert!(
            fs::read_dir(dir.path().join(key.directory()))
                .unwrap()
                .all(|entry| entry.unwrap().path().extension().unwrap() == "jpg")
        );
    }

    #[test]
    fn pyramid_checks_between_resize_encode_and_write() {
        // Fit, orientation, then four boundaries per level: before resize,
        // before encode, before publication and after publication.
        for stop in 1..=18 {
            let dir = tempfile::tempdir().unwrap();
            let store = PreviewStore::new(dir.path(), u64::MAX).unwrap();
            let key = PreviewKey::new(b"pyramid-stages", 1, [0; 32]);
            let calls = std::cell::Cell::new(0);
            let token = engine_api::jobs::CancellationToken::new();
            let result = store.put_image_cancellable(&key, &RgbImage::new(64, 32), 32, &|| {
                calls.set(calls.get() + 1);
                if calls.get() == stop {
                    token.cancel();
                }
                token.check()
            });
            assert!(
                matches!(
                    result,
                    Err(PreviewError::Render(engine_api::EngineError::Cancelled))
                ),
                "boundary {stop}"
            );
            assert_eq!(calls.get(), stop);
            let published = Level::ALL
                .into_iter()
                .filter(|&level| store.get(&key, level).is_some())
                .count();
            assert_eq!(published, (stop - 2).max(0) as usize / 4, "boundary {stop}");
        }
    }

    #[test]
    fn edited_raw_precancelled_does_not_read_missing_source() {
        let dir = tempfile::tempdir().unwrap();
        let store = PreviewStore::new(dir.path(), u64::MAX).unwrap();
        let token = engine_api::jobs::CancellationToken::new();
        token.cancel();
        let result = store.from_raw_settings_cancellable(
            Path::new("missing.raw"),
            64,
            &engine_api::recipe::DevelopSettings::default(),
            [1; 32],
            &|| token.check(),
        );
        assert!(matches!(
            result,
            Err(PreviewError::Render(engine_api::EngineError::Cancelled))
        ));
        assert_eq!(store.source_work_count(), 0);
    }

    #[test]
    fn embedded_threshold_includes_exact_eighth() {
        assert!(sufficient(&RgbImage::new(100, 75), 800, 600));
        assert!(!sufficient(&RgbImage::new(99, 75), 800, 600));
        assert!(!sufficient(&RgbImage::new(100, 74), 800, 600));
    }
    #[test]
    fn recipe_edits_change_cache_identity() {
        let mut recipe = engine_api::recipe::Recipe::default();
        let key = PreviewKey::new(b"raw", 1, recipe.recipe_hash().0.0);
        recipe.settings.tone.exposure = 1.0;
        let edited = PreviewKey::new(b"raw", 1, recipe.recipe_hash().0.0);
        assert_ne!(key.directory(), edited.directory());
    }
}
