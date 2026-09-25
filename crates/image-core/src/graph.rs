//! The fixed raw pipeline graph and its memoization policy.

use engine_api::recipe::DevelopSettings;
use engine_api::stage::{MemoKey, ParamHash, StageId};
use engine_api::{id::ImageId, tile::TileCoord};

/// Tile addressing frame of a stage's output.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Frame {
    /// Level-0 tiles over the full sensor plane, including masked margins.
    /// CFA phase is indexed in this frame, so every pre-crop stage uses it.
    Sensor,
    /// The active-area (default crop) pyramid: level `L` is the crop
    /// box-averaged in linear light by `2^L`, tiled from the crop origin.
    Output,
}

/// One stage of the graph.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StageNode {
    /// The stage.
    pub stage: StageId,
    /// Outputs are memoized in the [`crate::TileCache`] as `F16Planar`.
    pub cacheable: bool,
    /// The stage runs an operator in M1. Unimplemented stages are identity
    /// (their settings must be default; `pipeline_cpu::validate_settings`
    /// rejects anything else), so they produce no tiles and no cache entries.
    pub implemented: bool,
    /// Frame the stage's output tiles are addressed in.
    pub frame: Frame,
}

/// The fixed stage order (spec 04 §3) with per-stage memoization flags.
///
/// M1 memoizes three outputs:
/// - `Demosaic` (sensor frame, level 0): the expensive stage; WB and tone
///   edits never re-run it or anything before it.
/// - `Denoise`: marked cacheable for when a real denoiser lands; it is an
///   identity stage in M1 and therefore never materialised.
/// - `WhiteBalance` (output frame, every level): the scene-linear buffer
///   after the last linear stage, cropped and box-downsampled to the level.
///   This is the "cached mid-pipeline buffer" that tone/colour edits start
///   from, so a tone-only change at screen resolution costs one Tone and
///   one Output pass per visible tile.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PipelineGraph {
    nodes: [StageNode; StageId::COUNT],
}

impl Default for PipelineGraph {
    fn default() -> Self {
        Self::m2()
    }
}

impl PipelineGraph {
    /// M2 adds whole-level neighbourhood and geometry barriers. Only the
    /// upstream demosaic/WB buffers are memoized: crop-relative Effects depends
    /// on Geometry settings and must not use an Effects-only chain cache key.
    pub fn m2() -> Self {
        let mut graph = Self::m1();
        for stage in [
            StageId::Detail,
            StageId::Color,
            StageId::Effects,
            StageId::Geometry,
        ] {
            graph.nodes[stage.index()].implemented = true;
        }
        graph
    }

    /// The M1 graph: pipeline-cpu operators for Linearize, Demosaic,
    /// CameraProfile, WhiteBalance, Tone and Output.
    pub fn m1() -> Self {
        Self {
            nodes: StageId::ALL.map(|stage| {
                use StageId::*;
                StageNode {
                    stage,
                    cacheable: matches!(stage, Denoise | Demosaic | WhiteBalance),
                    implemented: matches!(
                        stage,
                        Decode
                            | Linearize
                            | Demosaic
                            | CameraProfile
                            | WhiteBalance
                            | Tone
                            | Output
                    ),
                    frame: if stage <= WhiteBalance {
                        Frame::Sensor
                    } else {
                        Frame::Output
                    },
                }
            }),
        }
    }

    /// Overrides a stage's memoization flag (diagnostics, tests, low-memory
    /// modes). Only `Demosaic` and `WhiteBalance` are materialised in M1.
    pub fn with_cacheable(mut self, stage: StageId, cacheable: bool) -> Self {
        self.nodes[stage.index()].cacheable = cacheable;
        self
    }

    /// Nodes in pipeline order.
    pub fn nodes(&self) -> &[StageNode; StageId::COUNT] {
        &self.nodes
    }

    /// Node for `stage`.
    pub fn node(&self, stage: StageId) -> StageNode {
        self.nodes[stage.index()]
    }

