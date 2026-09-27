//! Interactive geometry must use the resident export composition at L2.
#[path = "../../image-core/tests/common/mod.rs"]
mod common;
use engine_api::{
    jobs::CancellationToken,
    recipe::{
        DevelopSettings,
        settings::{GuideLine, LensProfileSource, NormalizedRect, UprightMode},
    },
};
use image_core::{PixelRect, Renderer, RendererConfig};
use pipeline_gpu::{GpuContext, GpuStageOp};
use std::sync::Arc;

#[test]
fn l2_interactive_composed_geometry_matches_resident_export() {
    let context = GpuContext::new().expect("M2-49 parity requires a GPU adapter");
    let renderer = Renderer::with_ops(
        Arc::new(GpuStageOp::new(Arc::new(context))),
        Arc::new(image_core::TileCache::new(64 << 20)),
        RendererConfig::default(),
    );
    let raw = common::synthetic(249, 512, 384, common::RGGB, [0, 0, 512, 384]);
    let mut s = DevelopSettings::default();
    s.lens.profile = LensProfileSource::None;
    s.lens.remove_chromatic_aberration = false;
    s.geometry.upright.mode = UprightMode::Guided;
    s.geometry.upright.guides = vec![
        GuideLine {
            start: [0.2, 0.1],
            end: [0.3, 0.9],
        },
        GuideLine {
            start: [0.8, 0.1],
            end: [0.7, 0.9],
        },
    ];
    s.geometry.transform.rotate = 2.;
    s.geometry.crop.rect = NormalizedRect {
        left: 0.1,
        top: 0.1,
        right: 0.9,
        bottom: 0.9,
    };
    let resolved = pipeline_cpu::resolve_lens_sensor(
        raw.cfa().pyramid().pixels(),
        raw.metadata(),
        &s,
        &Default::default(),
    )
    .unwrap();
    let plan = resolved.plan(&s, raw.metadata()).unwrap().unwrap();
    assert!(renderer.can_render_resident(&raw, &s).unwrap());
    let rect = PixelRect::full(Renderer::output_extent(&raw, &s, 2).unwrap());
    let actual = renderer.render_region(&raw, &s, 2, rect).unwrap();
    let cpu = Renderer::new(RendererConfig::default())
        .render_region(&raw, &s, 2, rect)
        .unwrap();
    let mut cpu_error = 0u8;
    for (a, c) in actual.iter().zip(&cpu) {
        assert_eq!(a.layout(), c.layout());
        for (a, c) in a
            .samples::<u8>()
            .unwrap()
            .iter()
            .zip(c.samples::<u8>().unwrap())
        {
            cpu_error = cpu_error.max(a.abs_diff(*c));
        }
    }
    assert!(
        cpu_error <= 3,
        "L2 interactive/CPU max code-value error: {cpu_error}"
    );
    let expected = renderer
        .render_resident_lens(&raw, &s, 2, rect, Some(&plan), &CancellationToken::new())
        .unwrap()
        .unwrap();
    assert_eq!(actual.len(), expected.len());
    let mut max_error = 0u8;
    for (a, e) in actual.iter().zip(&expected) {
        assert_eq!(a.layout(), e.layout());
        for (a, e) in a
            .samples::<u8>()
            .unwrap()
            .iter()
            .zip(e.samples::<u8>().unwrap())
        {
            max_error = max_error.max(a.abs_diff(*e));
        }
    }
    assert!(
        max_error <= 2,
        "L2 interactive/export code-value max error: {max_error}"
    );
}

#[test]
fn scalar_composed_lens_geometry_matches_export() {
    let raw = common::synthetic(250, 96, 72, common::RGGB, [0, 0, 96, 72]);
    let mut s = DevelopSettings::default();
    s.lens.profile = LensProfileSource::None;
    s.lens.remove_chromatic_aberration = false;
    s.lens.manual_distortion = 18.;
    s.lens.manual_vignetting = 35.;
    s.geometry.upright.mode = UprightMode::Guided;
    s.geometry.upright.guides = vec![
        GuideLine {
            start: [0.2, 0.1],
            end: [0.3, 0.9],
        },
        GuideLine {
            start: [0.8, 0.1],
            end: [0.7, 0.9],
        },
    ];
    s.geometry.transform.rotate = 2.;
    s.geometry.crop.rect = NormalizedRect {
        left: 0.1,
        top: 0.1,
        right: 0.9,
        bottom: 0.9,
    };
    for mode in [
        UprightMode::Guided,
        UprightMode::Auto,
        UprightMode::Level,
        UprightMode::Vertical,
        UprightMode::Full,
    ] {
        s.geometry.upright.mode = mode;
        if mode != UprightMode::Guided {
            s.geometry.upright.guides.clear();
        }
        let expected = pipeline_cpu::render_linear_scaled_with_lens(
            &s,
            &pipeline_cpu::RenderSource::Cfa {
                image: raw.cfa(),
                metadata: raw.metadata(),
            },
            1,
            &Default::default(),
        )
        .unwrap();
        let actual = Renderer::new(RendererConfig::default())
            .render_region_as(
                &raw,
                &s,
                0,
                PixelRect::full(Renderer::output_extent(&raw, &s, 0).unwrap()),
                image_core::RenderOutput::SceneLinear,
            )
            .unwrap();
        let reference = expected
            .tile(engine_api::tile::TileCoord::new(0, 0, 0), 0, 1)
            .unwrap();
        let max_error = actual[0]
            .samples::<f32>()
            .unwrap()
            .iter()
            .zip(reference.samples::<f32>().unwrap())
            .map(|(a, b)| (a - b).abs())
            .fold(0f32, f32::max);
        assert!(
            max_error < 0.00001,
            "scalar lens+{mode:?}+transform+crop versus export max error {max_error}"
        );
    }
}
