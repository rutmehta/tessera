//! CPU compatibility barriers over a caller-selected native backend.
use crate::{CpuStageOp, Op, StageOp};
use engine_api::{
    EngineResult,
    color::{ChromaticAdaptation, WorkingSpace},
    jobs::CancellationToken,
    stage::StageId,
    tile::Tile,
};
use std::sync::{
    Arc, Mutex, MutexGuard,
    atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering},
};

/// Adobe PV1–6 approximations. No resident transaction crosses this wrapper:
/// GPU native stages return host tiles before compatibility CPU work begins.
pub struct AdobeStageOp {
    native: Arc<dyn StageOp>,
    profile: Option<Arc<pipeline_adobe::dcp::DcpProfile>>,
    white_balance: Option<pipeline_adobe::dcp::DcpWhiteBalance>,
    baseline_exposure: f32,
    counts: [AtomicU64; StageId::COUNT],
}
impl AdobeStageOp {
    /// Wrap the chosen native CPU/GPU backend.
    pub fn new(native: Arc<dyn StageOp>) -> Self {
        Self {
            native,
            profile: None,
            white_balance: None,
            baseline_exposure: 0.,
            counts: Default::default(),
        }
    }
    pub(crate) fn with_baseline(
        native: Arc<dyn StageOp>,
        baseline_exposure: f32,
    ) -> EngineResult<Self> {
        pipeline_adobe::validate_baseline_exposure(baseline_exposure)?;
        Ok(Self {
            baseline_exposure,
            ..Self::new(native)
        })
    }
    pub(crate) fn with_profile(
        native: Arc<dyn StageOp>,
        profile: Arc<pipeline_adobe::dcp::DcpProfile>,
        image: &crate::RawImage,
        settings: &engine_api::recipe::DevelopSettings,
    ) -> EngineResult<Self> {
        let m = image.metadata();
        let camera_xyz = pipeline_cpu::camera_to_xyz(engine_api::color::ColorMatrix3(
            std::array::from_fn(|r| m.cam_xyz[r].map(f64::from)),
        ))?;
        let white_balance =
            profile.resolve_for_camera(&settings.white_balance, camera_xyz, m.as_shot_wb)?;
        let baseline_gain = 2f32.powf(m.baseline_exposure);
        if !baseline_gain.is_finite() || baseline_gain <= 0. {
            return Err(engine_api::EngineError::invalid(
                "BaselineExposure",
                "finite positive gain required",
            ));
        }
        Ok(Self {
            baseline_exposure: m.baseline_exposure,
            profile: Some(profile),
            white_balance: Some(white_balance),
            ..Self::new(native)
        })
    }
    fn count(&self, stage: StageId) {
        self.counts[stage.index()].fetch_add(1, Ordering::Relaxed);
    }
}
impl StageOp for AdobeStageOp {
    fn adobe_invocations(&self, stage: StageId) -> u64 {
        self.counts[stage.index()].load(Ordering::Relaxed)
    }
    fn blend_local(
        &self,
        base: &pipeline_cpu::Image,
        adjusted: &pipeline_cpu::Image,
        mask: &[f32],
    ) -> EngineResult<pipeline_cpu::Image> {
        self.native.blend_local(base, adjusted, mask)
    }
    fn run(&self, stage: StageId, op: &Op<'_>, mut input: Tile) -> EngineResult<Tile> {
        if matches!(
            stage,
            StageId::CameraProfile
                | StageId::WhiteBalance
                | StageId::Tone
                | StageId::Color
                | StageId::Detail
                | StageId::Effects
        ) {
            self.count(stage);
            if let Op::Tone(s) = op {
                if let Some(legacy) = &s.legacy_pv2010 {
                    pipeline_cpu::legacy_pv2010::validate(legacy)?;
                }
                let mut basic = (*s).clone();
                if let Some(profile) = &self.profile {
                    pipeline_cpu::map_rgb(&mut input, |p| {
                        profile
                            .apply_exposure(p, self.baseline_exposure + s.exposure.clamp(-10., 10.))
                    })?;
                } else {
                    let gain = (self.baseline_exposure + s.exposure.clamp(-10., 10.)).exp2();
                    pipeline_cpu::map_rgb(&mut input, |p| p.map(|v| v * gain))?;
                }
                basic.exposure = 0.;
                pipeline_cpu::map_rgb(&mut input, |p| pipeline_adobe::basic_tone(p, &basic))?;
                return Ok(input);
            }
            if let Some(profile) = &self.profile {
                if stage == StageId::CameraProfile {
                    pipeline_cpu::map_rgb(&mut input, |p| {
                        profile.apply_camera(
                            p,
                            self.white_balance
                                .as_ref()
                                .expect("resolved profile white balance"),
                        )
                    })?;
                    return Ok(input);
                }
                if stage == StageId::WhiteBalance {
                    return Ok(input);
                }
            }
            return CpuStageOp.run(stage, op, input);
        }
        if let Op::Display { gamut, headroom } = *op {
            // No second sigmoid; the recipe's gamut mapping, as export (ENG-9).
            return CpuStageOp::adobe_display(input, gamut, headroom);
        }
        self.native.run(stage, op, input)
    }
    fn run_image(
        &self,
        stage: StageId,
        op: &Op<'_>,
        input: pipeline_cpu::Image,
        cancel: &CancellationToken,
    ) -> EngineResult<pipeline_cpu::Image> {
        cancel.check()?;
        match op {
            Op::ToneExtra(s) => {
                self.count(stage);
                let curves = s.curves_extended.as_ref().unwrap_or(&s.curves);
                pipeline_adobe::curves::validate_domain(curves, s.curves_extended.is_some())?;
                let mut extra = (*s).clone();
                extra.curves_extended = None;
                extra.curves = Default::default();
                extra.curves.parametric = s.curves.parametric.clone();
                let output = pipeline_cpu::tone_extra_image(&input, &extra)?;
                let to_pro = WorkingSpace::LinearRec2020
                    .conversion_to(WorkingSpace::LinearProPhoto, ChromaticAdaptation::Bradford)?;
                let from_pro = to_pro.inverse()?;
                let coords: Vec<_> = output.coords().collect();
                // Point operators, in place: each worker reads and writes only
                // its own tiles (no halo), so the result is the serial one.
                let output = Mutex::new(output);
                for_each_tile(&coords, cancel, |coord| {
                    let mut tile = lock(&output).tile(coord, 0, 1)?;
                    pipeline_cpu::apply_matrix(&mut tile, to_pro)?;
                    pipeline_cpu::map_rgb(&mut tile, |p| {
                        pipeline_adobe::curves::apply_domain(
                            if self.profile.is_none() {
                                p.map(|x| {
                                    if s.curves_extended.is_some() && !(0. ..=1.).contains(&x) {
                                        x
                                    } else {
                                        pipeline_adobe::curves::default_tone(x)
                                    }
                                })
                            } else {
                                p
                            },
                            curves,
                            s.curves_extended.is_some(),
                        )
                    })?;
                    pipeline_cpu::apply_matrix(&mut tile, from_pro)?;
                    lock(&output).put(&tile)
                })?;
                Ok(output.into_inner().unwrap_or_else(|e| e.into_inner()))
            }
            Op::Tone(_)
            | Op::Detail(_)
            | Op::Color(_)
            | Op::Effects(_, _)
            | Op::EffectsInCrop(_, _, _) => {
                let halo = if let Op::Detail(s) = op {
                    pipeline_cpu::detail_halo(s)
                } else {
                    0
                };
                // Every tile reads its halo from the immutable input and is
                // written once, so tiles run in parallel with serial results
                // (ENG-10: this loop was most of an Adobe export's time).
                // Point operators (no halo) read and write only their own
                // tile, so they work in place without a second frame.
                let coords: Vec<_> = input.coords().collect();
                if halo == 0 {
                    let frame = Mutex::new(input);
                    for_each_tile(&coords, cancel, |coord| {
                        let tile = lock(&frame).tile(coord, 0, 1)?;
                        let tile = self.barrier_tile(stage, op, tile)?;
                        lock(&frame).put(&tile)
                    })?;
                    return Ok(frame.into_inner().unwrap_or_else(|e| e.into_inner()));
                }
                let output = Mutex::new(pipeline_cpu::Image::new(
                    input.width(),
                    input.height(),
                    vec![
                        vec![0.; input.width() as usize * input.height() as usize];
                        input.planes().len()
                    ],
                )?);
                for_each_tile(&coords, cancel, |coord| {
                    let tile = self.barrier_tile(stage, op, input.tile(coord, halo, 1)?)?;
                    lock(&output).put(&tile)
                })?;
                Ok(output.into_inner().unwrap_or_else(|e| e.into_inner()))
            }
            _ => self.native.run_image(stage, op, input, cancel),
        }
    }
}