    /// Earliest stage whose output differs between two settings, i.e. the
    /// first stage a re-render must run. `None` means nothing changed.
    pub fn earliest_dirty_stage(
        before: &DevelopSettings,
        after: &DevelopSettings,
    ) -> Option<StageId> {
        before.first_dirty_stage(after)
    }

    /// Implemented stages that a change from `before` to `after` re-runs
    /// when upstream memoized outputs are available, in pipeline order.
    /// Stages before the latest cacheable, materialised stage at or before
    /// the dirty stage are served from cache.
    pub fn stages_to_rerun(
        &self,
        before: &DevelopSettings,
        after: &DevelopSettings,
    ) -> Vec<StageId> {
        let Some(dirty) = Self::earliest_dirty_stage(before, after) else {
            return Vec::new();
        };
        // The resume point is the newest cached output strictly before `dirty`.
        let resume = self
            .nodes
            .iter()
            .filter(|n| n.stage < dirty && n.cacheable && n.implemented)
            .map(|n| n.stage)
            .max();
        self.nodes
            .iter()
            .filter(|n| n.implemented && n.stage != StageId::Decode)
            .filter(|n| resume.is_none_or(|r| n.stage > r))
            .map(|n| n.stage)
            .collect()
    }

    /// Memo key for `stage`'s output tile. `chain` is
    /// `DevelopSettings::stage_chain(process_version.chain_seed())`.
    pub fn memo_key(
        image: ImageId,
        chain: &[(StageId, ParamHash); StageId::COUNT],
        stage: StageId,
        tile: TileCoord,
    ) -> MemoKey {
        MemoKey {
            image_id: image,
            stage,
            params_hash: chain[stage.index()].1,
            tile,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn order_and_flags() {
        let g = PipelineGraph::m1();
        for (i, n) in g.nodes().iter().enumerate() {
            assert_eq!(n.stage.index(), i);
        }
        assert!(g.node(StageId::Demosaic).cacheable);
        assert!(g.node(StageId::Denoise).cacheable && !g.node(StageId::Denoise).implemented);
        assert!(!g.node(StageId::Tone).cacheable);
        assert_eq!(g.node(StageId::WhiteBalance).frame, Frame::Sensor);
        assert_eq!(g.node(StageId::Tone).frame, Frame::Output);
    }

    #[test]
    fn dirty_stage_and_rerun_set() {
        let g = PipelineGraph::m1();
        let a = DevelopSettings::default();
        assert_eq!(PipelineGraph::earliest_dirty_stage(&a, &a), None);
        assert!(g.stages_to_rerun(&a, &a).is_empty());

        let mut b = a.clone();
        b.tone.exposure = 1.0;
        assert_eq!(
            PipelineGraph::earliest_dirty_stage(&a, &b),
            Some(StageId::Tone)
        );
        assert_eq!(g.stages_to_rerun(&a, &b), [StageId::Tone, StageId::Output]);

        let mut c = a.clone();
        c.white_balance.mode = engine_api::recipe::settings::WhiteBalanceMode::Daylight;
        assert_eq!(
            g.stages_to_rerun(&a, &c),
            [
                StageId::CameraProfile,
                StageId::WhiteBalance,
                StageId::Tone,
                StageId::Output
            ]
        );

        let mut d = a.clone();
        d.output.gamut_mapping = engine_api::recipe::settings::GamutMapping::Clip;
        assert_eq!(
            PipelineGraph::earliest_dirty_stage(&a, &d),
            Some(StageId::Output)
        );
        // The WB buffer is cached, but Tone output is not: Tone re-runs too.
        assert_eq!(g.stages_to_rerun(&a, &d), [StageId::Tone, StageId::Output]);

        let mut e = a.clone();
        e.linearize.highlight_reconstruction =
            engine_api::recipe::settings::HighlightReconstruction::Clip;
        assert_eq!(g.stages_to_rerun(&a, &e)[0], StageId::Linearize);
    }
}
