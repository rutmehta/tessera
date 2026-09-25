//! Whole-level resident rendering (M2-17b): one level-sized tile per stage.
//! Gates: bit-identical to the per-tile resident path, CPU parity for
//! Detail and effects-map rendering, determinism, and whole-level dispatch
//! counts.
#[path = "../../image-core/tests/common/mod.rs"]
mod common;

use engine_api::{
    recipe::DevelopSettings,
    tile::{TILE_SIZE, Tile},
};
use image_core::{PixelRect, RenderOutput, Renderer, RendererConfig, TileCache};
use pipeline_gpu::{GpuContext, GpuStageOp};
use std::sync::Arc;

fn gpu() -> Arc<GpuStageOp> {
    Arc::new(GpuStageOp::new(Arc::new(GpuContext::new().unwrap())))
}

fn renderer(gpu: &Arc<GpuStageOp>) -> Renderer {
    let config = RendererConfig::default();
    Renderer::with_ops(
        gpu.clone(),
        Arc::new(TileCache::new(config.cache_budget_bytes)),
        config,
    )
}

fn heavy() -> DevelopSettings {
    let mut s = DevelopSettings::default();
    s.tone.exposure = 0.25;
    s.tone.texture = 35.0;
    s.tone.clarity = -25.0;
    s.tone.dehaze = 20.0;
    s.tone.curves.parametric.lights = 15.0;
    s.detail.sharpening.amount = 70.0;
    s.detail.sharpening.masking = 30.0;
    s.detail.noise_reduction.luminance = 30.0;
    s.detail.noise_reduction.color = 40.0;
    s.detail.noise_reduction.color_smoothness = 80.0;
    s.color.vibrance = 20.0;
    s.effects.vignette.amount = -30.0;
    s.effects.grain.amount = 25.0;
    s
}

fn f32_diff(a: &[Tile], b: &[Tile]) -> f32 {
    assert_eq!(a.len(), b.len());
    a.iter()
        .zip(b)
        .flat_map(|(a, b)| {
            assert_eq!(a.coord(), b.coord());
            assert_eq!(a.layout(), b.layout());
            a.samples::<f32>()
                .unwrap()
                .iter()
                .zip(b.samples::<f32>().unwrap())
                .map(|(a, b)| (a - b).abs())
                .collect::<Vec<_>>()
        })
        .fold(0.0, f32::max)
}

fn u8_diff(a: &[Tile], b: &[Tile]) -> u8 {
    assert_eq!(a.len(), b.len());
    a.iter()
        .zip(b)
        .map(|(a, b)| common::max_u8_diff(a.samples::<u8>().unwrap(), b.samples::<u8>().unwrap()))
        .max()
        .unwrap_or(0)
}

#[test]
fn whole_level_matches_per_tile_resident_path_bitwise() {
    let gpu = gpu();
    let r = renderer(&gpu);
    let image = common::synthetic(5101, 1400, 1100, common::RGGB, [4, 6, 1390, 1090]);
    let s = heavy();
    for level in [0u8, 1] {
        let e = image.level_extent(level);
        for output in [RenderOutput::SceneLinear, RenderOutput::Display] {
            // Whole level: one level-sized tile (single fused dispatch).
            let before = gpu.stats().fused_dispatches;
            let whole = r
                .render_region_as(&image, &s, level, PixelRect::full(e), output)
                .unwrap();
            assert_eq!(gpu.stats().fused_dispatches - before, 1);
            // A region: per-tile resident path (one fused dispatch per tile).
            let rect = PixelRect::new(TILE_SIZE, 0, e.width - TILE_SIZE, TILE_SIZE + 3);
            let before = gpu.stats().fused_dispatches;
            let part = r.render_region_as(&image, &s, level, rect, output).unwrap();
            assert_eq!(
                gpu.stats().fused_dispatches - before,
                part.len() as u64,
                "per-tile path"
            );
            for t in &part {
                let w = whole.iter().find(|w| w.coord() == t.coord()).unwrap();
                assert_eq!(w.layout(), t.layout());
                if output == RenderOutput::Display {
                    assert_eq!(w.samples::<u8>().unwrap(), t.samples::<u8>().unwrap());
                } else {
                    assert_eq!(w.samples::<f32>().unwrap(), t.samples::<f32>().unwrap());
                }
            }
            // Deterministic, warm caches included.
            let again = r
                .render_region_as(&image, &s, level, PixelRect::full(e), output)
                .unwrap();
            assert_eq!(f32_or_u8(&whole, &again, output), 0.0, "L{level} repeat");
        }
    }
}

fn f32_or_u8(a: &[Tile], b: &[Tile], output: RenderOutput) -> f32 {
    match output {
        RenderOutput::Display => f32::from(u8_diff(a, b)),
        RenderOutput::SceneLinear => f32_diff(a, b),
    }
}

