//! Fidelity preview for a Lightroom import (docs/05 §3.2 "Validation").
//!
//! Renders a sample of imported images with their translated recipes and
//! compares them with Lightroom's own cached previews (`Previews.lrdata`, read
//! through `import_lrcat::previews`). The comparison is CIEDE2000 per pixel in
//! CIELAB (D65), reported as the mean and the 95th percentile per image, plus a
//! JPEG thumbnail of each side.
//!
//! Renderer selection follows each recipe's process version. Adobe PV3–6
//! use the CPU compatibility operators; native recipes retain native math.
use crate::{Result, failure, lrcat::LrcatImport, lrcat::LrcatOptions};
use engine_api::{
    id::ImageId,
    recipe::Recipe,
    tile::{Extent, TILE_SIZE, Tile},
};
use image::{RgbImage, imageops};
use previews::Codec;
use std::{path::Path, sync::atomic::Ordering};

pub const RENDERER: &str = "recipe-selected (native/adobe-compat)";

#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum LrcatFidelityStatus {
    Compared,
    /// Lightroom never cached a preview for this image.
    NoPreview,
    MissingOriginal,
    /// The original or the preview could not be decoded / rendered.
    Failed,
}

#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct LrcatFidelitySample {
    pub catalog_id: i64,
    pub name: String,
    pub path: String,
    pub status: LrcatFidelityStatus,
    /// Why a sample was not compared, or a caveat (e.g. aspect mismatch).
    pub message: String,
    pub delta_e_mean: f32,
    pub delta_e_p95: f32,
    /// JPEG thumbnails (empty when unavailable).
    pub lightroom_jpeg: Vec<u8>,
    pub tessera_jpeg: Vec<u8>,
}

#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct LrcatFidelity {
    /// Operator-set selection policy (per-recipe native/Adobe compatibility).
    pub renderer: String,
    pub previews_available: bool,
    pub samples: Vec<LrcatFidelitySample>,
}

/// Edge (px) of the Lightroom level the comparison uses.
const COMPARE_EDGE: u32 = 512;

#[uniffi::export]
impl LrcatImport {
    /// Renders up to `n` imported images (edited ones first, spread across the
    /// catalog) and compares them with Lightroom's previews. Writes nothing.
    /// Cancellable with `cancel()` between images.
    pub fn fidelity_sample(
        &self,
        options: LrcatOptions,
        n: u32,
        thumb_px: u32,
    ) -> Result<LrcatFidelity> {
        self.cancel.store(false, Ordering::SeqCst);
        let thumb_px = thumb_px.clamp(32, 1024);
        let resolved = crate::lrcat::resolve(&self.plan, &options)?;
        let with_preview = |id: i64| {
            self.previews
                .as_ref()
                .and_then(|p| p.lrprev_path(id))
                .is_some_and(|f| f.is_file())
        };
        // Candidates: originals present (virtual copies render their own recipe
        // from the master's file). Edited images with previews first.
        let mut candidates: Vec<&crate::lrcat::Resolved> =
            resolved.iter().filter(|r| r.path.is_file()).collect();
        candidates.sort_by_key(|r| {
            let i = &self.plan.images[r.index];
            (
                !with_preview(i.catalog_id),
                i.recipe.history.entries.is_empty(),
                i.catalog_id,
            )
        });
        let preferred = candidates
            .iter()
            .filter(|r| with_preview(self.plan.images[r.index].catalog_id))
            .count()
            .max(1);
        let pool = &candidates[..preferred.min(candidates.len())];
        let picks = spread(pool.len(), n as usize);
        let mut samples = Vec::new();
        for i in picks {
            if self.cancel.load(Ordering::SeqCst) {
                break;
            }
            let r = pool[i];
            let image = &self.plan.images[r.index];
            let mut sample = LrcatFidelitySample {
                catalog_id: image.catalog_id,
                name: image.display_name.clone(),
                path: r.path.to_string_lossy().into_owned(),
                status: LrcatFidelityStatus::Compared,
                message: String::new(),
                delta_e_mean: 0.0,
                delta_e_p95: 0.0,
                lightroom_jpeg: vec![],
                tessera_jpeg: vec![],
            };
            if let Err(e) = self.compare(image, &r.path, thumb_px, &mut sample) {
                sample.status = LrcatFidelityStatus::Failed;
                sample.message = e.to_string();
            }
            samples.push(sample);
        }
        Ok(LrcatFidelity {
            renderer: RENDERER.into(),
            previews_available: self.previews.as_ref().is_some_and(|p| !p.is_empty()),
            samples,
        })
    }
}

