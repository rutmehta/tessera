use super::*;
use engine_api::{
    id::ImageId,
    recipe::settings::{LensProfileSource, UprightMode},
};

fn nonresident_fixture() -> (RawImage, DevelopSettings) {
    let (w, h) = (192, 160);
    let plane = (0..w * h)
        .map(|i| {
            let (x, y) = ((i % w) as f32, (i / w) as f32);
            if (y - 0.17 * x).rem_euclid(32.) < 4. {
                0.8
            } else {
                0.1
            }
        })
        .collect::<Vec<_>>();
    let rgb = crate::RgbSource::from_linear_rec2020(
        pipeline_cpu::Image::new(w, h, vec![plane; 3]).unwrap(),
    )
    .unwrap();
    let image = RawImage::from_rgb(ImageId(9049), rgb).unwrap();
    let mut s = DevelopSettings::default();
    s.lens.profile = LensProfileSource::None;
    s.lens.remove_chromatic_aberration = false;
    s.lens.manual_distortion = 8.;
    s.lens.defringe_purple.amount = 10.;
    s.geometry.upright.mode = UprightMode::Level;
    (image, s)
}

#[test]
fn nonresident_upright_analysis_is_cached_across_manual_transforms() {
    let (image, mut s) = nonresident_fixture();
    let ops = Arc::new(crate::CountingStageOp::new(crate::CpuStageOp));
    let r = Renderer::with_ops(
        ops.clone(),
        Arc::new(TileCache::new(64 << 20)),
        RendererConfig::default(),
    );
    let cancel = CancellationToken::new();
    assert!(
        r.interactive_lens_plan(&image, &s, &cancel)
            .unwrap()
            .is_none()
    );
    assert!(
        r.geometry_analysis.lock().unwrap().is_some(),
        "nonresident optics must retain the L0 analysis"
    );
    ops.reset();
    s.geometry.transform.rotate = 3.;
    s.geometry.transform.vertical = 12.;
    assert!(
        r.interactive_lens_plan(&image, &s, &cancel)
            .unwrap()
            .is_none()
    );
    assert_eq!(
        ops.count(StageId::Detail),
        0,
        "manual edits must not rerun L0 analysis"
    );
}

#[test]
fn nonresident_scalar_preview_uses_l0_upright_with_manual_transforms() {
    let (image, mut s) = nonresident_fixture();
    let r = Renderer::new(RendererConfig::default());
    let cancel = CancellationToken::new();
    let resolved = r.resolve_interactive_lens(&image, &s).unwrap();
    let analysis = r.develop_before_geometry(&image, &s, 0, &cancel).unwrap();
    // Defringe prevents a resident plan, but does not change the final inverse
    // optical map. Remove it only for the independent L0 reference map.
    let mut portable = s.clone();
    portable.lens.defringe_purple.amount = 0.;
    let original = resolved
        .plan_with_analysis(&portable, image.metadata(), &analysis)
        .unwrap()
        .unwrap();
    let mut differs_from_preview_analysis = false;
    for (level, rotate) in [(1, 0.), (2, 3.), (1, -2.)] {
        s.geometry.transform.rotate = rotate;
        portable.geometry = s.geometry.clone();
        let preview = r
            .develop_before_geometry(&image, &s, level, &cancel)
            .unwrap();
        let plan = resolved
            .plan_with_cached_upright(&portable, image.metadata(), &original)
            .unwrap()
            .unwrap();
        let expected = plan.map.unwrap().apply(&preview).unwrap();
        let level_specific = resolved.apply_geometry(&preview, &s).unwrap();
        differs_from_preview_analysis |= expected
            .planes()
            .iter()
            .flatten()
            .zip(level_specific.planes().iter().flatten())
            .any(|(a, b)| (a - b).abs() > 1e-4);
        let mut got = pipeline_cpu::Image::new(
            expected.width(),
            expected.height(),
            vec![vec![0.; (expected.width() * expected.height()) as usize]; 3],
        )
        .unwrap();
        r.run_m2(
            &image,
            &s,
            &[TileCoord::new(level, 0, 0)],
            RenderOutput::SceneLinear,
            &cancel,
            &mut |t| {
                let t = Tile::from_samples(
                    TileCoord::new(0, 0, 0),
                    t.layout(),
                    t.samples::<f32>().unwrap().to_vec(),
                )
                .unwrap();
                got.put(&t).unwrap();
            },
        )
        .unwrap();
        let max = got
            .planes()
            .iter()
            .flatten()
            .zip(expected.planes().iter().flatten())
            .map(|(a, b)| (a - b).abs())
            .fold(0f32, f32::max);
        assert!(
            max < 1e-6,
            "L{level} must reuse L0 Upright; max error {max}"
        );
    }
    assert!(
        differs_from_preview_analysis,
        "fixture must distinguish L0 from preview analysis"
    );
}

