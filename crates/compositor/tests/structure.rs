//! Groups, Blend If, knockout, adjustments, clipping, masks, fills and smart
//! objects, against hand-computed values (COMPOSITOR.md §2–4).
mod common;
use common::*;
use compositor::*;
use engine_api::tile::Extent;

const RED: [f32; 3] = [0.8, 0.2, 0.2];

fn one(px: [f32; 4]) -> Layer {
    layer_px("l", &[px])
}

fn group_doc(mode: GroupMode, opacity: f32, child: Layer) -> Document {
    let mut d = doc(Extent::new(1, 1), Depth::F32);
    add(&mut d, None, one(opaque(RED)));
    let g = add(&mut d, None, Layer::group("g", mode).with_opacity(opacity));
    add(&mut d, Some(g), child);
    d
}

#[test]
fn group_isolation_vs_pass_through() {
    let mul = || one(opaque([0.5, 0.5, 0.5])).with_mode(BlendMode::Multiply);
    // Pass-through: Multiply sees the backdrop.
    assert_close(
        &render(&group_doc(GroupMode::PassThrough, 1.0, mul())),
        &[0.4, 0.1, 0.1, 1.0],
        1e-6,
        "pass",
    );
    // Isolated: Multiply over transparent = the colour itself, then Normal.
    assert_close(
        &render(&group_doc(GroupMode::Isolated, 1.0, mul())),
        &[0.5, 0.5, 0.5, 1.0],
        1e-6,
        "iso",
    );
    // Group opacity: pass-through fades the group's *effect* against the
    // entry backdrop; isolated fades the group's composite.
    assert_close(
        &render(&group_doc(GroupMode::PassThrough, 0.5, mul())),
        &[0.6, 0.15, 0.15, 1.0],
        1e-6,
        "pass 50%",
    );
    assert_close(
        &render(&group_doc(GroupMode::Isolated, 0.5, mul())),
        &[0.65, 0.35, 0.35, 1.0],
        1e-6,
        "iso 50%",
    );
    // Adjustments inside a pass-through group reach the backdrop; inside an
    // isolated group they only see the (empty) group.
    let inv = || Layer::new("inv", LayerKind::Adjustment(Adjustment::Invert));
    assert_close(
        &render(&group_doc(GroupMode::PassThrough, 1.0, inv())),
        &[0.2, 0.8, 0.8, 1.0],
        1e-6,
        "pass inv",
    );
    assert_close(
        &render(&group_doc(GroupMode::Isolated, 1.0, inv())),
        &[0.8, 0.2, 0.2, 1.0],
        1e-6,
        "iso inv",
    );
    // Isolated group with a non-Normal mode blends its composite.
    let mut d = group_doc(GroupMode::Isolated, 1.0, one(opaque([0.5, 0.5, 0.5])));
    let g = d.state().root[1].id;
    set_props(&mut d, g, |p| p.blend_mode = BlendMode::Screen);
    assert_close(&render(&d), &[0.9, 0.6, 0.6, 1.0], 1e-6, "iso screen");
}

#[test]
fn blend_if_split_sliders_interpolate_linearly() {
    let v = [0.1f32, 0.3, 0.5, 0.7, 0.9];
    let mut d = doc(Extent::new(5, 1), Depth::F32);
    let under: Vec<[f32; 4]> = v.iter().map(|g| opaque([*g; 3])).collect();
    add(&mut d, None, layer_px("under", &under));
    let top = add(&mut d, None, layer_px("white", &[opaque([1.0; 3]); 5]));
    set_props(&mut d, top, |p| {
        p.blend_if.gray.underlying = [0.2, 0.4, 0.6, 0.8]
    });
    // weights 0, .5, 1, .5, 0 → v + w(1 − v)
    let want = [0.1, 0.65, 1.0, 0.85, 0.9];
    let got = render(&d);
    for (i, w) in want.iter().enumerate() {
        assert_close(
            &got[i * 4..i * 4 + 4],
            &[*w, *w, *w, 1.0],
            1e-5,
            &format!("underlying px{i}"),
        );
    }
    // "This layer" on the red channel: layer red values 0.1..0.9 over black,
    // hard (unsplit) range [0.3, 0.7] keeps pixels 1..=3 only.
    let mut d = doc(Extent::new(5, 1), Depth::F32);
    add(&mut d, None, layer_px("black", &[opaque([0.0; 3]); 5]));
    let lay: Vec<[f32; 4]> = v.iter().map(|r| opaque([*r, 1.0, 0.0])).collect();
    let top = add(&mut d, None, layer_px("top", &lay));
    set_props(&mut d, top, |p| {
        p.blend_if.rgb[0].this_layer = [0.3, 0.3, 0.7, 0.7]
    });
    let got = render(&d);
    for (i, r) in v.iter().enumerate() {
        let keep = (0.3..=0.7).contains(r);
        let want = if keep {
            [*r, 1.0, 0.0, 1.0]
        } else {
            [0.0, 0.0, 0.0, 1.0]
        };
        assert_close(
            &got[i * 4..i * 4 + 4],
            &want,
            1e-6,
            &format!("this-layer px{i}"),
        );
    }
}

