//! Stage operator backends.

use std::sync::atomic::{AtomicU64, Ordering};

use engine_api::EngineResult;
use engine_api::color::ColorMatrix3;
use engine_api::recipe::settings::{
    ColorSettings, DetailSettings, EffectsSettings, GamutMapping, GeometrySettings,
    HighlightReconstruction, ToneSettings,
};
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
    /// Neighbourhood detail controls; input includes the resolved halo.
    Detail(&'a DetailSettings),
    /// Global tone statistics and curves.
    ToneExtra(&'a ToneSettings),
    /// Creative colour controls.
    Color(&'a ColorSettings),
    /// Crop and straighten (image-level only).
    Geometry(&'a GeometrySettings),
    /// Post-crop effects in the supplied image coordinate domain.
    Effects(&'a EffectsSettings, engine_api::tile::Extent),
    /// Effects pulled back through the final crop/rotation coordinate map.
    EffectsInCrop(
        &'a EffectsSettings,
        engine_api::tile::Extent,
        &'a engine_api::recipe::settings::Crop,
    ),
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
    /// Optional resident graph execution; CPU implementations need no changes.
    fn begin_resident(&self) -> Option<Box<dyn crate::resident::ResidentBatch + '_>> {
        None
    }

    /// Runs `op`, which belongs to `stage`, on `input`.
    fn run(&self, stage: StageId, op: &Op<'_>, input: Tile) -> EngineResult<Tile>;

    /// Whole-image barrier for global statistics and geometry. Backends may
    /// override this; the default CPU fallback avoids 256-pixel Tile limits.
    /// Other operators still dispatch through `run`, gathering from an immutable
    /// source so neighbourhood filters never read already-processed neighbours.
    fn run_image(
        &self,
        stage: StageId,
        op: &Op<'_>,
        input: pipeline_cpu::Image,
        cancel: &engine_api::jobs::CancellationToken,
    ) -> EngineResult<pipeline_cpu::Image> {
        cancel.check()?;
        let output = match *op {
            Op::ToneExtra(s) => pipeline_cpu::tone_extra_image(&input, s)?,
            Op::Geometry(s) => pipeline_cpu::geometry(&input, s)?,
            _ => {
                let halo = match *op {
                    Op::Detail(s) => pipeline_cpu::detail_halo(s),
                    _ => 0,
                };
                let mut output = input.clone();
                for coord in input.coords() {
                    cancel.check()?;
                    output.put(&self.run(stage, op, input.tile(coord, halo, 1)?)?)?;
                }
                output
            }
        };
        cancel.check()?;
        Ok(output)
    }

    /// Preferred submission size. One retains the renderer's CPU parallelism.
    fn batch_size(&self) -> usize {
        1
    }

    /// Runs a contiguous chain on each tile, preserving input order. No halo
    /// gathering, resampling or cache access happens inside a chain. The GPU
    /// override keeps intermediates resident and reads only the final output.
    /// Empty chains are identity. Cancellation is checked between operations.
    fn run_chain_batch(
        &self,
        chain: &[(StageId, Op<'_>)],
        inputs: Vec<Tile>,
        cancel: &engine_api::jobs::CancellationToken,
    ) -> EngineResult<Vec<Tile>> {
        cancel.check()?;
        inputs
            .into_iter()
            .map(|mut tile| {
                for (stage, op) in chain {
                    cancel.check()?;
                    tile = self.run(*stage, op, tile)?;
                }
                Ok(tile)
            })
            .collect()
    }
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
            Op::Detail(s) => {
                pipeline_cpu::detail(&mut input, s)?;
                Ok(input)
            }
            Op::ToneExtra(s) => {
                pipeline_cpu::tone_extra(&mut input, s)?;
                Ok(input)
            }
            Op::Color(s) => {
                pipeline_cpu::color(&mut input, s)?;
                Ok(input)
            }
            Op::Effects(s, extent) => {
                pipeline_cpu::effects(&mut input, s, extent)?;
                Ok(input)
            }
            Op::EffectsInCrop(s, extent, crop) => {
                pipeline_cpu::effects_in_crop(&mut input, s, extent, crop)?;
                Ok(input)
            }
            Op::Geometry(_) => Err(engine_api::EngineError::invalid(
                "geometry",
                "requires run_image",
            )),
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
    fn run_image(
        &self,
        stage: StageId,
        op: &Op<'_>,
        input: pipeline_cpu::Image,
        cancel: &engine_api::jobs::CancellationToken,
    ) -> EngineResult<pipeline_cpu::Image> {
        cancel.check()?;
        let count = if matches!(op, Op::ToneExtra(_) | Op::Geometry(_)) {
            1
        } else {
            input.coords().count() as u64
        };
        self.counts[stage.index()].fetch_add(count, Ordering::Relaxed);
        self.inner.run_image(stage, op, input, cancel)
    }

    fn run(&self, stage: StageId, op: &Op<'_>, input: Tile) -> EngineResult<Tile> {
        self.counts[stage.index()].fetch_add(1, Ordering::Relaxed);
        self.inner.run(stage, op, input)
    }

    fn batch_size(&self) -> usize {
        self.inner.batch_size()
    }

    fn run_chain_batch(
        &self,
        chain: &[(StageId, Op<'_>)],
        inputs: Vec<Tile>,
        cancel: &engine_api::jobs::CancellationToken,
    ) -> EngineResult<Vec<Tile>> {
        cancel.check()?;
        // Count scheduled invocations, including a batch that later fails.
        for (stage, _) in chain {
            self.counts[stage.index()].fetch_add(inputs.len() as u64, Ordering::Relaxed);
        }
        self.inner.run_chain_batch(chain, inputs, cancel)
    }
}