/// `k` indices spread evenly over `0..len`.
fn spread(len: usize, k: usize) -> Vec<usize> {
    if len == 0 || k == 0 {
        return vec![];
    }
    if k >= len {
        return (0..len).collect();
    }
    (0..k).map(|i| i * len / k).collect()
}

impl LrcatImport {
    fn compare(
        &self,
        image: &import_lrcat::ImportedImage,
        path: &Path,
        thumb_px: u32,
        sample: &mut LrcatFidelitySample,
    ) -> Result<()> {
        if !path.is_file() {
            sample.status = LrcatFidelityStatus::MissingOriginal;
            sample.message = "original not found".into();
            return Ok(());
        }
        let preview = match &self.previews {
            Some(p) => p.jpeg(image.catalog_id, COMPARE_EDGE)?,
            None => None,
        };
        let rendered = render(path, &image.recipe, COMPARE_EDGE.max(thumb_px))?;
        sample.tessera_jpeg = thumbnail_jpeg(&rendered, thumb_px)?;
        let Some(preview) = preview else {
            sample.status = LrcatFidelityStatus::NoPreview;
            sample.message = "Lightroom has no cached preview for this photo".into();
            return Ok(());
        };
        let reference = decode_preview(&preview)?;
        sample.lightroom_jpeg = thumbnail_jpeg(&reference, thumb_px)?;
        let (w, h) = reference.dimensions();
        let aspect = |w: u32, h: u32| w as f32 / h.max(1) as f32;
        if (aspect(w, h) / aspect(rendered.width(), rendered.height()) - 1.0).abs() > 0.02 {
            sample.message = "aspect ratio differs (crop or orientation not matched)".into();
        }
        let resized = imageops::resize(&rendered, w, h, imageops::FilterType::Triangle);
        let (mean, p95) = delta_e_stats(&reference, &resized);
        sample.delta_e_mean = mean;
        sample.delta_e_p95 = p95;
        Ok(())
    }
}

fn thumbnail_jpeg(img: &RgbImage, edge: u32) -> Result<Vec<u8>> {
    let (w, h) = img.dimensions();
    let s = edge as f32 / w.max(h).max(1) as f32;
    let small = if s < 1.0 {
        imageops::thumbnail(
            img,
            ((w as f32 * s).round() as u32).max(1),
            ((h as f32 * s).round() as u32).max(1),
        )
    } else {
        img.clone()
    };
    previews::Jpeg.encode(&small).map_err(failure)
}

/// Decodes a preview JPEG to sRGB, converting from an embedded ICC profile
/// (Lightroom's previews are not guaranteed to be sRGB).
fn decode_preview(bytes: &[u8]) -> Result<RgbImage> {
    let mut img = previews::Jpeg.decode(bytes).map_err(failure)?;
    if let Some(icc) = import_lrcat::previews::jpeg_icc_profile(bytes) {
        let mut registry = color_mgmt::Registry::new();
        let mut convert = || -> std::result::Result<color_mgmt::Transform, color_mgmt::Error> {
            let source = registry.load_bytes(&icc)?;
            let srgb = registry.builtin(color_mgmt::Builtin::Srgb)?;
            color_mgmt::Transform::new(&source, &srgb, color_mgmt::TransformOptions::default())
        };
        let transform = convert().map_err(failure)?;
        for p in img.pixels_mut() {
            let rgb = transform.apply(p.0.map(|c| c as f32 / 255.0));
            p.0 = rgb.map(|c| (c.clamp(0.0, 1.0) * 255.0).round() as u8);
        }
    }
    Ok(img)
}

