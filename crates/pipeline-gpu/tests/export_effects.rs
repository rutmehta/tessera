//! ENG-14: export transactions never build the effects constants map
//! (vignette mask and grain value per pixel of the whole frame, 8 B/px,
//! outside every band's scratch). They use the inline WGSL path, which
//! produces bit-identical pixels: the test-only switch
//! `ManagedRenderer::with_export_effects_map` forces the map for comparison.

use color_mgmt::{Builtin, Registry, TransformOptions};
use engine_api::{
    jobs::CancellationToken,
    recipe::DevelopSettings,
    tile::{Extent, Tile},
};
use image_core::PixelRect;
use pipeline_cpu::{OutputContext, OutputTarget};
use pipeline_gpu::{ExportResize, GpuContext, GpuManagedOutput, ManagedRenderer};
use std::sync::Arc;

#[path = "../../image-core/tests/common/mod.rs"]
mod common;

const WIDTH: u32 = 1200;
const HEIGHT: u32 = 700;

fn variants() -> Vec<(&'static str, DevelopSettings)> {
    let mut base = DevelopSettings::default();
    base.lens.profile = engine_api::recipe::settings::LensProfileSource::None;
    base.lens.remove_chromatic_aberration = false;
    let with = |edit: fn(&mut DevelopSettings)| {
        let mut s = base.clone();
        edit(&mut s);
        s
    };
    vec![
        ("vignette", with(|s| s.effects.vignette.amount = -40.)),
        ("grain", with(|s| s.effects.grain.amount = 40.)),
        (
            "vignette+grain",
            with(|s| {
                s.effects.vignette.amount = 30.;
                s.effects.vignette.feather = 70.;
                s.effects.grain.amount = 25.;
                s.tone.exposure = 0.4;
            }),
        ),
    ]
}

fn output(settings: &DevelopSettings) -> Arc<GpuManagedOutput> {
    let mut registry = Registry::new();
    let target = registry.builtin(Builtin::Srgb).unwrap();
    Arc::new(
        GpuManagedOutput::new(
            Arc::new(GpuContext::new().unwrap()),
            settings,
            &mut OutputContext {
                registry: &mut registry,
                target: OutputTarget::Export(&target),
                proof: None,
                options: TransformOptions::default(),
            },
        )
        .unwrap(),
    )
}

fn config() -> image_core::RendererConfig {
    image_core::RendererConfig {
        cache_budget_bytes: 0,
        ..Default::default()
    }
}

/// One worker's bands (recycling, as export runs them) covering the output:
/// the pixels and the effects maps each band built.
fn bands(
    base: &ManagedRenderer,
    image: &image_core::RawImage,
    settings: &DevelopSettings,
    destination: Option<Extent>,
) -> (Vec<f32>, Vec<u64>) {
    let frame = image.active_extent();
    let out = destination.unwrap_or(frame);
    let worker = base.export_band(None);
    let (mut pixels, mut maps) = (Vec::new(), Vec::new());
    let mut top = 0;
    for rows in [96u32, 160, 32, 200, 64].into_iter().cycle() {
        if top >= out.height {
            break;
        }
        let rows = rows.min(out.height - top);
        let resize = destination.map(|destination| ExportResize {
            source: frame,
            destination,
            top,
            rows,
        });
        let source = match resize {
            Some(r) => {
                let rect = r.support_rect().unwrap();
                rect.y..rect.y + rect.height
            }
            None => top..top + rows,
        };
        let renderer = worker.export_band_recycling(resize);
        let mut dst = vec![0f32; (rows * out.width * 3) as usize];
        assert!(
            renderer
                .render_export_rows(
                    image,
                    settings,
                    0,
                    source,
                    None,
                    &mut dst,
                    &CancellationToken::new(),
                )
                .unwrap(),
            "band renderer declined rows {top}+{rows}"
        );
        pixels.extend(dst);
        maps.push(renderer.stats().effects_maps);
        top += rows;
    }
    (pixels, maps)
}

fn bits(samples: &[f32]) -> Vec<u32> {
    samples.iter().map(|v| v.to_bits()).collect()
}

#[test]
fn export_bands_use_the_inline_effects_path_bit_identical_to_the_map() {
    let image = common::synthetic(1401, WIDTH, HEIGHT, common::RGGB, [3, 2, 1190, 690]);
    for (name, settings) in variants() {
        let output = output(&settings);
        let base = ManagedRenderer::new_export_budgeted(output, config(), None, 1 << 30);
        for destination in [None, Some(Extent::new(800, 466))] {
            // A fresh switch per run: the map is cached per parameter key.
            let forced = base.with_export_effects_map();
            let (inline, inline_maps) = bands(&base, &image, &settings, destination);
            let (mapped, mapped_maps) = bands(&forced, &image, &settings, destination);
            assert!(
                inline_maps.iter().all(|&m| m == 0),
                "{name} {destination:?}: export bands built effects maps {inline_maps:?}"
            );
            assert!(
                mapped_maps.iter().sum::<u64>() > 0,
                "{name} {destination:?}: the forced map was never built"
            );
            assert_eq!(inline.len(), mapped.len());
            let differing = bits(&inline)
                .iter()
                .zip(bits(&mapped))
                .filter(|(a, b)| **a != *b)
                .count();
            assert_eq!(
                differing, 0,
                "{name} {destination:?}: inline and mapped bands differ"
            );
        }
    }
}

/// The export tile path (`render_export`) is an export transaction too.
#[test]
fn export_tiles_use_the_inline_effects_path_bit_identical_to_the_map() {
    let image = common::synthetic(1402, 700, 520, common::RGGB, [0, 0, 700, 520]);
    for (name, settings) in variants() {
        let output = output(&settings);
        let base = ManagedRenderer::new_export_budgeted(output, config(), None, 1 << 30);
        let forced = base.with_export_effects_map();
        let render = |renderer: &ManagedRenderer| -> (Vec<Tile>, u64) {
            let renderer = renderer.export_band(None);
            let tiles = renderer
                .render_export(
                    &image,
                    &settings,
                    0,
                    PixelRect::full(image.active_extent()),
                    &CancellationToken::new(),
                )
                .unwrap()
                .expect("resident export");
            (tiles, renderer.stats().effects_maps)
        };
        let (inline, inline_maps) = render(&base);
        let (mapped, mapped_maps) = render(&forced);
        assert_eq!(inline_maps, 0, "{name}: the export built an effects map");
        assert!(mapped_maps > 0, "{name}: the forced map was never built");
        assert_eq!(inline.len(), mapped.len());
        for (a, b) in inline.iter().zip(&mapped) {
            assert_eq!(
                bits(a.samples::<f32>().unwrap()),
                bits(b.samples::<f32>().unwrap()),
                "{name}: tile {:?}",
                a.coord()
            );
        }
    }
}
