mod common;
use common::*;
use compositor::render::styles::{
    Bevel, GlobalLight, Glow, Overlay, Satin, Shadow, Stroke, StyleEffect,
};
use compositor::*;
use engine_api::tile::{Extent, TileCoord};

#[test]
fn styled_live_edit_reuses_distant_tiles_and_updates_shadow_across_seam() {
    let mut d = doc(Extent::new(768, 64), Depth::F32);
    let model = vector::ShapeModel {
        path: vector::Path::polyline(
            &[
                vector::Point::new(230., 20.),
                vector::Point::new(245., 20.),
                vector::Point::new(245., 40.),
                vector::Point::new(230., 40.),
            ],
            true,
        ),
        fill: Some(vector::Fill::Solid([0.8, 0.2, 0.1, 1.])),
        ..Default::default()
    };
    let mut layer = Layer::new(
        "styled shape",
        LayerKind::Shape {
            model: model.clone(),
            transform: Affine::IDENTITY,
        },
    );
    layer
        .props
        .styles
        .effects
        .push(StyleEffect::DropShadow(Shadow {
            distance: 30.,
            size: 3.,
            angle: 180.,
            use_global_light: false,
            ..Default::default()
        }));
    let id = add(&mut d, None, layer);
    let c = Compositor::new(32 << 20);
    let distant = TileCoord::new(0, 2, 0);
    c.render_tile(&d, distant).unwrap();
    let before = c.render_tile(&d, TileCoord::new(0, 1, 0)).unwrap();
    d.apply(DocOp::EditShape {
        id,
        model,
        transform: Affine::scale_translate(1., 1., -10., 0.),
    })
    .unwrap();
    let stats = c.stats();
    c.render_tile(&d, distant).unwrap();
    assert_eq!(
        c.stats().root_full,
        stats.root_full,
        "distant tile must be reused"
    );
    let after = c.render_tile(&d, TileCoord::new(0, 1, 0)).unwrap();
    assert_ne!(
        before.samples::<f32>().unwrap(),
        after.samples::<f32>().unwrap()
    );
    let cold = Compositor::new(0);
    assert_eq!(
        c.render_level_rgba(&d, 0).unwrap(),
        cold.render_level_rgba(&d, 0).unwrap()
    );
    assert!(d.undo());
    assert_eq!(
        c.render_level_rgba(&d, 0).unwrap(),
        cold.render_level_rgba(&d, 0).unwrap()
    );
}

#[test]
fn styled_text_nested_effects_and_light_changes_match_cold_frames() {
    let effects = [
        StyleEffect::DropShadow(Shadow::default()),
        StyleEffect::InnerShadow(Shadow::default()),
        StyleEffect::OuterGlow(Glow::default()),
        StyleEffect::InnerGlow(Glow::default()),
        StyleEffect::Bevel(Bevel::default()),
        StyleEffect::Satin(Satin::default()),
        StyleEffect::Stroke(Stroke::default()),
        StyleEffect::ColorOverlay(Overlay::default()),
    ];
    let fonts = || {
        let mut fonts = typography::TextRenderer::new();
        fonts.fonts_mut().load_font_data(
            include_bytes!("../../typography/tests/fonts/NotoSans-Regular.ttf").to_vec(),
        );
        fonts
    };
    for effect in effects {
        let mut d = doc(Extent::new(800, 64), Depth::F32);
        let mut group = Layer::group("styled ancestor", GroupMode::Isolated);
        group
            .props
            .styles
            .effects
            .push(StyleEffect::DropShadow(Shadow {
                distance: 22.5,
                size: 2.5,
                ..Default::default()
            }));
        let parent = add(&mut d, None, group);
        let mut model = typography::TextModel::point("i", "Noto Sans", 25.);
        let transform = Affine::scale_translate(1., 1., 240., 4.);
        let mut text = Layer::new(
            "text",
            LayerKind::Text {
                model: model.clone(),
                transform,
            },
        );
        text.props.styles.scale = 1.3;
        text.props.styles.effects.push(effect.clone());
        let id = add(&mut d, Some(parent), text);
        let comp = Compositor::new(64 << 20);
        comp.set_text_renderer(fonts());
        let cold = Compositor::new(0);
        cold.set_text_renderer(fonts());
        for level in [0, 1] {
            comp.render_level_rgba(&d, level).unwrap();
        }
        for step in 0..4 {
            match step {
                0 => {
                    model.runs[0].text.push('W');
                    d.apply(DocOp::EditText {
                        id,
                        model: model.clone(),
                        transform,
                    })
                    .unwrap();
                }
                1 => {
                    d.apply(DocOp::SetGlobalLight(GlobalLight {
                        angle: 180.,
                        elevation: 45.,
                    }))
                    .unwrap();
                }
                2 => {
                    d.apply(DocOp::SetMask {
                        id,
                        mask: Some(Mask::hide_all(d.state().canvas, Depth::F32)),
                    })
                    .unwrap();
                }
                _ => {
                    assert!(d.undo());
                }
            }
            for level in [0, 1] {
                assert_eq!(
                    comp.render_level_rgba(&d, level).unwrap(),
                    cold.render_level_rgba(&d, level).unwrap(),
                    "{effect:?} step={step} level={level}",
                );
            }
        }
    }
}