fn knockout_doc(mode: GroupMode, k: Knockout, fill: f32, opacity: f32) -> Vec<f32> {
    let mut d = doc(Extent::new(1, 1), Depth::F32);
    let bg = add(&mut d, None, one(opaque([0.0, 0.0, 1.0])));
    set_props(&mut d, bg, |p| p.background = true);
    add(&mut d, None, one(opaque([1.0, 0.0, 0.0])));
    let g = add(&mut d, None, Layer::group("g", mode));
    add(&mut d, Some(g), one(opaque([0.0, 1.0, 0.0])));
    let ko = add(&mut d, Some(g), one(opaque([1.0, 1.0, 1.0])));
    set_props(&mut d, ko, |p| {
        p.knockout = k;
        p.fill_opacity = fill;
        p.opacity = opacity;
    });
    render(&d)
}

#[test]
fn knockout_shallow_and_deep() {
    let red = [1.0, 0.0, 0.0, 1.0];
    let blue = [0.0, 0.0, 1.0, 1.0];
    // Isolated group: shallow reveals the group's transparent start → red.
    assert_close(
        &knockout_doc(GroupMode::Isolated, Knockout::Shallow, 0.0, 1.0),
        &red,
        1e-6,
        "iso shallow",
    );
    // Deep reveals the Background layer.
    assert_close(
        &knockout_doc(GroupMode::Isolated, Knockout::Deep, 0.0, 1.0),
        &blue,
        1e-6,
        "iso deep",
    );
    // Pass-through: shallow reveals the backdrop at group entry (red).
    assert_close(
        &knockout_doc(GroupMode::PassThrough, Knockout::Shallow, 0.0, 1.0),
        &red,
        1e-6,
        "pass shallow",
    );
    assert_close(
        &knockout_doc(GroupMode::PassThrough, Knockout::Deep, 0.0, 1.0),
        &blue,
        1e-6,
        "pass deep",
    );
    // Fill 50 %: the layer at fill 0.5 over the knockout backdrop.
    assert_close(
        &knockout_doc(GroupMode::PassThrough, Knockout::Deep, 0.5, 1.0),
        &[0.5, 0.5, 1.0, 1.0],
        1e-6,
        "fill .5",
    );
    // Opacity 50 %: lerp(green, knockout result (blue), 0.5).
    assert_close(
        &knockout_doc(GroupMode::PassThrough, Knockout::Deep, 0.0, 0.5),
        &[0.0, 0.5, 0.5, 1.0],
        1e-6,
        "opacity .5",
    );
    // No knockout: white covers.
    assert_close(
        &knockout_doc(GroupMode::Isolated, Knockout::None, 1.0, 1.0),
        &[1.0, 1.0, 1.0, 1.0],
        1e-6,
        "none",
    );
}

fn adj(a: Adjustment) -> Layer {
    Layer::new("adj", LayerKind::Adjustment(a))
}

