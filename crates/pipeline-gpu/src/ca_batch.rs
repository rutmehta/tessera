//! Host-tile bridge into the resident sensor-frame demosaic/CA transaction.
use crate::GpuStageOp;
use engine_api::{
    EngineError, EngineResult,
    jobs::CancellationToken,
    tile::{Extent, TILE_SIZE, Tile, TileCoord},
};
use image_core::{Op, StageOp};
use pipeline_cpu::{CaPlan, DemosaicAlgorithm};
use raw_decode::RawMetadata;
use std::collections::HashMap;

/// Be conservative until every staged DNG opcode has a native implementation.
/// The presence bit also covers metadata providers that omit opcode payloads.
pub(crate) fn supports_metadata(metadata: &RawMetadata) -> bool {
    !metadata.has_opcode_list && !metadata.opcode_lists.iter().any(Option::is_some)
}

impl GpuStageOp {
    /// Demosaic linear CFA dependency tiles and correct lateral CA in one
    /// transaction, reading back only the requested camera-RGB output tiles.
    ///
    /// Inputs carry the demosaicer's CFA halo and use level-zero sensor
    /// coordinates. They must cover the CA neighbourhood of every output;
    /// neither the active crop nor preview scaling is applied here. The RGB
    /// gather and CA dispatch share the demosaic encoder (not a fused shader:
    /// neighbouring invocations require a dispatch barrier).
    ///
    /// `None` asks the caller to use its CPU reference path. In particular,
    /// staged DNG opcodes are not implemented by this entry point. An absent
    /// dependency is an error, never a black/edge-clamped substitute.
    #[allow(clippy::too_many_arguments)]
    pub fn demosaic_ca_batch(
        &self,
        metadata: &RawMetadata,
        algorithm: DemosaicAlgorithm,
        plan: &CaPlan,
        inputs: Vec<Tile>,
        outputs: &[TileCoord],
        cancel: &CancellationToken,
    ) -> EngineResult<Option<Vec<Tile>>> {
        cancel.check()?;
        if !supports_metadata(metadata) {
            return Ok(None);
        }
        if outputs.is_empty() {
            return Ok(Some(Vec::new()));
        }
        let frame = Extent::new(metadata.width, metadata.height);
        if frame.width == 0 || frame.height == 0 {
            return Err(EngineError::invalid("demosaic CA", "empty sensor frame"));
        }
        let displacement = plan.max_displacement(frame.width, frame.height);
        let Some(displacement) = displacement
            .filter(|v| v.is_finite() && v.ceil() + 2. <= f64::from(engine_api::tile::MAX_HALO))
        else {
            return Ok(None);
        };
        let halo = displacement.ceil() as u16 + 2;
        let valid_coord = |c: TileCoord| {
            c.level == 0
                && c.x < frame.width.div_ceil(TILE_SIZE)
                && c.y < frame.height.div_ceil(TILE_SIZE)
        };
        if outputs.iter().any(|&c| !valid_coord(c)) {
            return Err(EngineError::invalid(
                "demosaic CA",
                "output outside sensor frame",
            ));
        }
        let mut batch = self.begin_resident().expect("GPU resident backend");
        let mut demosaiced = HashMap::new();
        for input in &inputs {
            cancel.check()?;
            let coord = input.coord();
            if !valid_coord(coord) || demosaiced.contains_key(&coord) {
                return Err(EngineError::invalid(
                    "demosaic CA",
                    "invalid or duplicate dependency",
                ));
            }
            let (x, y) = coord.pixel_origin(TILE_SIZE);
            if input.layout().extent
                != Extent::new(
                    (frame.width - x).min(TILE_SIZE),
                    (frame.height - y).min(TILE_SIZE),
                )
            {
                return Err(EngineError::invalid(
                    "demosaic CA",
                    "dependency extent differs from sensor tile",
                ));
            }
            let tile = batch.upload(input)?;
            let tile = batch.run(
                &Op::Demosaic {
                    cfa: metadata.cfa_layout,
                    algorithm,
                },
                &tile,
            )?;
            demosaiced.insert(coord, tile);
        }
        let mut corrected = Vec::with_capacity(outputs.len());
        for &coord in outputs {
            cancel.check()?;
            let tile = batch.gather(frame, coord, halo, 1, &demosaiced)?;
            corrected.push(batch.lateral_ca(&tile, frame, plan)?);
        }
        Ok(Some(batch.finish(corrected, false, None, cancel)?.tiles))
    }
}
