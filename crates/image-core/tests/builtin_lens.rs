//! ENG-7b: the engine applies a raw's built-in (DNG opcode) lens correction
//! with lens profile `None`, exactly like the full-resolution reference. The
//! resident fast path must not treat `None` as "no lens work" when the raw
//! carries opcode lists.
mod common;
#[path = "common/preview.rs"]
#[allow(dead_code)] // `display` is unused here
mod preview;

use std::sync::Arc;

use common::*;
use engine_api::id::ImageId;
use engine_api::recipe::{DevelopSettings, settings::LensProfileSource};
use image_core::{PixelRect, RawImage, RenderOutput, Renderer, RendererConfig};
use pipeline_cpu::RenderSource;
use raw_decode::CfaImage;

/// One DNG FixVignetteRadial opcode (OpcodeList3), as the DNG reader yields it.
fn vignette_opcode() -> Vec<u8> {
    let mut b = Vec::new();
    for x in [1_u32, 3, 0x01030000, 0, 56] {
        b.extend(x.to_be_bytes());
    }
    for x in [0.5_f64, 0., 0., 0., 0., 0.5, 0.5] {
        b.extend(x.to_be_bytes());
    }
    b
}

fn image(id: u128, opcodes: bool) -> RawImage {
    let (w, h) = (257, 190);
    let mut m = metadata(w, h, RGGB, [0, 0, w, h]);
    if opcodes {
        m.has_opcode_list = true;
        m.opcode_lists = [None, None, Some(vignette_opcode())];
    }
    RawImage::new(
        ImageId(id),
        Arc::new(CfaImage::from_linear(w, h, samples(w, h, RGGB)).unwrap()),
        Arc::new(m),
    )
    .unwrap()
}

fn linear(raw: &RawImage, s: &DevelopSettings) -> Vec<f32> {
    let e = raw.level_extent(0);
    let tiles = Renderer::new(RendererConfig::default())
        .render_region_as(raw, s, 0, PixelRect::full(e), RenderOutput::SceneLinear)
        .unwrap();
    assemble_f32(e, &tiles).concat()
}

#[test]
fn profile_none_still_applies_built_in_opcodes() {
    let mut s = DevelopSettings::default();
    s.lens.profile = LensProfileSource::None;
    let with = image(2301, true);
    let without = image(2302, false);
    let reference = pipeline_cpu::render_linear_scaled(
        &s,
        &pipeline_cpu::RenderSource::Cfa {
            image: with.cfa(),
            metadata: with.metadata(),
        },
        1,
    )
    .unwrap();
    let engine = linear(&with, &s);
    let reference = reference.planes().concat();
    assert_eq!(engine.len(), reference.len());
    let diff = engine
        .iter()
        .zip(&reference)
        .map(|(a, b)| (a - b).abs())
        .fold(0f32, f32::max);
    assert_eq!(diff, 0.0, "engine differs from the reference by {diff:e}");
    assert_ne!(
        engine,
        linear(&without, &s),
        "built-in correction must be applied with profile None"
    );
}

fn level(raw: &RawImage, s: &DevelopSettings, l: u8) -> Vec<f32> {
    let e = raw.level_extent(l);
    let tiles = Renderer::new(RendererConfig::default())
        .render_region_as(raw, s, l, PixelRect::full(e), RenderOutput::SceneLinear)
        .unwrap();
    assemble_f32(e, &tiles).concat()
}

/// ENG-7c (REV2 S-3): the L3 preview path of an opcode raw in profile None
/// matches the contract-order L3 model (ENG-6, `common/preview.rs`) built from
/// pipeline-cpu operators, equals the explicit Embedded mode, and differs from
/// the same raw without opcodes.
#[test]
fn profile_none_level3_preview_matches_reference_with_built_in_opcodes() {
    let mut s = DevelopSettings::default();
    s.lens.profile = LensProfileSource::None;
    let with = image(2303, true);
    let without = image(2304, false);
    let engine = level(&with, &s, 3);
    let source = RenderSource::Cfa {
        image: with.cfa(),
        metadata: with.metadata(),
    };
    let reference = preview::linear(&source, &s, 8).planes().concat();
    assert_eq!(engine.len(), reference.len());
    let diff = engine
        .iter()
        .zip(&reference)
        .map(|(a, b)| (a - b).abs())
        .fold(0f32, f32::max);
    assert!(
        diff <= 1e-5,
        "L3 engine differs from the contract-order model by {diff:e}"
    );
    let mut embedded = s.clone();
    embedded.lens.profile = LensProfileSource::Embedded;
    assert_eq!(engine, level(&with, &embedded, 3));
    assert_ne!(engine, level(&without, &s, 3));
}