#[test]
fn adjustment_layers_stack_in_order() {
    let base = [opaque([0.5, 0.2, 0.8]), opaque([0.35, 0.65, 0.95])];
    let levels = Adjustment::Levels {
        master: LevelsChannel {
            in_black: 0.2,
            in_white: 0.8,
            ..Default::default()
        },
        rgb: Default::default(),
    };
    let mut d = doc(Extent::new(2, 1), Depth::F32);
    add(&mut d, None, layer_px("base", &base));
    add(&mut d, None, adj(levels.clone()));
    add(&mut d, None, adj(Adjustment::Invert));
    add(&mut d, None, adj(Adjustment::Posterize { levels: 3 }));
    let last = add(&mut d, None, adj(Adjustment::Invert));
    set_props(&mut d, last, |p| p.opacity = 0.25);
    // levels → (.5,0,1),(.25,.75,1); invert → (.5,1,0),(.75,.25,0);
    // posterize 3 → (.5,1,0),(1,0,0); invert @25 % → .75x + .25(1−x).
    let want = [0.5, 0.75, 0.25, 1.0, 0.75, 0.25, 0.25, 1.0];
    assert_close(&render(&d), &want, 2e-4, "stack");

    // Reordering changes the result: invert first, then levels.
    let mut d = doc(Extent::new(2, 1), Depth::F32);
    add(&mut d, None, layer_px("base", &base));
    add(&mut d, None, adj(Adjustment::Invert));
    add(&mut d, None, adj(levels));
    // invert → (.5,.8,.2),(.65,.35,.05); levels → (.5,1,0),(.75,.25,0)
    assert_close(
        &render(&d),
        &[0.5, 1.0, 0.0, 1.0, 0.75, 0.25, 0.0, 1.0],
        2e-4,
        "reordered",
    );
}

#[test]
fn clipped_and_masked_adjustments() {
    let mut d = doc(Extent::new(2, 1), Depth::F32);
    add(&mut d, None, layer_px("a", &[opaque([0.2, 0.4, 0.6]); 2]));
    // T covers pixel 0 only; an Invert clipped to T affects only T.
    add(
        &mut d,
        None,
        layer_px("t", &[opaque([0.9, 0.5, 0.1]), [0.0; 4]]),
    );
    let inv = add(&mut d, None, adj(Adjustment::Invert));
    set_props(&mut d, inv, |p| p.clipped = true);
    assert_close(
        &render(&d),
        &[0.1, 0.5, 0.9, 1.0, 0.2, 0.4, 0.6, 1.0],
        1e-6,
        "clipped adj",
    );

    // A masked (unclipped) adjustment: mask hides pixel 1.
    let mut d = doc(Extent::new(2, 1), Depth::F32);
    add(&mut d, None, layer_px("a", &[opaque([0.2, 0.4, 0.6]); 2]));
    let inv = add(&mut d, None, adj(Adjustment::Invert));
    let mut m = Mask::reveal_all(Extent::new(2, 1), Depth::F32);
    m.raster
        .edit_region(Rect::new(1, 0, 2, 1), 1, |_, _, p| p[0] = 0.0)
        .unwrap();
    d.apply(DocOp::SetMask {
        id: inv,
        mask: Some(m),
    })
    .unwrap();
    assert_close(
        &render(&d),
        &[0.8, 0.6, 0.4, 1.0, 0.2, 0.4, 0.6, 1.0],
        1e-6,
        "masked adj",
    );
}

#[test]
fn clipping_mask_limits_to_base_alpha() {
    let mut d = doc(Extent::new(3, 1), Depth::F32);
    add(&mut d, None, layer_px("bg", &[opaque([0.0, 0.0, 1.0]); 3]));
    // Base: opaque, 50 %, transparent.
    add(
        &mut d,
        None,
        layer_px(
            "base",
            &[opaque([1.0, 0.0, 0.0]), [1.0, 0.0, 0.0, 0.5], [0.0; 4]],
        ),
    );
    let c = add(
        &mut d,
        None,
        layer_px("clip", &[opaque([0.0, 1.0, 0.0]); 3]),
    );
    set_props(&mut d, c, |p| p.clipped = true);
    // px0: green; px1: (green at 50 %) over blue; px2: blue (clip hidden).
    let want = [0.0, 1.0, 0.0, 1.0, 0.0, 0.5, 0.5, 1.0, 0.0, 0.0, 1.0, 1.0];
    assert_close(&render(&d), &want, 1e-6, "clip");
    // Hiding the base hides the clipped layer too.
    let base = d.state().root[1].id;
    set_props(&mut d, base, |p| p.visible = false);
    assert_close(
        &render(&d),
        &[0.0, 0.0, 1.0, 1.0].repeat(3),
        1e-6,
        "hidden base",
    );
}

