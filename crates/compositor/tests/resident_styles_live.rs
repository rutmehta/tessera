//! M5-30/M5-31 integration: styles must preserve live fonts and F32 geometry.
mod common;

use common::*;
use compositor::{gpu::GpuCompositor, render::styles::*, resident::ResidentRenderer, *};
use engine_api::tile::Extent;
use std::sync::Arc;

fn fonts() -> typography::TextRenderer {
    let mut fonts = typography::TextRenderer::new();
    fonts.fonts_mut().load_font_data(
        include_bytes!("../../typography/tests/fonts/NotoSans-Regular.ttf").to_vec(),
    );
    fonts.fonts_mut().set_sans_serif_family("Noto Sans");
    fonts
}

#[derive(Clone, Copy, Debug)]
enum Placement {
    Direct,
    IsolatedGroup,
    SmartChild,
    StyledSmart,
    FilteredSmart,
    CpuFilteredSmart,
    DisabledSmart,
}

fn text_document(placement: Placement, family: &str) -> Document {
    let extent = Extent::new(96, 64);
    let mut text = Layer::new(
        "live text",
        LayerKind::Text {
            model: typography::TextModel::point("office", family, 22.0),
            transform: Affine::scale_translate(1.0, 1.0, 5.25, 7.5),
        },
    );
    let effect = StyleEffect::OuterGlow(Glow {
        size: 2.5,
        ..Default::default()
    });
    let mut state = DocState::new(extent, Depth::F32);
    match placement {
        Placement::Direct
        | Placement::SmartChild
        | Placement::FilteredSmart
        | Placement::CpuFilteredSmart
        | Placement::DisabledSmart => {
            text.props.styles.effects.push(effect);
            state.root.push(Arc::new(text));
        }
        Placement::StyledSmart => state.root.push(Arc::new(text)),
        Placement::IsolatedGroup => {
            let mut group = Layer::group("styled group", GroupMode::Isolated);
            group.props.styles.effects.push(effect);
            let LayerKind::Group { children, .. } = &mut group.kind else {
                unreachable!()
            };
            children.push(Arc::new(text));
            state.root.push(Arc::new(group));
        }
    }
    if matches!(
        placement,
        Placement::SmartChild
            | Placement::StyledSmart
            | Placement::FilteredSmart
            | Placement::CpuFilteredSmart
            | Placement::DisabledSmart
    ) {
        let mut child = SmartObject::new(state, Affine::IDENTITY);
        if matches!(
            placement,
            Placement::FilteredSmart | Placement::CpuFilteredSmart | Placement::DisabledSmart
        ) {
            child.filters.push(SmartFilter {
                name: if matches!(placement, Placement::CpuFilteredSmart) {
                    "gaussian"
                } else {
                    "invert"
                }
                .into(),
                enabled: !matches!(placement, Placement::DisabledSmart),
                params: serde_json::json!({"radius": 1.0}),
                ..Default::default()
            });
        }
        state = DocState::new(extent, Depth::F32);
        let mut smart = Layer::new("smart text", LayerKind::SmartObject(child));
        if matches!(placement, Placement::StyledSmart) {
            smart
                .props
                .styles
                .effects
                .push(StyleEffect::OuterGlow(Glow {
                    size: 2.5,
                    ..Default::default()
                }));
        }
        state.root.push(Arc::new(smart));
    }
    Document::new(state)
}

#[track_caller]
fn parity(cpu: &Compositor, resident: &mut ResidentRenderer, doc: &Document, level: u8) {
    resident
        .render(doc, level)
        .expect("styled live source must render");
    let tiles = resident.read_tiles(level).unwrap();
    assert!(!tiles.is_empty());
    let mut nonempty = false;
    for tile in tiles {
        let expected = cpu.render_tile_premultiplied(doc, tile.coord()).unwrap();
        let wanted = expected.samples::<f32>().unwrap();
        nonempty |= wanted.iter().any(|v| *v > 0.0);
        let actual = tile.samples::<f32>().unwrap();
        assert_eq!(actual.len(), wanted.len());
        let (index, error) = actual
            .iter()
            .zip(wanted)
            .enumerate()
            .map(|(i, (a, b))| (i, (a - b).abs()))
            .max_by(|a, b| a.1.total_cmp(&b.1))
            .unwrap();
        assert!(
            error <= 1e-4,
            "L{level} sample {index}: GPU {} CPU {}, error {error}",
            actual[index],
            wanted[index]
        );
    }
    assert!(nonempty, "fixture must contain visible text/shape pixels");
}

#[test]
fn styled_text_uses_explicit_fonts_in_direct_group_and_smart_sources() {
    let gpu = GpuCompositor::new().expect("live styles regression requires a GPU");
    for placement in [
        Placement::Direct,
        Placement::IsolatedGroup,
        Placement::SmartChild,
        Placement::StyledSmart,
        Placement::FilteredSmart,
        Placement::CpuFilteredSmart,
        Placement::DisabledSmart,
    ] {
        eprintln!("placement {placement:?}");
        let doc = text_document(placement, "Noto Sans");
        let cpu = Compositor::new(8 << 20);
        cpu.set_text_renderer(fonts());
        let mut resident = ResidentRenderer::with_budget(&gpu, 8 << 20).unwrap();
        resident.set_text_renderer(fonts());
        for level in [0, 1, 2] {
            parity(&cpu, &mut resident, &doc, level);
        }
        resident.wait_for_specializations();
        resident.invalidate();
        parity(&cpu, &mut resident, &doc, 0);
    }
}

