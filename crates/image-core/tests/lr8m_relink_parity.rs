//! LR-8m (A-LR8 M10): edits made on a 2560 px Smart Preview land in the same
//! normalized place, with the same look, on the full-resolution original once
//! it is relinked. Both carry the same absolute catalog orientation.
mod common;
#[path = "common/raw_fixtures.rs"]
mod raw_fixtures;
#[allow(dead_code)]
#[path = "../../raw-decode/tests/support/mod.rs"]
mod support;

use engine_api::{
    id::ImageId,
    recipe::{
        DevelopSettings, LocalAdjustment, LocalParams, MaskComponent, MaskKind, ProcessVersion,
        settings::UprightMode,
    },
};
use image_core::{PixelRect, RawImage, Renderer};
use std::{path::Path, sync::Arc};

const GRID: (usize, usize) = (48, 32);

fn edits(mask_center: [f32; 2]) -> DevelopSettings {
    let mut s = DevelopSettings::default();
    s.detail.sharpening.amount = 0.;
    s.detail.noise_reduction.color = 0.;
    s.tone.exposure = 0.3;
    s.tone.contrast = 15.;
    s.geometry.crop.rect.left = 0.1;
    s.geometry.crop.rect.right = 0.85;
    s.geometry.crop.rect.top = 0.15;
    s.geometry.crop.rect.bottom = 0.9;
    s.geometry.crop.angle = 4.;
    s.geometry.upright.mode = UprightMode::Full;
    s.geometry.upright.homography = Some([[1., 0.02, 0.01], [0., 1., 0.02], [0.015, 0., 1.]]);
    s.lens.manual_vignetting = 20.;
    s.locals.adjustments.push(LocalAdjustment {
        components: vec![MaskComponent::new(MaskKind::Radial {
            center: mask_center,
            radii: [0.12, 0.16],
            angle: 15.,
            feather: 30.,
        })],
        params: LocalParams {
            exposure: 1.5,
            ..Default::default()
        },
        ..Default::default()
    });
    s.locals.adjustments.push(LocalAdjustment {
        components: vec![MaskComponent::new(MaskKind::Linear {
            start: [0.5, 0.0],
            end: [0.5, 0.4],
        })],
        params: LocalParams {
            exposure: -1.,
            ..Default::default()
        },
        ..Default::default()
    });
    s
}

/// Cell means of the cropped display output on a fixed normalized grid, so a
/// 2560 px render and a full-resolution render compare in the same frame.
fn cells(image: &RawImage, s: &DevelopSettings) -> ((u32, u32), Vec<f64>) {
    let renderer = Renderer::new(Default::default());
    let extent = Renderer::output_extent(image, s, 0).unwrap();
    let tiles = renderer
        .render_region(image, s, 0, PixelRect::full(extent))
        .unwrap();
    let rgb = common::assemble_u8(extent, &tiles);
    let (w, h) = (extent.width as usize, extent.height as usize);
    let mut sum = vec![0f64; GRID.0 * GRID.1 * 3];
    let mut count = vec![0f64; GRID.0 * GRID.1];
    for y in 0..h {
        let cy = (y * GRID.1) / h;
        for x in 0..w {
            let cx = (x * GRID.0) / w;
            let cell = cy * GRID.0 + cx;
            count[cell] += 1.;
            for c in 0..3 {
                sum[cell * 3 + c] += f64::from(rgb[(y * w + x) * 3 + c]);
            }
        }
    }
    let means = sum
        .iter()
        .enumerate()
        .map(|(i, v)| v / count[i / 3])
        .collect();
    ((extent.width, extent.height), means)
}

fn compare(a: &[f64], b: &[f64]) -> (f64, f64) {
    let diffs: Vec<f64> = a.iter().zip(b).map(|(a, b)| (a - b).abs()).collect();
    (
        diffs.iter().sum::<f64>() / diffs.len() as f64,
        diffs.iter().copied().fold(0., f64::max),
    )
}

