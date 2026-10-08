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
