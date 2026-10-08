//! ENG-8: the GPU-resident engine applies a raw's maker-note built-in lens
//! correction in lens mode `None` and by default, through the resolved lens
//! plan, matching the CPU renderer. The lens-free resident fast path must not
//! take such a raw (it would silently drop the correction).
#[path = "../../image-core/tests/common/mod.rs"]
mod common;
use engine_api::{
    id::ImageId,
    jobs::CancellationToken,
    recipe::{DevelopSettings, settings::LensProfileSource},
};
use image_core::{PixelRect, RawImage, Renderer, RendererConfig};
use pipeline_gpu::{GpuContext, GpuStageOp};
use raw_decode::{FujifilmLens, MakerLens};
use std::sync::Arc;
use test_fixtures::raw as raw_fixtures;

fn gpu() -> Renderer {
    Renderer::with_ops(
        Arc::new(GpuStageOp::new(Arc::new(
            GpuContext::new().expect("ENG-8 parity requires a GPU adapter"),
        ))),
        Arc::new(image_core::TileCache::new(64 << 20)),
        RendererConfig::default(),
    )
}

/// A strong Fujifilm-format correction (pincushion, CA, falloff).
fn strong() -> MakerLens {
    MakerLens::Fujifilm(FujifilmLens {
        knots: (0..=10).map(|i| i as f64 / 10.).collect(),
        distortion: (0..=10).map(|i| 6. * (i as f64 / 10.).powi(2)).collect(),
        ca_red: (0..=10).map(|i| 0.002 * i as f64 / 10.).collect(),
        ca_blue: (0..=10).map(|i| -0.002 * i as f64 / 10.).collect(),
        vignetting: (0..=10)
            .map(|i| 100. - 30. * (i as f64 / 10.).powi(2))
            .collect(),
        crop_factor: 1.,
    })
}

fn display(r: &Renderer, raw: &RawImage, s: &DevelopSettings, level: u8) -> Vec<u8> {
    let e = Renderer::output_extent(raw, s, level).unwrap();
    common::assemble_u8(
        e,
        &r.render_region(raw, s, level, PixelRect::full(e)).unwrap(),
    )
}

fn max_diff(a: &[u8], b: &[u8]) -> u8 {
    assert_eq!(a.len(), b.len());
    a.iter().zip(b).map(|(a, b)| a.abs_diff(*b)).max().unwrap()
}

#[test]
fn gpu_applies_maker_note_correction_in_profile_none_like_cpu() {
    let plain = common::synthetic(8801, 512, 384, common::RGGB, [0, 0, 512, 384]);
    let mut m = plain.metadata().clone();
    m.make = "FUJIFILM".into();
    m.maker_lens = Some(strong());
    let corrected = plain.with_metadata(ImageId(8802), Arc::new(m)).unwrap();
    let renderer = gpu();
    let cpu = Renderer::new(RendererConfig::default());
    for profile in [LensProfileSource::None, LensProfileSource::Auto] {
        let mut s = DevelopSettings::default();
        s.lens.profile = profile.clone();
        let rect = PixelRect::full(corrected.level_extent(0));
        // The lens-free resident region would skip the correction: declined.
        assert!(
            renderer
                .render_resident_region(&corrected, &s, 0, rect, &CancellationToken::new())
                .unwrap()
                .is_none(),
            "{profile:?}: lens-free resident path took a maker-note raw"
        );
        for level in [0, 2] {
            let gpu = display(&renderer, &corrected, &s, level);
            let reference = display(&cpu, &corrected, &s, level);
            let error = max_diff(&gpu, &reference);
            let moved = max_diff(&gpu, &display(&renderer, &plain, &s, level));
            eprintln!("{profile:?} L{level}: GPU vs CPU {error}, vs uncorrected {moved}");
            assert!(
                error <= 3,
                "{profile:?} L{level}: GPU/CPU code-value error {error}"
            );
            assert!(moved >= 10, "{profile:?} L{level}: correction not applied");
        }
    }
}