impl AdobeStageOp {
    /// One tile of an image-level compatibility barrier.
    fn barrier_tile(&self, stage: StageId, op: &Op<'_>, tile: Tile) -> EngineResult<Tile> {
        let mut tile = self.run(stage, op, tile)?;
        // Profile tone runs once, not in upstream WB assembly.
        if matches!(op, Op::Tone(_))
            && let Some(profile) = &self.profile
        {
            pipeline_cpu::map_rgb(&mut tile, |p| profile.apply_tone(profile.apply_look(p)))?;
        }
        Ok(tile)
    }
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// Runs `f` once per tile on scoped worker threads (one per core), taking
/// tiles in order from a shared cursor. The cancellation token is polled
/// before every tile; the first error stops further tiles and is returned.
fn for_each_tile(
    coords: &[engine_api::tile::TileCoord],
    cancel: &CancellationToken,
    f: impl Fn(engine_api::tile::TileCoord) -> EngineResult<()> + Sync,
) -> EngineResult<()> {
    let workers = std::thread::available_parallelism()
        .map_or(1, usize::from)
        .min(coords.len());
    if workers <= 1 {
        for &coord in coords {
            cancel.check()?;
            f(coord)?;
        }
        return Ok(());
    }
    let next = AtomicUsize::new(0);
    let failed = AtomicBool::new(false);
    let first: Mutex<Option<(usize, engine_api::EngineError)>> = Mutex::new(None);
    std::thread::scope(|scope| {
        for _ in 0..workers {
            scope.spawn(|| {
                loop {
                    let i = next.fetch_add(1, Ordering::Relaxed);
                    if i >= coords.len() || failed.load(Ordering::Relaxed) {
                        break;
                    }
                    if let Err(e) = cancel.check().and_then(|()| f(coords[i])) {
                        failed.store(true, Ordering::Relaxed);
                        let mut first = lock(&first);
                        // The earliest failing tile among those that ran.
                        if first.as_ref().is_none_or(|(j, _)| i < *j) {
                            *first = Some((i, e));
                        }
                    }
                }
            });
        }
    });
    match first.into_inner().unwrap_or_else(|e| e.into_inner()) {
        Some((_, e)) => Err(e),
        None => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use engine_api::recipe::settings::{ColorSettings, DetailSettings, ToneSettings};

    fn bits(image: &pipeline_cpu::Image) -> Vec<u32> {
        image
            .planes()
            .iter()
            .flatten()
            .map(|v| v.to_bits())
            .collect()
    }

    /// ENG-10: the image-level compatibility barriers run their tiles in
    /// parallel; the result is the serial tile loop's, bit for bit (Detail
    /// with a halo across tile seams, Tone, Colour, ToneExtra curves).
    #[test]
    fn eng10_parallel_barriers_equal_the_serial_tile_loop() {
        let (w, h) = (
            3 * engine_api::tile::TILE_SIZE - 37,
            2 * engine_api::tile::TILE_SIZE + 9,
        );
        let n = (w * h) as usize;
        let input = pipeline_cpu::Image::new(
            w,
            h,
            (0..3)
                .map(|c| {
                    (0..n)
                        .map(|i| {
                            let (x, y) = (i as u32 % w, i as u32 / w);
                            0.02 + 0.3 * (((x / 7 + y / 5 + c) % 5) as f32 / 4.)
                                + 0.1 * (x as f32 / w as f32)
                        })
                        .collect()
                })
                .collect(),
        )
        .unwrap();
        let op = AdobeStageOp::new(Arc::new(CpuStageOp));
        let cancel = CancellationToken::new();
        let mut detail = DetailSettings::default();
        detail.sharpening.amount = 70.;
        let tone = ToneSettings {
            contrast: 30.,
            highlights: -40.,
            shadows: 20.,
            ..Default::default()
        };
        let color = ColorSettings {
            vibrance: 25.,
            saturation: 10.,
            ..Default::default()
        };
        for (stage, op_value) in [
            (StageId::Detail, Op::Detail(&detail)),
            (StageId::Tone, Op::Tone(&tone)),
            (StageId::Color, Op::Color(&color)),
        ] {
            let halo = if let Op::Detail(s) = op_value {
                pipeline_cpu::detail_halo(s)
            } else {
                0
            };
            let mut serial = input.clone();
            for coord in input.coords() {
                serial
                    .put(
                        &op.run(stage, &op_value, input.tile(coord, halo, 1).unwrap())
                            .unwrap(),
                    )
                    .unwrap();
            }
            let parallel = op
                .run_image(stage, &op_value, input.clone(), &cancel)
                .unwrap();
            assert_eq!(bits(&parallel), bits(&serial), "{stage:?}");
        }
        // ToneExtra: the global operator, then the serial curve loop.
        let mut serial = pipeline_cpu::tone_extra_image(&input, &tone).unwrap();
        let to_pro = WorkingSpace::LinearRec2020
            .conversion_to(WorkingSpace::LinearProPhoto, ChromaticAdaptation::Bradford)
            .unwrap();
        let from_pro = to_pro.inverse().unwrap();
        for coord in serial.coords().collect::<Vec<_>>() {
            let mut tile = serial.tile(coord, 0, 1).unwrap();
            pipeline_cpu::apply_matrix(&mut tile, to_pro).unwrap();
            pipeline_cpu::map_rgb(&mut tile, |p| {
                pipeline_adobe::curves::apply_domain(
                    p.map(pipeline_adobe::curves::default_tone),
                    &tone.curves,
                    false,
                )
            })
            .unwrap();
            pipeline_cpu::apply_matrix(&mut tile, from_pro).unwrap();
            serial.put(&tile).unwrap();
        }
        let parallel = op
            .run_image(StageId::Tone, &Op::ToneExtra(&tone), input.clone(), &cancel)
            .unwrap();
        assert_eq!(bits(&parallel), bits(&serial), "ToneExtra");
        // A cancelled token stops the barrier.
        let cancelled = CancellationToken::new();
        cancelled.cancel();
        assert!(
            op.run_image(StageId::Detail, &Op::Detail(&detail), input, &cancelled)
                .is_err()
        );
    }
}
