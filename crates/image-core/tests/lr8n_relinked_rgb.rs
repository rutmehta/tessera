//! LR-8n (REV-SP-B S1): a rotated phone JPEG imported offline as a Lightroom
//! Smart Preview, edited, then relinked to its original keeps its crop
//! rectangle and masks on the same content, with the same crop aspect. Both
//! sides are read in the stored (pre-orientation) frame and carry the same
//! absolute catalog orientation as their display orientation.
//!
//! The Smart Preview is camera-linear and the original is display-encoded
//! sRGB, so their tone differs by construction. The comparison therefore
//! normalizes each render (z-scored luminance on a fixed normalized grid, as
//! LR-8m M10 compares on a grid) and compares where things are, not how
//! bright they are.
mod common;
#[allow(dead_code)]
#[path = "../../raw-decode/tests/support/mod.rs"]
mod support;

use engine_api::{
    id::ImageId,
    recipe::{DevelopSettings, LocalAdjustment, LocalParams, MaskComponent, MaskKind},
};
use image_core::{PixelRect, RawImage, Renderer};
use std::sync::Arc;

const GRID: (usize, usize) = (36, 24);
const W: u32 = 1280;
const H: u32 = 852;

/// Scene-linear luminance of the stored (unrotated) frame: a gradient, a
/// texture and an off-centre bright disc, so any rotation, flip or transpose
/// of the edit frame moves content.
fn scene(u: f32, v: f32) -> f32 {
    let disc = if (u - 0.72).powi(2) + (v - 0.28).powi(2) < 0.012 {
        0.18
    } else {
        0.
    };
    0.04 + 0.16 * u + 0.07 * v + 0.03 * (u * 31.).sin() * (v * 19.).cos() + disc
}

fn edits(mask_center: [f32; 2], locals: bool) -> DevelopSettings {
    let mut s = DevelopSettings::default();
    s.detail.sharpening.amount = 0.;
    s.detail.noise_reduction.color = 0.;
    s.geometry.crop.rect.left = 0.1;
    s.geometry.crop.rect.right = 0.8;
    s.geometry.crop.rect.top = 0.15;
    s.geometry.crop.rect.bottom = 0.95;
    s.geometry.crop.angle = 3.;
    if locals {
        s.locals.adjustments.push(LocalAdjustment {
            components: vec![MaskComponent::new(MaskKind::Radial {
                center: mask_center,
                radii: [0.12, 0.16],
                angle: 15.,
                feather: 30.,
            })],
            params: LocalParams {
                exposure: 1.,
                ..Default::default()
            },
            ..Default::default()
        });
        s.locals.adjustments.push(LocalAdjustment {
            components: vec![MaskComponent::new(MaskKind::Linear {
                start: [0.3, 0.0],
                end: [0.3, 0.35],
            })],
            params: LocalParams {
                exposure: -1.,
                ..Default::default()
            },
            ..Default::default()
        });
    }
    s
}

/// Mean luminance per cell of the cropped output on a fixed normalized grid.
fn cells(image: &RawImage, s: &DevelopSettings) -> ((u32, u32), Vec<f64>) {
    let renderer = Renderer::new(Default::default());
    let extent = Renderer::output_extent(image, s, 0).unwrap();
    let tiles = renderer
        .render_region(image, s, 0, PixelRect::full(extent))
        .unwrap();
    let rgb = common::assemble_u8(extent, &tiles);
    let (w, h) = (extent.width as usize, extent.height as usize);
    let mut sum = vec![0f64; GRID.0 * GRID.1];
    let mut count = vec![0f64; GRID.0 * GRID.1];
    for y in 0..h {
        for x in 0..w {
            let cell = (y * GRID.1) / h * GRID.0 + (x * GRID.0) / w;
            count[cell] += 1.;
            sum[cell] += (0..3)
                .map(|c| f64::from(rgb[(y * w + x) * 3 + c]))
                .sum::<f64>()
                / 3.;
        }
    }
    let means = sum.iter().zip(&count).map(|(s, n)| s / n).collect();
    ((extent.width, extent.height), means)
}

fn zscore(v: &[f64]) -> Vec<f64> {
    let n = v.len() as f64;
    let mean = v.iter().sum::<f64>() / n;
    let sd = (v.iter().map(|x| (x - mean).powi(2)).sum::<f64>() / n).sqrt();
    v.iter().map(|x| (x - mean) / sd).collect()
}

fn correlation(a: &[f64], b: &[f64]) -> f64 {
    let (a, b) = (zscore(a), zscore(b));
    a.iter().zip(&b).map(|(a, b)| a * b).sum::<f64>() / a.len() as f64
}

/// Where the local adjustments act: the change they make, normalized.
fn effect(image: &RawImage, mask_center: [f32; 2]) -> ((u32, u32), Vec<f64>) {
    let (extent, with) = cells(image, &edits(mask_center, true));
    let (_, without) = cells(image, &edits(mask_center, false));
    (
        extent,
        with.iter().zip(&without).map(|(a, b)| a - b).collect(),
    )
}

