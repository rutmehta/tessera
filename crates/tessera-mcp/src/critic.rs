//! Full-resolution critic data; preview pixels are never a metrics fallback.
use super::*;
use engine_api::{jobs::CancellationToken, recipe::settings::NormalizedRect};
use image_core::resident::OutputMetrics;
impl PreviewCache {
    pub(crate) fn noise_patch(
        &self,
        id: ImageId,
        path: &Path,
        recipe: &Recipe,
    ) -> EngineResult<image::RgbImage> {
        let source = self.source(id, path)?;
        let (w, h) = match &*source {
            Decoded::Raw(raw) => {
                let e = Renderer::output_extent(raw, &recipe.settings, 0)?;
                if (5..=8).contains(&raw.metadata().orientation) {
                    (e.height, e.width)
                } else {
                    (e.width, e.height)
                }
            }
            Decoded::Rgb { .. } => {
                let full = self.display(id, path, recipe, None)?;
                let (w, h) = full.dimensions();
                return Ok(image::imageops::crop_imm(
                    &full,
                    w.saturating_sub(256) / 2,
                    h.saturating_sub(256) / 2,
                    w.min(256),
                    h.min(256),
                )
                .to_image());
            }
        };
        let (x, y) = (w.saturating_sub(256) / 2, h.saturating_sub(256) / 2);
        self.crop(
            id,
            path,
            recipe,
            NormalizedRect {
                left: x as f32 / w as f32,
                top: y as f32 / h as f32,
                right: (x + w.min(256)) as f32 / w as f32,
                bottom: (y + h.min(256)) as f32 / h as f32,
            },
        )
    }
    pub(crate) fn metrics(
        &self,
        id: ImageId,
        path: &Path,
        recipe: &Recipe,
    ) -> EngineResult<OutputMetrics> {
        let source = self.source(id, path)?;
        if let Decoded::Raw(raw) = &*source
            && let Some(metrics) = self.renderer().for_recipe(recipe).render_output_metrics(
                raw,
                &recipe.settings,
                &CancellationToken::new(),
            )?
        {
            return Ok(metrics);
        }
        // RGB, Adobe, geometry, local adjustments, or a missing GPU: exact
        // full-resolution CPU fallback, deliberately not the cheap preview.
        let full = self.display(id, path, recipe, None)?;
        let mut metrics = OutputMetrics::default();
        for p in full.pixels() {
            metrics.add_pixel(p.0);
        }
        Ok(metrics)
    }
    pub(crate) fn crop(
        &self,
        id: ImageId,
        path: &Path,
        recipe: &Recipe,
        region: NormalizedRect,
    ) -> EngineResult<image::RgbImage> {
        if !region.is_valid() {
            return Err(EngineError::invalid(
                "face region",
                "invalid normalized rectangle",
            ));
        }
        let source = self.source(id, path)?;
        let Decoded::Raw(raw) = &*source else {
            let full = self.display(id, path, recipe, None)?;
            let rect = bounds(region, full.width(), full.height());
            return Ok(
                image::imageops::crop_imm(&full, rect.x, rect.y, rect.width, rect.height)
                    .to_image(),
            );
        };
        let frame = Renderer::output_extent(raw, &recipe.settings, 0)?;
        let orientation = raw.metadata().orientation;
        let (ow, oh) = if (5..=8).contains(&orientation) {
            (frame.height, frame.width)
        } else {
            (frame.width, frame.height)
        };
        let oriented = bounds(region, ow, oh);
        let rect = unorient_rect(oriented, frame.width, frame.height, orientation);
        let tiles =
            self.renderer()
                .for_recipe(recipe)
                .render_region(raw, &recipe.settings, 0, rect)?;
        let mut rgb = image::RgbImage::new(rect.width, rect.height);
        for tile in tiles {
            let l = tile.layout();
            let samples = tile.samples::<u8>()?;
            let (ox, oy) = tile.coord().pixel_origin(TILE_SIZE);
            for y in oy.max(rect.y)..(oy + l.extent.height).min(rect.y + rect.height) {
                for x in ox.max(rect.x)..(ox + l.extent.width).min(rect.x + rect.width) {
                    rgb.put_pixel(
                        x - rect.x,
                        y - rect.y,
                        image::Rgb(std::array::from_fn(|c| {
                            samples[l.index(c as u8, (x - ox) as i32, (y - oy) as i32).unwrap()]
                        })),
                    );
                }
            }
        }
        Ok(pixels::orient(rgb, orientation))
    }
}
fn bounds(r: NormalizedRect, w: u32, h: u32) -> PixelRect {
    let x = (r.left * w as f32).floor() as u32;
    let y = (r.top * h as f32).floor() as u32;
    let right = ((r.right * w as f32).ceil() as u32).min(w);
    let bottom = ((r.bottom * h as f32).ceil() as u32).min(h);
    PixelRect::new(x, y, right - x, bottom - y)
}
fn unorient_rect(r: PixelRect, w: u32, h: u32, orientation: u16) -> PixelRect {
    let inverse = |x, y| match orientation {
        2 => (w - x, y),
        3 => (w - x, h - y),
        4 => (x, h - y),
        5 => (y, x),
        6 => (y, h - x),
        7 => (w - y, h - x),
        8 => (w - y, x),
        _ => (x, y),
    };
    let (x0, y0) = inverse(r.x, r.y);
    let (x1, y1) = inverse(r.x + r.width, r.y + r.height);
    PixelRect::new(x0.min(x1), y0.min(y1), x0.abs_diff(x1), y0.abs_diff(y1))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn face_crop_inversion_matches_every_exif_orientation() {
        let full = image::RgbImage::from_fn(11, 7, |x, y| image::Rgb([x as u8, y as u8, 0]));
        for orientation in 1..=8 {
            let displayed = pixels::orient(full.clone(), orientation);
            let r = PixelRect::new(1, 2, 3, 4);
            let raw = unorient_rect(r, full.width(), full.height(), orientation);
            let crop =
                image::imageops::crop_imm(&full, raw.x, raw.y, raw.width, raw.height).to_image();
            assert_eq!(
                pixels::orient(crop, orientation),
                image::imageops::crop_imm(&displayed, r.x, r.y, r.width, r.height).to_image(),
                "orientation {orientation}"
            );
        }
    }
}
