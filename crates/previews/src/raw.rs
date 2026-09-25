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
        if max_px == 0 {
            return Err(engine_api::EngineError::invalid("max_px", "must be positive").into());
        }
        let mut raw = raw_decode::RawSource::open(path)?;
        let metadata = raw.metadata();
        let mut identity = fs::read(path)?;
        identity.extend_from_slice(&max_px.to_le_bytes());
        let key = PreviewKey::new(
            &identity,
            metadata.orientation as u8,
            engine_api::recipe::Recipe::default().recipe_hash().0.0,
        );
        let embedded = raw.embedded_preview().and_then(|b| Jpeg.decode(&b).ok());
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
                let cfa = raw.decode_cfa()?;
                let metadata = raw.metadata();
                let mut settings = engine_api::recipe::DevelopSettings::default();
                settings.demosaic.method = engine_api::recipe::settings::DemosaicMethod::Bilinear;
                let scale = metadata.default_crop[2]
                    .max(metadata.default_crop[3])
                    .div_ceil(max_px)
                    .max(1);
                self.renders.fetch_add(1, Ordering::Relaxed);
                pipeline_cpu::render_scaled(
                    &settings,
                    &pipeline_cpu::RenderSource::Cfa {
                        image: &cfa,
                        metadata: &metadata,
                    },
                    scale,
                )?
            }
        };
        let img = image::DynamicImage::ImageRgb8(img)
            .thumbnail(max_px, max_px)
            .to_rgb8();
        let full = orient(img, metadata.orientation as u8);
        for level in Level::ALL {
            let d = level.divisor();
            let scaled = image::imageops::resize(
                &full,
                (full.width() / d).max(1),
                (full.height() / d).max(1),
                FilterType::Lanczos3,
            );
            self.put(&key, level, &Jpeg.encode(&scaled)?)?;
        }
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
        if max_px == 0 {
            return Err(engine_api::EngineError::invalid("max_px", "must be positive").into());
        }
        let mut raw = raw_decode::RawSource::open(path)?;
        let metadata = raw.metadata();
        let mut identity = fs::read(path)?;
        identity.extend_from_slice(&max_px.to_le_bytes());
        let key = PreviewKey::new(&identity, metadata.orientation as u8, recipe_hash);
        if self.get(&key, Level::Full).is_some() {
            return Ok(key);
        }
        let cfa = raw.decode_cfa()?;
        let scale = metadata.default_crop[2]
            .max(metadata.default_crop[3])
            .div_ceil(max_px)
            .max(1);
        self.renders.fetch_add(1, Ordering::Relaxed);
        let img = pipeline_cpu::render_scaled(
            settings,
            &pipeline_cpu::RenderSource::Cfa {
                image: &cfa,
                metadata: &metadata,
            },
            scale,
        )?;
        self.put_image(&key, &img, max_px)?;
        Ok(key)
    }

    /// Stores a rendered, unoriented image as the pyramid for `key`: fitted
    /// to `max_px`, then oriented by `key.orientation`.
    pub fn put_image(&self, key: &PreviewKey, img: &RgbImage, max_px: u32) -> Result<()> {
        let fitted = if img.width().max(img.height()) > max_px {
            image::DynamicImage::ImageRgb8(img.clone())
                .thumbnail(max_px, max_px)
                .to_rgb8()
        } else {
            img.clone()
        };
        let full = orient(fitted, key.orientation);
        for level in Level::ALL {
            let d = level.divisor();
            let scaled = image::imageops::resize(
                &full,
                (full.width() / d).max(1),
                (full.height() / d).max(1),
                FilterType::Lanczos3,
            );
            self.put(key, level, &Jpeg.encode(&scaled)?)?;
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