/// The original as a phone writes it: stored pixels plus an EXIF tag.
fn phone_jpeg(orientation: u16) -> Vec<u8> {
    let pixels = image::RgbImage::from_fn(W, H, |x, y| {
        let l = scene(x as f32 / W as f32, y as f32 / H as f32);
        // sRGB encoding of the scene-linear value.
        let e = if l <= 0.003_130_8 {
            12.92 * l
        } else {
            1.055 * l.powf(1. / 2.4) - 0.055
        };
        let v = (e * 255.).round().clamp(0., 255.) as u8;
        image::Rgb([v, v, v])
    });
    common::exif_jpeg(&pixels, Some(orientation))
}

/// Lightroom's Smart Preview of the same photo: a half-size LinearRaw DNG of
/// the stored (unrotated) pixels.
fn smart_preview() -> pipeline_cpu::CameraLinearProxy {
    let (pw, ph) = (W / 2, H / 2);
    let mut dng =
        raw_decode::lossy_dng::read(&mut std::io::Cursor::new(support::lossy_dng(false, false)))
            .unwrap()
            .unwrap();
    dng.width = pw as usize;
    dng.height = ph as usize;
    dng.pixels = (0..ph)
        .flat_map(|y| (0..pw).map(move |x| (x, y)))
        .map(|(x, y)| [scene((x as f32 + 0.5) / pw as f32, (y as f32 + 0.5) / ph as f32); 3])
        .collect();
    dng.metadata.width = pw;
    dng.metadata.height = ph;
    dng.metadata.default_crop = [0, 0, pw, ph];
    dng.metadata.opcode_lists = [None, None, None];
    pipeline_cpu::CameraLinearProxy::from_dng(dng).unwrap()
}

#[test]
fn lr8n_rotated_jpeg_proxy_edits_stay_on_the_same_content_after_relink() {
    let dir = tempfile::tempdir().unwrap();
    for orientation in [3u16, 6, 8] {
        let id = 8000 + 10 * u128::from(orientation);
        let proxy = RawImage::from_camera_linear_proxy(
            ImageId(id),
            ImageId(id + 1),
            Arc::new(
                smart_preview()
                    .with_catalog_orientation(orientation)
                    .unwrap(),
            ),
        )
        .unwrap();
        let path = dir.path().join(format!("phone-{orientation}.jpg"));
        std::fs::write(&path, phone_jpeg(orientation)).unwrap();
        let original =
            RawImage::open_with_catalog_orientation(ImageId(id + 2), &path, Some(orientation))
                .unwrap();
        assert_eq!(proxy.metadata().orientation, orientation);
        assert_eq!(
            original.metadata().orientation,
            orientation,
            "the catalog orientation is the display orientation on both sides"
        );
        assert_eq!(
            (
                original.active_extent().width,
                original.active_extent().height
            ),
            (W, H),
            "orientation {orientation}: the original is read in the stored frame"
        );

        // Crop: same normalized rectangle, same aspect, same content.
        let s = edits([0.3, 0.6], true);
        let ((pw, ph), on_proxy) = cells(&proxy, &s);
        let ((ow, oh), on_original) = cells(&original, &s);
        let aspect = |w: u32, h: u32| f64::from(w) / f64::from(h);
        assert!(
            (aspect(pw, ph) - aspect(ow, oh)).abs() < 2. / f64::from(ph),
            "orientation {orientation}: crop aspect {pw}x{ph} on the proxy vs {ow}x{oh} relinked"
        );
        assert!(ow > pw, "the original renders at full resolution");
        let content = correlation(&on_proxy, &on_original);
        eprintln!("orientation {orientation}: cropped content correlation {content:.4}");
        assert!(
            content > 0.97,
            "orientation {orientation}: the crop frames different content after relink ({content})"
        );
        // Control: the pre-LR-8n relink read the original in the rotated
        // (EXIF-consumed) frame. That frame transposes the crop aspect for
        // 6 and 8, and frames other content for 3, so this test detects it.
        let rotated = RawImage::open(ImageId(id + 3), &path).unwrap();
        let ((rw, rh), on_rotated) = cells(&rotated, &s);
        if orientation >= 5 {
            assert!((aspect(pw, ph) - aspect(rw, rh)).abs() > 0.2);
        } else {
            let wrong = correlation(&on_proxy, &on_rotated);
            eprintln!("orientation {orientation}: rotated-frame control {wrong:.4}");
            assert!(wrong < 0.9, "control: {wrong}");
        }

        // Masks: the local adjustments change the same cells on both sides.
        let (_, proxy_effect) = effect(&proxy, [0.3, 0.6]);
        let (_, original_effect) = effect(&original, [0.3, 0.6]);
        let masks = correlation(&proxy_effect, &original_effect);
        // Sensitivity control: a radial mask moved by 4% of the frame.
        let (_, moved) = effect(&proxy, [0.34, 0.6]);
        let control = correlation(&moved, &original_effect);
        eprintln!(
            "orientation {orientation}: mask effect correlation {masks:.4} (4% shift control {control:.4})"
        );
        assert!(
            masks > 0.97,
            "orientation {orientation}: masks moved after relink ({masks})"
        );
        assert!(
            control < masks - 0.03,
            "control: the comparison must detect a 4% mask shift ({control} vs {masks})"
        );
    }
}