/// Resident Detail (sharpening with masking, luminance and chroma NR) and
/// the GPU-resident vignette/grain map against the CPU reference.
#[test]
fn whole_level_detail_and_effects_match_cpu() {
    let gpu = gpu();
    let r = renderer(&gpu);
    let cpu = Renderer::new(RendererConfig::default());
    for (id, cfa) in [(5102, common::RGGB), (5103, common::xtrans())] {
        let image = common::synthetic(id, 700, 520, cfa, [2, 4, 690, 510]);
        let mut s = heavy();
        s.tone.texture = 0.0;
        s.tone.clarity = 0.0;
        s.tone.dehaze = 0.0;
        for level in [0u8, 2] {
            let rect = PixelRect::full(image.level_extent(level));
            for output in [RenderOutput::SceneLinear, RenderOutput::Display] {
                r.cache().clear();
                cpu.cache().clear();
                gpu.clear_cache();
                let a = r.render_region_as(&image, &s, level, rect, output).unwrap();
                let b = cpu
                    .render_region_as(&image, &s, level, rect, output)
                    .unwrap();
                let d = f32_or_u8(&a, &b, output);
                eprintln!("{id} L{level} {output:?}: {d:e}");
                match output {
                    RenderOutput::Display => assert!(d <= 1.0, "L{level}: {d}"),
                    RenderOutput::SceneLinear => assert!(d <= 2e-3, "L{level}: {d}"),
                }
            }
        }
        // Effects only (per-operator tolerance): the cached constants map
        // must reproduce the scalar vignette/grain.
        let mut s = DevelopSettings::default();
        s.detail.sharpening.amount = 0.0;
        s.detail.noise_reduction.color = 0.0;
        s.effects.vignette.amount = -45.0;
        s.effects.vignette.feather = 70.0;
        s.effects.grain.amount = 40.0;
        let rect = PixelRect::full(image.level_extent(1));
        r.cache().clear();
        cpu.cache().clear();
        gpu.clear_cache();
        let a = r
            .render_region_as(&image, &s, 1, rect, RenderOutput::SceneLinear)
            .unwrap();
        let b = cpu
            .render_region_as(&image, &s, 1, rect, RenderOutput::SceneLinear)
            .unwrap();
        let d = f32_diff(&a, &b);
        eprintln!("{id} effects map: {d:e}");
        assert!(d <= 1e-4, "{d}");
    }
}

/// Point-stage edits on a warm level cost one fused dispatch plus output:
/// no Detail, WB or per-tile work.
#[test]
fn warm_point_edits_are_one_fused_level_pass() {
    let gpu = gpu();
    let r = renderer(&gpu);
    let image = common::synthetic(5104, 900, 700, common::RGGB, [0, 0, 900, 700]);
    let rect = PixelRect::full(image.level_extent(1));
    let mut s = DevelopSettings::default();
    r.render_region(&image, &s, 1, rect).unwrap();
    for ev in [0.1f32, 0.2, 0.3] {
        s.tone.exposure = ev;
        let before = gpu.stats();
        r.render_region(&image, &s, 1, rect).unwrap();
        let after = gpu.stats();
        assert_eq!(after.submissions - before.submissions, 1);
        assert_eq!(after.fused_dispatches - before.fused_dispatches, 1);
        assert_eq!(after.uploads, before.uploads, "no pixel uploads");
        // One fused pass plus one crop per output tile for CPU readback.
        let tiles = Renderer::tiles_for(&image, 1, rect).len() as u64;
        assert_eq!(after.last_resident_dispatches, 1 + tiles);
    }
}

/// Level 0 does not retain the output-demosaic checkpoint (a crop of the
/// memoized sensor demosaic): per-tile Detail edits whose WB + Detail tiles
/// fit the budget reach a steady state without re-decoding (large frames at
/// L0 re-decoded on every Detail edit before).
#[test]
fn level_zero_detail_edits_do_not_redecode() {
    let gpu = Arc::new(GpuStageOp::with_cache_budget(
        Arc::new(GpuContext::new().unwrap()),
        // Fits WB + Detail + transients, not also the duplicate checkpoint
        // (which re-decoded every edit before: [20, 5, 15, 2, 18] uploads).
        30 << 20,
    ));
    let r = renderer(&gpu);
    let image = common::synthetic(5105, 1104, 904, common::RGGB, [2, 2, 1100, 900]);
    // All but the last tile column: the per-tile resident path.
    let rect = PixelRect::new(0, 0, 1024, 900);
    let mut s = DevelopSettings::default();
    let mut uploads = Vec::new();
    for i in 0..5 {
        s.detail.sharpening.amount = 40.0 + i as f32;
        let before = gpu.stats().uploads;
        r.render_region(&image, &s, 0, rect).unwrap();
        uploads.push(gpu.stats().uploads - before);
    }
    assert!(uploads[0] > 0);
    assert_eq!(&uploads[2..], &[0, 0, 0], "{uploads:?}");
}