/// Recipe-selected render, about `edge` px on the long side, sRGB.
fn render(path: &Path, recipe: &Recipe, edge: u32) -> Result<RgbImage> {
    let adobe = recipe.process_version.family == engine_api::recipe::ProcessFamily::Adobe;
    if adobe && !(3..=6).contains(&recipe.process_version.revision) {
        return Err(failure("Adobe fidelity requires PV3–6"));
    }
    let ext = path
        .extension()
        .map(|e| e.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    if matches!(ext.as_str(), "jpg" | "jpeg") {
        let decoded = image::open(path).map_err(failure)?.into_rgb8();
        let (w, h) = decoded.dimensions();
        let s = (edge as f32 / w.max(h) as f32).min(1.0);
        let small = imageops::thumbnail(
            &decoded,
            ((w as f32 * s).round() as u32).max(1),
            ((h as f32 * s).round() as u32).max(1),
        );
        let source = linear_rec2020(&small)?;
        if adobe {
            return Ok(image_core::pipeline_adobe::render_scaled(
                &recipe.settings,
                &pipeline_cpu::RenderSource::Rgb(&source),
                1,
            )?);
        }
        return Ok(pipeline_cpu::render(
            &recipe.settings,
            &pipeline_cpu::RenderSource::Rgb(&source),
        )?);
    }
    if matches!(ext.as_str(), "png" | "tif" | "tiff" | "heic") {
        return Err(failure(format!(
            "fidelity preview does not decode .{ext} originals yet"
        )));
    }
    let raw = image_core::RawImage::open(ImageId::default(), path)?;
    let mut level = 0;
    while level < image_core::render::MAX_LEVEL {
        let next = raw.level_extent(level + 1);
        if next.width.max(next.height) < edge {
            break;
        }
        level += 1;
    }
    let extent = raw.level_extent(level);
    let renderer = image_core::Renderer::new(image_core::RendererConfig {
        process_version: recipe.process_version,
        ..Default::default()
    });
    let tiles = renderer.render_region(
        &raw,
        &recipe.settings,
        level,
        image_core::PixelRect::full(extent),
    )?;
    Ok(orient(stitch(extent, &tiles)?, raw.metadata().orientation))
}

fn linear_rec2020(rgb: &RgbImage) -> Result<pipeline_cpu::Image> {
    let (w, h) = rgb.dimensions();
    let mut planes = vec![vec![0.0f32; (w as usize) * (h as usize)]; 3];
    const M: [[f32; 3]; 3] = [
        [0.6274, 0.3293, 0.0433],
        [0.0691, 0.9195, 0.0114],
        [0.0164, 0.0880, 0.8956],
    ];
    for (i, p) in rgb.pixels().enumerate() {
        let l = p.0.map(srgb_to_linear);
        for (c, row) in M.iter().enumerate() {
            planes[c][i] = row[0] * l[0] + row[1] * l[1] + row[2] * l[2];
        }
    }
    Ok(pipeline_cpu::Image::new(w, h, planes)?)
}

fn stitch(extent: Extent, tiles: &[Tile]) -> Result<RgbImage> {
    let mut out = RgbImage::new(extent.width.max(1), extent.height.max(1));
    for tile in tiles {
        let layout = tile.layout();
        if layout.channels != 3 {
            return Err(failure("display tile must have three planes"));
        }
        let x0 = tile.coord().x * TILE_SIZE;
        let y0 = tile.coord().y * TILE_SIZE;
        let planes = [
            tile.plane::<u8>(0)?,
            tile.plane::<u8>(1)?,
            tile.plane::<u8>(2)?,
        ];
        for y in 0..layout.extent.height {
            for x in 0..layout.extent.width {
                let i = (y as usize + layout.halo as usize) * layout.stride()
                    + x as usize
                    + layout.halo as usize;
                if x0 + x < out.width() && y0 + y < out.height() {
                    out.put_pixel(
                        x0 + x,
                        y0 + y,
                        image::Rgb([planes[0][i], planes[1][i], planes[2][i]]),
                    );
                }
            }
        }
    }
    Ok(out)
}

fn orient(p: RgbImage, orientation: u16) -> RgbImage {
    match orientation {
        2 => imageops::flip_horizontal(&p),
        3 => imageops::rotate180(&p),
        4 => imageops::flip_vertical(&p),
        5 => imageops::rotate90(&imageops::flip_vertical(&p)),
        6 => imageops::rotate90(&p),
        7 => imageops::rotate90(&imageops::flip_horizontal(&p)),
        8 => imageops::rotate270(&p),
        _ => p,
    }
}

fn srgb_to_linear(c: u8) -> f32 {
    let v = c as f32 / 255.0;
    if v <= 0.04045 {
        v / 12.92
    } else {
        ((v + 0.055) / 1.055).powf(2.4)
    }
}

/// sRGB (D65) 8-bit to CIELAB (D65 reference white).
pub(crate) fn srgb_to_lab(rgb: [u8; 3]) -> [f32; 3] {
    let [r, g, b] = rgb.map(srgb_to_linear);
    let x = 0.412_456_4 * r + 0.357_576_1 * g + 0.180_437_5 * b;
    let y = 0.212_672_9 * r + 0.715_152_2 * g + 0.072_175 * b;
    let z = 0.019_333_9 * r + 0.119_192 * g + 0.950_304_1 * b;
    let f = |t: f32| {
        if t > 216.0 / 24389.0 {
            t.cbrt()
        } else {
            (24389.0 / 27.0 * t + 16.0) / 116.0
        }
    };
    let (fx, fy, fz) = (f(x / 0.950_47), f(y), f(z / 1.088_83));
    [116.0 * fy - 16.0, 500.0 * (fx - fy), 200.0 * (fy - fz)]
}

/// CIEDE2000 (Sharma, Wu & Dalal 2005), kL = kC = kH = 1.
pub(crate) fn delta_e_2000(lab1: [f32; 3], lab2: [f32; 3]) -> f32 {
    let [l1, a1, b1] = lab1.map(f64::from);
    let [l2, a2, b2] = lab2.map(f64::from);
    let c1 = a1.hypot(b1);
    let c2 = a2.hypot(b2);
    let cm = (c1 + c2) / 2.0;
    let g = 0.5 * (1.0 - (cm.powi(7) / (cm.powi(7) + 25f64.powi(7))).sqrt());
    let (a1p, a2p) = ((1.0 + g) * a1, (1.0 + g) * a2);
    let (c1p, c2p) = (a1p.hypot(b1), a2p.hypot(b2));
    let hue = |b: f64, a: f64| {
        if a == 0.0 && b == 0.0 {
            0.0
        } else {
            let h = b.atan2(a).to_degrees();
            if h < 0.0 { h + 360.0 } else { h }
        }
    };
    let (h1p, h2p) = (hue(b1, a1p), hue(b2, a2p));
    let dl = l2 - l1;
    let dc = c2p - c1p;
    let dh = if c1p * c2p == 0.0 {
        0.0
    } else if (h2p - h1p).abs() <= 180.0 {
        h2p - h1p
    } else if h2p - h1p > 180.0 {
        h2p - h1p - 360.0
    } else {
        h2p - h1p + 360.0
    };
    let dhh = 2.0 * (c1p * c2p).sqrt() * (dh.to_radians() / 2.0).sin();
    let lm = (l1 + l2) / 2.0;
    let cmp = (c1p + c2p) / 2.0;
    let hm = if c1p * c2p == 0.0 {
        h1p + h2p
    } else if (h1p - h2p).abs() <= 180.0 {
        (h1p + h2p) / 2.0
    } else if h1p + h2p < 360.0 {
        (h1p + h2p + 360.0) / 2.0
    } else {
        (h1p + h2p - 360.0) / 2.0
    };
    let t = 1.0 - 0.17 * (hm - 30.0).to_radians().cos()
        + 0.24 * (2.0 * hm).to_radians().cos()
        + 0.32 * (3.0 * hm + 6.0).to_radians().cos()
        - 0.20 * (4.0 * hm - 63.0).to_radians().cos();
    let d_theta = 30.0 * (-((hm - 275.0) / 25.0).powi(2)).exp();
    let rc = 2.0 * (cmp.powi(7) / (cmp.powi(7) + 25f64.powi(7))).sqrt();
    let sl = 1.0 + 0.015 * (lm - 50.0).powi(2) / (20.0 + (lm - 50.0).powi(2)).sqrt();
    let sc = 1.0 + 0.045 * cmp;
    let sh = 1.0 + 0.015 * cmp * t;
    let rt = -(2.0 * d_theta).to_radians().sin() * rc;
    let (x, y, z) = (dl / sl, dc / sc, dhh / sh);
    (x * x + y * y + z * z + rt * y * z).sqrt() as f32
}

/// Mean and 95th percentile ΔE2000 over two equally sized sRGB images.
pub(crate) fn delta_e_stats(a: &RgbImage, b: &RgbImage) -> (f32, f32) {
    let mut values: Vec<f32> = a
        .pixels()
        .zip(b.pixels())
        .map(|(p, q)| delta_e_2000(srgb_to_lab(p.0), srgb_to_lab(q.0)))
        .collect();
    if values.is_empty() {
        return (0.0, 0.0);
    }
    let mean = values.iter().sum::<f32>() / values.len() as f32;
    let k = ((values.len() as f32 * 0.95).ceil() as usize).clamp(1, values.len()) - 1;
    let (_, p95, _) = values.select_nth_unstable_by(k, f32::total_cmp);
    (mean, *p95)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn jpeg_fidelity_selects_recipe_process_version() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("source.jpg");
        let rgb = RgbImage::from_pixel(32, 24, image::Rgb([120, 80, 50]));
        rgb.save(&path).unwrap();
        let mut recipe = Recipe::new(ImageId::default());
        let native = render(&path, &recipe, 64).unwrap();
        recipe.process_version = engine_api::recipe::ProcessVersion::adobe(6);
        let compat = render(&path, &recipe, 64).unwrap();
        assert_ne!(native, compat);
        let decoded = image::open(path).unwrap().into_rgb8();
        let linear = linear_rec2020(&decoded).unwrap();
        let expected = image_core::pipeline_adobe::render_scaled(
            &recipe.settings,
            &pipeline_cpu::RenderSource::Rgb(&linear),
            1,
        )
        .unwrap();
        assert_eq!(compat, expected);
    }

    #[test]
    fn ciede2000_matches_sharma_reference_pairs() {
        // Sharma et al. (2005) test data, pairs 1, 7, 13, 17 and 25.
        for (l1, l2, want) in [
            ([50.0, 2.6772, -79.7751], [50.0, 0.0, -82.7485], 2.0425),
            ([50.0, 0.0, 0.0], [50.0, -1.0, 2.0], 2.3669),
            ([50.0, 2.4900, -0.0010], [50.0, -2.4900, 0.0011], 7.2195),
            ([50.0, 2.5, 0.0], [73.0, 25.0, -18.0], 27.1492),
            (
                [60.2574, -34.0099, 36.2677],
                [60.4626, -34.1751, 39.4387],
                1.2644,
            ),
        ] {
            let got = delta_e_2000(l1, l2);
            assert!((got - want).abs() < 1e-3, "{l1:?} {l2:?}: {got} != {want}");
            assert!((delta_e_2000(l2, l1) - want).abs() < 1e-3);
        }
    }

    #[test]
    fn identical_images_have_zero_delta_e_and_white_is_l100() {
        let img = RgbImage::from_fn(8, 4, |x, y| image::Rgb([x as u8 * 30, y as u8 * 60, 90]));
        assert_eq!(delta_e_stats(&img, &img), (0.0, 0.0));
        let white = srgb_to_lab([255, 255, 255]);
        assert!((white[0] - 100.0).abs() < 0.01 && white[1].abs() < 0.01 && white[2].abs() < 0.01);
        let mut darker = img.clone();
        darker.pixels_mut().for_each(|p| p.0 = p.0.map(|c| c / 2));
        let (mean, p95) = delta_e_stats(&img, &darker);
        assert!(mean > 1.0 && p95 >= mean);
    }

    #[test]
    fn spread_is_even_and_bounded() {
        assert_eq!(spread(10, 3), vec![0, 3, 6]);
        assert_eq!(spread(2, 5), vec![0, 1]);
        assert!(spread(0, 4).is_empty());
    }
}