#[test]
fn layer_mask_density_and_fill_layers() {
    let e = Extent::new(2, 1);
    let mut d = doc(e, Depth::F32);
    add(&mut d, None, layer_px("black", &[opaque([0.0; 3]); 2]));
    let f = add(
        &mut d,
        None,
        Layer::new(
            "fill",
            LayerKind::Fill(Fill::Solid {
                color: [1.0, 1.0, 1.0],
            }),
        ),
    );
    let mut m = Mask::hide_all(e, Depth::F32);
    m.raster
        .edit_region(Rect::new(0, 0, 1, 1), 1, |_, _, p| p[0] = 1.0)
        .unwrap();
    m.density = 0.5;
    d.apply(DocOp::SetMask {
        id: f,
        mask: Some(m),
    })
    .unwrap();
    // px0: mask 1 → white; px1: mask 0 at density .5 → 0.5.
    assert_close(
        &render(&d),
        &[1.0, 1.0, 1.0, 1.0, 0.5, 0.5, 0.5, 1.0],
        1e-6,
        "mask density",
    );
    // Gradient fill: linear from x=0 (black) to x=2 (white), sampled at
    // pixel centres 0.5 and 1.5.
    d.apply(DocOp::SetMask { id: f, mask: None }).unwrap();
    d.apply(DocOp::SetFill {
        id: f,
        fill: Fill::Gradient {
            gradient: GradientKind::Linear,
            start: [0.0, 0.0],
            end: [2.0, 0.0],
            stops: vec![
                GradientStop {
                    position: 0.0,
                    color: [0.0, 0.0, 0.0, 1.0],
                },
                GradientStop {
                    position: 1.0,
                    color: [1.0, 1.0, 1.0, 1.0],
                },
            ],
        },
    })
    .unwrap();
    assert_close(
        &render(&d),
        &[0.25, 0.25, 0.25, 1.0, 0.75, 0.75, 0.75, 1.0],
        1e-6,
        "gradient",
    );
}

#[test]
fn mips_are_alpha_weighted_box_averages() {
    let e = Extent::new(4, 2);
    let mut d = doc(e, Depth::F32);
    // 2×2 block 0: red opaque, green opaque, transparent (colour ignored), blue 50 %.
    let pxs = [
        [1.0, 0.0, 0.0, 1.0],
        [0.0, 1.0, 0.0, 1.0],
        [0.0, 0.0, 0.0, 1.0],
        [1.0, 1.0, 1.0, 1.0],
        [0.3, 0.3, 0.3, 0.0],
        [0.0, 0.0, 1.0, 0.5],
        [1.0, 1.0, 1.0, 1.0],
        [1.0, 1.0, 1.0, 1.0],
    ];
    add(
        &mut d,
        None,
        layer_fn("l", e, Depth::F32, move |x, y| pxs[(y * 4 + x) as usize]),
    );
    let c = Compositor::new(1 << 20);
    let (le, img) = c.render_level_rgba(&d, 1).unwrap();
    assert_eq!(le, Extent::new(2, 1));
    // Σα = 2.5, colour = (1, 1, .5)/2.5, α = 2.5/4.
    assert_close(&img[..4], &[0.4, 0.4, 0.2, 0.625], 1e-6, "mip px0");
    assert_close(&img[4..], &[0.75, 0.75, 0.75, 1.0], 1e-6, "mip px1");
}

fn child_doc() -> DocState {
    let e = Extent::new(64, 64);
    let mut s = Document::new(DocState::new(e, Depth::F32));
    s.apply(DocOp::AddLayer {
        parent: None,
        index: 0,
        layer: layer_fn("c", e, Depth::F32, |x, y| {
            [
                x as f32 / 63.0,
                y as f32 / 63.0,
                ((x ^ y) & 7) as f32 / 7.0,
                1.0,
            ]
        }),
    })
    .unwrap();
    (**s.state()).clone()
}