#[test]
fn saved_upright_skips_interactive_l0_analysis() {
    let (image, mut s) = nonresident_fixture();
    let ops = Arc::new(crate::CountingStageOp::new(crate::CpuStageOp));
    let r = Renderer::with_ops(
        ops.clone(),
        Arc::new(TileCache::new(0)),
        RendererConfig::default(),
    );
    s.geometry.upright.mode = UprightMode::Auto;
    s.geometry.upright.homography_mode = Some(UprightMode::Auto);
    s.geometry.upright.homography = Some([[1., 0., 0.], [0., 1., 0.], [0.2, 0., 1.]]);
    assert!(
        r.interactive_upright_analysis(&image, &s, &CancellationToken::new())
            .unwrap()
            .is_none()
    );
    assert_eq!(ops.counts(), [0; StageId::COUNT]);
}

#[test]
fn lr3e_spot_upright_reuses_session_analysis() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    let (image, mut s) = nonresident_fixture();
    s.locals
        .retouch
        .push(engine_api::recipe::mask::RetouchOperation {
            id: engine_api::id::RetouchId(1),
            kind: engine_api::recipe::mask::RetouchKind::Clone {
                source_offset: [0.2, 0.],
            },
            target: engine_api::recipe::mask::RetouchTarget::Area { components: vec![] },
            opacity: 100.,
            feather: 0.,
            enabled: true,
        });
    let full_solves = Arc::new(AtomicUsize::new(0));
    let calls = full_solves.clone();
    let r = Renderer::new(RendererConfig::default()).with_retouch_renderer(Arc::new(
        move |w: u32,
              _: u32,
              planes: &mut [Vec<f32>],
              _: &[engine_api::recipe::mask::RetouchOperation]| {
            if w == 192 {
                calls.fetch_add(1, Ordering::Relaxed);
            }
            for p in planes {
                p[0] = 0.3;
            }
            Ok(())
        },
    ));
    let cancel = CancellationToken::new();
    let mut timings = Vec::new();
    let mut solves = Vec::new();
    for rotate in [0., 1., 2., 3., 4.] {
        s.geometry.transform.rotate = rotate;
        let start = std::time::Instant::now();
        r.render_tiles(
            &image,
            &s,
            &[TileCoord::new(1, 0, 0)],
            RenderOutput::SceneLinear,
            &cancel,
            &mut |_| {},
        )
        .unwrap();
        timings.push(start.elapsed().as_secs_f64() * 1000.);
        solves.push(full_solves.load(Ordering::Relaxed));
    }
    eprintln!("LR-3e spot + Upright frame ms: {timings:?}; L0 solves: {solves:?}");
    assert_eq!(
        solves,
        vec![1; 5],
        "manual transforms must reuse the session's L0 analysis"
    );
    // Public admission must reject invalid settings before any retouch work,
    // even though CPU selection now happens before Upright analysis.
    s.tone.exposure = f32::NAN;
    let before = full_solves.load(Ordering::Relaxed);
    assert!(
        r.render_tiles(
            &image,
            &s,
            &[TileCoord::new(0, 0, 0)],
            RenderOutput::SceneLinear,
            &cancel,
            &mut |_| {}
        )
        .is_err()
    );
    assert_eq!(
        full_solves.load(Ordering::Relaxed),
        before,
        "invalid full settings must not reach the retouch callback"
    );
}
