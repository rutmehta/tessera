mod common;
use engine_api::{EngineResult, recipe::DevelopSettings, stage::StageId, tile::Tile};
use image_core::{CpuStageOp, Op, PixelRect, Renderer, RendererConfig, StageOp, TileCache};
use std::sync::Arc;

struct OwnedCpu;
impl StageOp for OwnedCpu {
    fn run(&self, stage: StageId, op: &Op<'_>, input: Tile) -> EngineResult<Tile> {
        if stage == StageId::Tone {
            assert!(
                input.is_unique(),
                "CPU tone should receive its uniquely-owned resampled tile"
            );
        }
        CpuStageOp.run(stage, op, input)
    }
}

#[test]
fn cpu_batch_hook_preserves_in_place_ownership() {
    let image = common::synthetic(8, 300, 280, common::RGGB, [0, 0, 300, 280]);
    for threads in [1, 4] {
        let r = Renderer::with_ops(
            Arc::new(OwnedCpu),
            Arc::new(TileCache::new(0)),
            RendererConfig {
                threads,
                ..Default::default()
            },
        );
        r.render_region(
            &image,
            &DevelopSettings::default(),
            0,
            PixelRect::full(image.level_extent(0)),
        )
        .unwrap();
    }
}