#[test]
fn styled_text_does_not_discover_fonts_when_explicit_database_is_empty() {
    let gpu = GpuCompositor::new().expect("live styles regression requires a GPU");
    for placement in [
        Placement::Direct,
        Placement::IsolatedGroup,
        Placement::SmartChild,
        Placement::StyledSmart,
        Placement::FilteredSmart,
        Placement::CpuFilteredSmart,
        Placement::DisabledSmart,
    ] {
        eprintln!("placement {placement:?}");
        let doc = text_document(placement, "sans-serif");
        let cpu = Compositor::new(8 << 20);
        cpu.set_text_renderer(typography::TextRenderer::new());
        assert!(cpu.render_level_rgba(&doc, 0).is_err());
        let mut resident = ResidentRenderer::with_budget(&gpu, 8 << 20).unwrap();
        resident.set_text_renderer(typography::TextRenderer::new());
        assert!(
            resident.render(&doc, 0).is_err(),
            "{placement:?}: explicit empty fonts must not fall back to system discovery"
        );
    }
}

#[test]
fn replacing_fonts_invalidates_styled_text_and_nested_caches() {
    let gpu = GpuCompositor::new().expect("live styles regression requires a GPU");
    for placement in [
        Placement::Direct,
        Placement::IsolatedGroup,
        Placement::SmartChild,
        Placement::StyledSmart,
        Placement::FilteredSmart,
        Placement::CpuFilteredSmart,
        Placement::DisabledSmart,
    ] {
        eprintln!("placement {placement:?}");
        let doc = text_document(placement, "sans-serif");
        let mut resident = ResidentRenderer::with_budget(&gpu, 8 << 20).unwrap();
        resident.set_text_renderer(fonts());
        resident.render(&doc, 0).unwrap();
        resident.wait().unwrap();
        resident.set_text_renderer(typography::TextRenderer::new());
        assert!(
            resident.render(&doc, 0).is_err(),
            "{placement:?}: font replacement must discard cached style/smart sources"
        );
        resident.set_text_renderer(fonts());
        let cpu = Compositor::new(8 << 20);
        cpu.set_text_renderer(fonts());
        for level in [0, 1, 2] {
            parity(&cpu, &mut resident, &doc, level);
        }
    }
}

#[test]
fn styled_live_shapes_keep_float_source_precision_in_integer_documents() {
    let gpu = GpuCompositor::new().expect("live styles regression requires a GPU");
    for depth in [Depth::U8, Depth::U16, Depth::F32] {
        let mut doc = doc(Extent::new(48, 40), depth);
        let mut shape = Layer::new(
            "fractional shape",
            LayerKind::Shape {
                model: vector::ShapeModel {
                    path: vector::Shape::Rectangle {
                        rect: vector::Rect::new(0.25, 0.375, 23.6, 19.3),
                        radii: [0.0; 4],
                    }
                    .path()
                    .unwrap(),
                    fill: Some(vector::Fill::Solid([0.713, 0.237, 0.419, 0.637])),
                    ..Default::default()
                },
                transform: Affine::scale_translate(1.0, 1.0, 5.125, 4.25),
            },
        );
        shape
            .props
            .styles
            .effects
            .push(StyleEffect::OuterGlow(Glow {
                size: 1.7,
                opacity: 0.45,
                ..Default::default()
            }));
        add(&mut doc, None, shape);
        let cpu = Compositor::new(8 << 20);
        let mut resident = ResidentRenderer::with_budget(&gpu, 8 << 20).unwrap();
        resident.set_specialization(false);
        for level in [0, 1, 2] {
            parity(&cpu, &mut resident, &doc, level);
        }
        resident.set_specialization(true);
        resident.render(&doc, 0).unwrap();
        resident.wait_for_specializations();
        resident.invalidate();
        parity(&cpu, &mut resident, &doc, 0);
    }
}

#[test]
fn styled_vector_masks_keep_float_coverage_and_native_raster_mips() {
    let gpu = GpuCompositor::new().unwrap();
    for depth in [Depth::U8, Depth::U16, Depth::F32] {
        let extent = Extent::new(273, 39);
        let mut doc = doc(extent, depth);
        let mut pixel = layer_fn("native raster", extent, depth, |x, y| {
            [0.713, 0.237, 0.419, ((x * 7 + y * 13) % 17) as f32 / 16.0]
        });
        pixel.vector_mask = Some(VectorMask {
            path: vector::Shape::Rectangle {
                rect: vector::Rect::new(5.375, 4.125, 265.6, 27.25),
                radii: [0.; 4],
            }
            .path()
            .unwrap(),
            enabled: true,
            feather: 1.3,
            density: 0.73,
        });
        let mut mask = Mask::reveal_all(extent, depth);
        mask.raster
            .edit_region(Rect::of_extent(extent), 1, |x, y, p| {
                p[0] = ((x * 3 + y * 11) % 19) as f32 / 18.0;
            })
            .unwrap();
        pixel.mask = Some(mask);
        let mut group = Layer::group("styled masked raster", GroupMode::Isolated);
        group
            .props
            .styles
            .effects
            .push(StyleEffect::OuterGlow(Glow {
                size: 1.7,
                opacity: 0.45,
                ..Default::default()
            }));
        let LayerKind::Group { children, .. } = &mut group.kind else {
            unreachable!()
        };
        children.push(Arc::new(pixel));
        add(&mut doc, None, group);
        let cpu = Compositor::new(8 << 20);
        let mut resident = ResidentRenderer::with_budget(&gpu, 8 << 20).unwrap();
        resident.set_specialization(false);
        for level in [0, 1, 2] {
            parity(&cpu, &mut resident, &doc, level);
        }
        resident.set_specialization(true);
        resident.render(&doc, 0).unwrap();
        resident.wait_for_specializations();
        resident.invalidate();
        parity(&cpu, &mut resident, &doc, 0);
    }
}
