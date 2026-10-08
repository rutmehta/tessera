//! ENG-8: the engine applies a raw's maker-note built-in lens correction
//! (Fujifilm RAF) in the default mode and in `None`, matching the
//! full-resolution reference exactly at L0 and the ENG-6 contract-order model
//! at L3. The resident fast path must not treat `None` as "no lens work" for
//! such raws.
mod common;
#[path = "common/preview.rs"]
mod preview;

use common::*;
use engine_api::id::ImageId;
use engine_api::recipe::{DevelopSettings, settings::LensProfileSource};
use image_core::{PixelRect, RawImage, RenderOutput, Renderer, RendererConfig};
use pipeline_cpu::RenderSource;
use test_fixtures::raw as raw_fixtures;

fn modes() -> Vec<(&'static str, DevelopSettings)> {
    let auto = DevelopSettings::default();
    let mut none = DevelopSettings::default();
    none.lens.profile = LensProfileSource::None;
    let mut ca_on = DevelopSettings::default();
    ca_on.lens.remove_chromatic_aberration = true;
    vec![("Auto", auto), ("None", none), ("Auto+RemoveCA", ca_on)]
}

fn raf(test: &str, id: u128) -> Option<RawImage> {
    let path = raw_fixtures::with_extension(test, "raf")?;
    let image = RawImage::open(ImageId(id), &path).unwrap();
    assert!(
        image.metadata().maker_lens.is_some(),
        "the RAF fixture carries a maker-note correction"
    );
    Some(image)
}

fn without_maker_note(image: &RawImage, id: u128) -> RawImage {
    let mut m = image.metadata().clone();
    m.maker_lens = None;
    image
        .with_metadata(ImageId(id), std::sync::Arc::new(m))
        .unwrap()
}

#[test]
fn raf_maker_note_level0_matches_pipeline_cpu_reference() {
    let Some(image) = raf("raf_maker_note_level0_matches_pipeline_cpu_reference", 801) else {
        return;
    };
    let e = image.level_extent(0);
    let mut failures = Vec::new();
    for (name, s) in modes() {
        let linear = Renderer::new(RendererConfig::default())
            .render_region_as(&image, &s, 0, PixelRect::full(e), RenderOutput::SceneLinear)
            .unwrap();
        let reference = pipeline_cpu::render_linear_scaled(
            &s,
            &RenderSource::Cfa {
                image: image.cfa(),
                metadata: image.metadata(),
            },
            1,
        )
        .unwrap();
        let diff = max_f32_diff(&assemble_f32(e, &linear), reference.planes());
        eprintln!("RAF {name}: L0 max linear diff {diff:e}");
        if diff != 0.0 {
            failures.push(format!("{name}: L0 scene-linear max diff {diff:e}"));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn raf_maker_note_level3_matches_contract_model() {
    let Some(image) = raf("raf_maker_note_level3_matches_contract_model", 802) else {
        return;
    };
    let plain = without_maker_note(&image, 803);
    let e = image.level_extent(3);
    let rect = PixelRect::full(e);
    let mut failures = Vec::new();
    for (name, s) in modes() {
        let source = RenderSource::Cfa {
            image: image.cfa(),
            metadata: image.metadata(),
        };
        let linear = Renderer::new(RendererConfig::default())
            .render_region_as(&image, &s, 3, rect, RenderOutput::SceneLinear)
            .unwrap();
        let reference = preview::linear(&source, &s, 8);
        let diff = max_f32_diff(&assemble_f32(e, &linear), reference.planes());
        let display = Renderer::new(RendererConfig::default())
            .render_region(&image, &s, 3, rect)
            .unwrap();
        let d = max_u8_diff(&assemble_u8(e, &display), &preview::display(&reference, &s));
        let uncorrected = Renderer::new(RendererConfig::default())
            .render_region_as(&plain, &s, 3, rect, RenderOutput::SceneLinear)
            .unwrap();
        let moved = max_f32_diff(&assemble_f32(e, &linear), &assemble_f32(e, &uncorrected));
        eprintln!(
            "RAF {name}: L3 max linear diff {diff:e}, display diff {d}, vs uncorrected {moved:e}"
        );
        if diff > 1e-5 || d != 0 {
            failures.push(format!(
                "{name}: scene-linear max diff {diff:e}, display max diff {d}"
            ));
        }
        if moved < 1e-3 {
            failures.push(format!("{name}: maker-note correction not applied"));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