#[test]
fn gpu_raf_fixture_matches_cpu_with_built_in_correction() {
    let test = "gpu_raf_fixture_matches_cpu_with_built_in_correction";
    let Some(path) = raw_fixtures::with_extension(test, "raf") else {
        return;
    };
    let raw = RawImage::open(ImageId(8803), &path).unwrap();
    assert!(raw.metadata().maker_lens.is_some());
    let renderer = gpu();
    // ENG-8c: the raw-prefix correction is a resident GPU stage (the
    // sensor-frame resample), not a fallback to the CPU chain.
    assert!(
        renderer
            .can_render_resident(&raw, &DevelopSettings::default())
            .unwrap(),
        "the RAF left the resident GPU path"
    );
    let cpu = Renderer::new(RendererConfig::default());
    for profile in [LensProfileSource::Auto, LensProfileSource::None] {
        let mut s = DevelopSettings::default();
        s.lens.profile = profile.clone();
        let error = max_diff(
            &display(&renderer, &raw, &s, 3),
            &display(&cpu, &raw, &s, 3),
        );
        eprintln!("RAF {profile:?} L3: GPU vs CPU {error}");
        assert!(
            error <= 3,
            "RAF {profile:?} L3: GPU/CPU code-value error {error}"
        );
    }
}

/// A Fujifilm-format barrel with `corner` percent distortion at the corner
/// (an r² profile), the shape of a wide zoom's correction.
fn barrel(corner: f64) -> MakerLens {
    MakerLens::Fujifilm(FujifilmLens {
        knots: (0..=10).map(|i| i as f64 / 10.).collect(),
        distortion: (0..=10)
            .map(|i| corner * (i as f64 / 10.).powi(2))
            .collect(),
        ca_red: (0..=10).map(|i| 3e-4 * i as f64 / 10.).collect(),
        ca_blue: (0..=10).map(|i| -3e-4 * i as f64 / 10.).collect(),
        vignetting: (0..=10)
            .map(|i| 100. - 20. * (i as f64 / 10.).powi(2))
            .collect(),
        crop_factor: 1.,
    })
}

/// REV3-ENG-8 NS3: real wide-zoom corrections (-4 %, -6 %: ~77-113 px on
/// this 16 MP sensor) stay on the resident GPU path, with a gather halo
/// sized from the displacement, and match the CPU reference within 1 code
/// value. N1: a correction beyond the resident limit makes the renderer
/// report it (`can_render_resident` false) and render on the reference chain
/// instead of failing.
#[test]
fn gpu_raf_with_wide_zoom_barrels_stays_resident() {
    let test = "gpu_raf_with_wide_zoom_barrels_stays_resident";
    let Some(path) = raw_fixtures::with_extension(test, "raf") else {
        return;
    };
    let raw = RawImage::open(ImageId(8804), &path).unwrap();
    let renderer = gpu();
    let cpu = Renderer::new(RendererConfig::default());
    let s = DevelopSettings::default();
    for (i, (corner, resident)) in [(-4., true), (-6., true), (-40., false)]
        .into_iter()
        .enumerate()
    {
        let mut m = raw.metadata().clone();
        m.maker_lens = Some(barrel(corner));
        let image = raw
            .with_metadata(ImageId(8805 + i as u128), Arc::new(m))
            .unwrap();
        let resolved = pipeline_cpu::resolve_lens_sensor(
            image.cfa().pyramid().pixels(),
            image.metadata(),
            &s,
            &Default::default(),
        )
        .unwrap();
        let plan = resolved.plan(&s, image.metadata()).unwrap().unwrap();
        let ca = plan.ca.as_ref().unwrap();
        let sensor = image.sensor_extent();
        let displacement = ca.max_displacement(sensor.width, sensor.height).unwrap();
        assert_eq!(
            renderer.can_render_resident(&image, &s).unwrap(),
            resident,
            "{corner} %: displacement {displacement:.0} px"
        );
        let metrics = renderer
            .render_output_metrics(&image, &s, &CancellationToken::new())
            .unwrap();
        assert_eq!(metrics.is_some(), resident, "{corner} %: resident render");
        let started = std::time::Instant::now();
        let gpu_l2 = display(&renderer, &image, &s, 2);
        let gpu_time = started.elapsed();
        for level in [2, 3] {
            let (ga, cb) = (
                display(&renderer, &image, &s, level),
                display(&cpu, &image, &s, level),
            );
            let error = max_diff(&ga, &cb);
            eprintln!(
                "{corner} % barrel (displacement {displacement:.0} px, halo {:?}) L{level}: GPU vs CPU {error}, GPU L2 {gpu_time:?}",
                ca.halo(sensor.width, sensor.height)
            );
            assert!(
                error <= 1,
                "{corner} % L{level}: GPU/CPU code-value error {error}"
            );
        }
        drop(gpu_l2);
    }
}