/// Build an external LinearRaw Smart Preview (Lightroom's 2560 px tier) from
/// the original's own camera-linear pixels and metadata.
fn smart_preview(path: &Path) -> pipeline_cpu::CameraLinearProxy {
    let mut raw = raw_decode::RawSource::open(path).unwrap();
    let cfa = raw.decode_cfa().unwrap();
    let metadata = raw.metadata();
    let mut base = DevelopSettings::default();
    base.detail.sharpening.amount = 0.;
    base.detail.noise_reduction.color = 0.;
    let generated = pipeline_cpu::CameraLinearProxy::generate(
        &cfa,
        &metadata,
        &base,
        ProcessVersion::NATIVE_CURRENT,
        [7; 32],
        &Default::default(),
    )
    .unwrap();
    let pixels = generated.pixels();
    let (pw, ph) = (pixels.width(), pixels.height());
    assert!(pw.max(ph) <= 2560, "Lightroom's Smart Preview tier");
    let mut dng =
        raw_decode::lossy_dng::read(&mut std::io::Cursor::new(support::lossy_dng(false, false)))
            .unwrap()
            .unwrap();
    dng.width = pw as usize;
    dng.height = ph as usize;
    dng.pixels = (0..(pw * ph) as usize)
        .map(|i| std::array::from_fn(|c| pixels.planes()[c][i]))
        .collect();
    dng.baseline_exposure = metadata.baseline_exposure;
    dng.metadata = metadata;
    dng.metadata.width = pw;
    dng.metadata.height = ph;
    dng.metadata.default_crop = [0, 0, pw, ph];
    dng.metadata.opcode_lists = [None, None, None];
    pipeline_cpu::CameraLinearProxy::from_dng(dng).unwrap()
}

fn assert_relink_parity(path: &Path, id: u128) {
    let orientation = 6;
    let proxy = RawImage::from_camera_linear_proxy(
        ImageId(id),
        ImageId(id + 1),
        Arc::new(
            smart_preview(path)
                .with_catalog_orientation(orientation)
                .unwrap(),
        ),
    )
    .unwrap();
    let original =
        RawImage::open_with_catalog_orientation(ImageId(id + 2), path, Some(orientation)).unwrap();
    assert_eq!(
        proxy.metadata().orientation,
        original.metadata().orientation
    );
    let tier = proxy.active_extent();
    assert_eq!(
        tier.width.max(tier.height),
        original
            .active_extent()
            .width
            .max(original.active_extent().height)
            .div_ceil(
                original
                    .active_extent()
                    .width
                    .max(original.active_extent().height)
                    .div_ceil(2560)
            ),
        "Lightroom's 2560 px Smart Preview tier"
    );
    let s = edits([0.3, 0.6]);
    let ((pw, ph), on_proxy) = cells(&proxy, &s);
    let ((ow, oh), on_original) = cells(&original, &s);
    // Same normalized crop: the aspect ratio agrees to within a pixel.
    let aspect = |w: u32, h: u32| f64::from(w) / f64::from(h);
    assert!(
        (aspect(pw, ph) - aspect(ow, oh)).abs() < 2. / f64::from(ph),
        "crop aspect {pw}x{ph} vs {ow}x{oh}"
    );
    assert!(ow > pw, "the original renders at full resolution");
    let (mean, max) = compare(&on_proxy, &on_original);
    eprintln!("proxy vs relinked original: mean {mean:.3}, max {max:.3} levels");
    assert!(
        mean < 1.5 && max < 8.,
        "edits differ after relink: mean {mean}, max {max}"
    );
    // Sensitivity control: the same comparison detects a mask misplaced by
    // 4% of the frame, so the bounds above cannot hide a frame error.
    let (_, moved) = cells(&proxy, &edits([0.34, 0.6]));
    let (_, max_moved) = compare(&moved, &on_original);
    assert!(max_moved > 3. * 8., "control: {max_moved}");
}

#[test]
fn lr8m_proxy_edits_land_on_the_relinked_full_resolution_original() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("original.dng");
    // 5120 x 3412 active area: the 2560 px tier halves it exactly.
    std::fs::write(
        &path,
        support::bayer_dng_sized(1, 5120, 3412, |x, y| {
            let (u, v) = (x as f32 / 5120., y as f32 / 3412.);
            let gain = [0.5, 1.0, 1.0, 0.7][((y % 2) * 2 + x % 2) as usize];
            let scene = 0.08
                + 0.45 * u
                + 0.2 * v
                + 0.1 * (u * 37.).sin() * (v * 23.).cos()
                + if (u - 0.7).powi(2) + (v - 0.3).powi(2) < 0.01 {
                    0.3
                } else {
                    0.
                };
            (gain * scene * 60000.).clamp(0., 65535.) as u16
        }),
    )
    .unwrap();
    assert_relink_parity(&path, 7500);
}

/// The same parity on a real camera file (ENG-6 fixture rules: a visible
/// SKIPPED line when fixtures/raw is absent, a failure when required).
#[test]
fn lr8m_proxy_edits_land_on_a_relinked_real_raw() {
    let path = raw_fixtures::root().join("sample.dng");
    if !path.exists() {
        raw_fixtures::skipped(
            "lr8m_proxy_edits_land_on_a_relinked_real_raw",
            "sample.dng is absent",
        );
        return;
    }
    assert_relink_parity(&path, 7600);
}
