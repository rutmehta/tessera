//! Stage operator backends.

use std::sync::atomic::{AtomicU64, Ordering};

use engine_api::EngineResult;
use engine_api::color::ColorMatrix3;
use engine_api::recipe::settings::{GamutMapping, HighlightReconstruction, ToneSettings};
use engine_api::stage::StageId;
use engine_api::tile::Tile;
use pipeline_cpu::{DemosaicAlgorithm, SigmoidSettings};
use raw_decode::CfaLayout;

/// One operator invocation with its resolved parameters.
#[derive(Debug, Clone, Copy)]
pub enum Op<'a> {
    /// Linearize: highlight handling on a CFA tile with halo. Returns the
    /// interior only.
    Highlights {
        /// Sensor pattern, indexed in full sensor coordinates.
        cfa: CfaLayout,
        /// Reconstruction mode.
        mode: HighlightReconstruction,
    },
    /// Demosaic a CFA tile with halo to RGB. Returns the interior only.
    Demosaic {
        /// Sensor pattern.
        cfa: CfaLayout,
        /// Resolved algorithm.
        algorithm: DemosaicAlgorithm,
    },
    /// A 3×3 colour matrix (CameraProfile and WhiteBalance).
    Matrix(ColorMatrix3),
    /// Scene-linear exposure and tone controls.
    Tone(&'a ToneSettings),
    /// Display transform to 8-bit sRGB (`U8` output tile).
    Display {
        /// Gamut mapping mode.
        gamut: GamutMapping,
    },
}

/// A backend that executes stage operators on single tiles.
///
/// Inputs arrive `F32Planar`, already carrying whatever halo the operator
/// needs; the graph gathers halos and owns memoization and resampling. The
/// input is passed by value so point operators can work in place.
/// [`CpuStageOp`] is the reference implementation; a GPU implementation must
/// match it within the contract's regression tolerance.
pub trait StageOp: Send + Sync {
    /// Runs `op`, which belongs to `stage`, on `input`.
    fn run(&self, stage: StageId, op: &Op<'_>, input: Tile) -> EngineResult<Tile>;
}

/// Scalar reference operators from `pipeline-cpu`.
#[derive(Debug, Default, Clone, Copy)]
pub struct CpuStageOp;

impl StageOp for CpuStageOp {
    fn run(&self, _stage: StageId, op: &Op<'_>, mut input: Tile) -> EngineResult<Tile> {
        match *op {
            Op::Highlights { cfa, mode } => pipeline_cpu::reconstruct_highlights(&input, cfa, mode),
            Op::Demosaic { cfa, algorithm } => pipeline_cpu::demosaic(&input, cfa, algorithm),
            Op::Matrix(m) => {
                pipeline_cpu::apply_matrix(&mut input, m)?;
                Ok(input)
            }
            Op::Tone(settings) => {
                pipeline_cpu::tone(&mut input, settings)?;
                Ok(input)
            }
            Op::Display { gamut } => {
                pipeline_cpu::display(&input, SigmoidSettings::default(), gamut)
            }
        }
    }
}

/// Wraps a backend and counts invocations per stage (diagnostics and tests).
#[derive(Debug, Default)]
pub struct CountingStageOp<O> {
    inner: O,
    counts: [AtomicU64; StageId::COUNT],
}

impl<O> CountingStageOp<O> {
    /// Wraps `inner` with zeroed counters.
    pub fn new(inner: O) -> Self {
        Self {
            inner,
            counts: Default::default(),
        }
    }

    /// Invocations of `stage` so far.
    pub fn count(&self, stage: StageId) -> u64 {
        self.counts[stage.index()].load(Ordering::Relaxed)
    }

    /// All counters, in pipeline order.
    pub fn counts(&self) -> [u64; StageId::COUNT] {
        std::array::from_fn(|i| self.counts[i].load(Ordering::Relaxed))
    }

    /// Resets every counter to zero.
    pub fn reset(&self) {
        for c in &self.counts {
            c.store(0, Ordering::Relaxed);
        }
    }
}

impl<O: StageOp> StageOp for CountingStageOp<O> {
    fn run(&self, stage: StageId, op: &Op<'_>, input: Tile) -> EngineResult<Tile> {
        self.counts[stage.index()].fetch_add(1, Ordering::Relaxed);
        self.inner.run(stage, op, input)
    }
}