#[test]
fn smart_objects_render_nested_documents_with_transforms() {
    let child = child_doc();
    // Identity transform: pixel-exact copy of the child.
    let mut d = doc(Extent::new(64, 64), Depth::F32);
    let so = add(
        &mut d,
        None,
        Layer::new(
            "so",
            LayerKind::SmartObject(SmartObject::new(child.clone(), Affine::IDENTITY)),
        ),
    );
    let got = render(&d);
    let want = render(&Document::new(child.clone()));
    assert_close(&got, &want, 1e-6, "identity");
    // Half scale into a 32×32 region at (10, 5): samples the child's level-1
    // composite exactly at pixel centres.
    d.apply(DocOp::SetSmartTransform {
        id: so,
        transform: Affine::scale_translate(0.5, 0.5, 10.0, 5.0),
    })
    .unwrap();
    let got = render(&d);
    let child_l1 = Compositor::new(1 << 24)
        .render_level_rgba(&Document::new(child), 1)
        .unwrap()
        .1;
    for y in 0..32 {
        for x in 0..32 {
            assert_close(
                &px(&got, 64, x + 10, y + 5),
                &px(&child_l1, 32, x, y),
                1e-5,
                &format!("({x},{y})"),
            );
        }
    }
    assert_eq!(px(&got, 64, 5, 5)[3], 0.0);
    // Editing inside the smart object invalidates the parent.
    let comp = Compositor::new(1 << 26);
    let before = comp.render_level_rgba(&d, 0).unwrap().1;
    let inner = match &d.state().find(so).unwrap().kind {
        LayerKind::SmartObject(s) => s.state.root[0].id,
        _ => unreachable!(),
    };
    let p = LayerProps {
        opacity: 0.0,
        ..Default::default()
    };
    d.apply(DocOp::EditSmartObject {
        id: so,
        op: Box::new(DocOp::SetProps {
            id: inner,
            props: p,
        }),
    })
    .unwrap();
    let after = comp.render_level_rgba(&d, 0).unwrap().1;
    assert_ne!(before, after);
    assert!(after.chunks(4).all(|p| p[3] == 0.0));
}

#[test]
fn selections_are_float_rasters_with_boolean_ops() {
    use compositor::document::selection::{Combine, combine, rect};
    let e = Extent::new(300, 10);
    let a = rect(e, Rect::new(0, 0, 200, 10)).unwrap();
    let b = rect(e, Rect::new(100, 0, 300, 10)).unwrap();
    let i = combine(&a, &b, Combine::Intersect).unwrap();
    assert_eq!(i.pixel(150, 5)[0], 1.0);
    assert_eq!(i.pixel(50, 5)[0], 0.0);
    let s = combine(&a, &b, Combine::Subtract).unwrap();
    assert_eq!(s.pixel(50, 5)[0], 1.0);
    assert_eq!(s.pixel(150, 5)[0], 0.0);
    let u = combine(&a, &b, Combine::Add).unwrap();
    assert_eq!(u.pixel(250, 5)[0], 1.0);
    let mut d = doc(e, Depth::U8);
    d.apply(DocOp::SetSelection { selection: Some(u) }).unwrap();
    assert!(d.state().selection.is_some());
}

#[test]
fn locks_are_enforced() {
    let mut d = doc(Extent::new(4, 4), Depth::U8);
    let id = add(
        &mut d,
        None,
        Layer::pixel("p", Extent::new(4, 4), Depth::U8),
    );
    set_props(&mut d, id, |p| p.locks.pixels = true);
    let op = paint_op(
        d.state(),
        id,
        PaintTarget::Content,
        Rect::new(0, 0, 2, 2),
        |_, _, p| *p = [1.0; 4],
    )
    .unwrap();
    assert!(d.apply(op).is_err());
    set_props(&mut d, id, |p| {
        p.locks.pixels = false;
        p.locks.transparency = true;
    });
    let op = paint_op(
        d.state(),
        id,
        PaintTarget::Content,
        Rect::new(0, 0, 2, 2),
        |_, _, p| *p = [1.0; 4],
    )
    .unwrap();
    d.apply(op).unwrap();
    let l = d.state().find(id).unwrap();
    assert_eq!(l.raster().unwrap().pixel(0, 0), [1.0, 1.0, 1.0, 0.0]);
}
