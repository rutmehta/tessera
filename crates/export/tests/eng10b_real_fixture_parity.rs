//! ENG-10b (REV-ENG-10 SF1): full-size (scale 1) Adobe-process print of the
//! real Canon CR3 and Fuji X-Trans RAF fixtures equals Develop's level-0
//! frame put through the same output transform, bit for bit.
//!
//! ENG-9 exported Adobe recipes through `pipeline_adobe`, which on these two
//! cameras was not within 1e-4 of Develop's renderer (the reviewer measured
//! up to 18 codes on the CR3 and 87 codes on the RAF in 8-bit levels).
//! ENG-10 draws exports with Develop's renderer itself, so they now match
//! Develop; this test keeps them matched on real sensor data. The comparison
//! is after the export's own transform on both sides (as in
//! `eng10_level_parity.rs`); ENG-9's 8-bit parity covers the transform.
//!
//! Without `fixtures/raw` the test prints `SKIPPED` (or fails when
//! `TESSERA_REQUIRE_RAW_FIXTURES` is set).
#[path = "../../image-core/tests/common/raw_fixtures.rs"]
mod raw_fixtures;

use engine_api::{
    id::ImageId,
    jobs::CancellationToken,
    recipe::{EditMeta, ProcessVersion, Recipe},
    tile::Pyramid as _,
};
use export::{ColorSpace, ExportImage, RenderRequest, Resize, SharpenFor};
use image_core::{PixelRect, RawImage, RenderOutput, Renderer, RendererConfig};
use std::sync::Arc;

/// `auto_calibrated`: lens distortion estimated from image content (an
/// explicit choice since ENG-7; it was the default `Auto` fallback when the
/// reviewer measured ENG-9's differences on these cameras).
fn recipe(auto_calibrated: bool) -> Recipe {
    let mut recipe = Recipe {
        process_version: ProcessVersion::adobe(6),
        ..Default::default()
    };
    recipe
        .edit(EditMeta::user("eng10b", 1), |s| {
            s.tone.exposure = 0.3;
            s.tone.contrast = 20.;
            s.tone.highlights = -40.;
            s.tone.shadows = 30.;
            s.color.vibrance = 15.;
            s.color.saturation = 5.;
            if auto_calibrated {
                s.lens.profile = engine_api::recipe::settings::LensProfileSource::AutoCalibrated;
            }
        })
        .unwrap();
    recipe
}

#[test]
fn eng10b_cr3_and_xtrans_full_size_print_matches_develop() {
    const TEST: &str = "eng10b_cr3_and_xtrans_full_size_print_matches_develop";
    let files: Vec<_> = raw_fixtures::all(TEST)
        .into_iter()
        .filter(|p| {
            p.extension().is_some_and(|e| {
                ["cr3", "raf"].contains(&e.to_string_lossy().to_ascii_lowercase().as_str())
            })
        })
        .collect();
    if files.is_empty() {
        raw_fixtures::skipped(TEST, "no CR3 or RAF fixture");
        return;
    }
    for path in files {
        for auto_calibrated in [false, true] {
            check(&path, &recipe(auto_calibrated), auto_calibrated);
        }
    }
}

fn check(path: &std::path::Path, recipe: &Recipe, auto_calibrated: bool) {
    const TEST: &str = "eng10b_cr3_and_xtrans_full_size_print_matches_develop";
    {
        let mut raw = raw_decode::RawSource::open(path).unwrap();
        let cfa = raw.decode_cfa().unwrap();
        // Compare unoriented frames (print orients after rendering).
        let mut metadata = raw.metadata();
        metadata.orientation = 1;
        metadata.catalog_orientation = None;
        let image = RawImage::new(
            ImageId(10_100),
            Arc::new(
                raw_decode::CfaImage::from_linear(
                    cfa.pyramid().extent().width,
                    cfa.pyramid().extent().height,
                    cfa.pyramid().pixels().to_vec(),
                )
                .unwrap(),
            ),
            Arc::new(metadata.clone()),
        )
        .unwrap();
        let renderer = Renderer::new(RendererConfig {
            process_version: recipe.process_version,
            ..Default::default()
        });
        let extent = Renderer::output_extent(&image, &recipe.settings, 0).unwrap();
        let tiles = renderer
            .render_region_as(
                &image,
                &recipe.settings,
                0,
                PixelRect::full(extent),
                RenderOutput::SceneLinear,
            )
            .unwrap();
        let mut linear = image::Rgb32FImage::new(extent.width, extent.height);
        for t in &tiles {
            let l = t.layout();
            let n = l.plane_len();
            let (ox, oy) = t.coord().pixel_origin(engine_api::tile::TILE_SIZE);
            let values = t.samples::<f32>().unwrap();
            for y in 0..l.extent.height {
                for x in 0..l.extent.width {
                    let i = (y * l.extent.width + x) as usize;
                    linear.put_pixel(
                        ox + x,
                        oy + y,
                        image::Rgb(std::array::from_fn(|c| values[c * n + i])),
                    );
                }
            }
        }
        let mut registry = color_mgmt::Registry::new();
        let target = registry.builtin(color_mgmt::Builtin::Srgb).unwrap();
        let expected = pipeline_cpu::output_managed_pixels(
            &recipe.settings,
            linear,
            &mut pipeline_cpu::OutputContext {
                registry: &mut registry,
                target: pipeline_cpu::OutputTarget::Export(&target),
                proof: None,
                options: color_mgmt::TransformOptions::default(),
            },
        )
        .unwrap();
        let (printed, _) = export::render_pixels_with_notes(
            &ExportImage {
                source: pipeline_cpu::RenderSource::Cfa {
                    image: &cfa,
                    metadata: &metadata,
                },
                name: "eng10b",
                sequence: 1,
                date: "",
                metadata: None,
            },
            recipe,
            &RenderRequest {
                color_space: ColorSpace::Srgb,
                resize: Resize::None,
                sharpen_for: SharpenFor::None,
                scale: 1,
            },
            &CancellationToken::new(),
            None,
            None,
            None,
        )
        .unwrap();
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        assert_eq!(printed.dimensions(), expected.dimensions(), "{name}");
        let differing = printed
            .as_raw()
            .iter()
            .zip(expected.as_raw())
            .filter(|(a, b)| a.to_bits() != b.to_bits())
            .count();
        raw_fixtures::notice(
            TEST,
            &format!(
                "{name} {}x{} (lens auto-calibration {auto_calibrated}): {differing} samples \
                 differ from Develop",
                extent.width, extent.height
            ),
        );
        assert_eq!(
            differing, 0,
            "{name} (auto-calibration {auto_calibrated}): print differs from Develop's frame"
        );
    }
}
