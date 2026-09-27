mod common;
use common::*;
use compositor::{gpu::GpuCompositor, render::styles::*, resident::ResidentRenderer, *};
use engine_api::tile::Extent;

#[test]
fn styled_cache_reuses_unrelated_edits_and_obeys_budget() {
    let gpu = GpuCompositor::new().expect("M5-31 requires a real GPU");
    let e = Extent::new(64, 32);
    let mut d = doc(e, Depth::F32);
    let back = add(
        &mut d,
        None,
        layer_fn("back", e, Depth::F32, |_, _| [0.1, 0.2, 0.3, 0.8]),
    );
    let styled = add(
        &mut d,
        None,
        layer_fn("styled", e, Depth::F32, |x, _| {
            [0.6, 0.3, 0.1, if x < 32 { 0.5 } else { 0.0 }]
        }),
    );
    set_props(&mut d, styled, |p| {
        p.styles.effects = vec![StyleEffect::OuterGlow(Glow::default())]
    });
    let mut r = ResidentRenderer::new(&gpu).unwrap();
    r.render(&d, 0).unwrap();
    assert_eq!(r.style_evaluations(), 1);
    r.invalidate(); // Exercise actual plane cache, not idle-frame short circuit.
    r.render(&d, 0).unwrap();
    assert_eq!(r.style_evaluations(), 1);
    set_props(&mut d, back, |p| p.opacity = 0.5);
    r.render(&d, 0).unwrap();
    assert_eq!(r.style_evaluations(), 1);
    let op = paint_op(
        d.state(),
        styled,
        PaintTarget::Content,
        Rect::new(30, 0, 34, 32),
        |_, _, p| p[3] = 0.9,
    )
    .unwrap();
    d.apply(op).unwrap();
    r.render(&d, 0).unwrap();
    assert_eq!(r.style_evaluations(), 2);
    let (_, want) = Compositor::new(1 << 20).render_level_rgba(&d, 0).unwrap();
    let (_, got) = r.read_level(0, false).unwrap();
    for (a, b) in got.iter().zip(want) {
        assert!((a - b).abs() <= 1e-4);
    }
    let mut bounded = ResidentRenderer::with_budget(&gpu, 1024).unwrap();
    for count in 1..=2 {
        bounded.invalidate();
        bounded.render(&d, 0).unwrap();
        assert_eq!(bounded.style_evaluations(), count);
        assert_eq!(
            bounded.style_cache_bytes(),
            0,
            "oversized planes must not be retained"
        );
    }
}
