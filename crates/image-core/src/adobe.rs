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
    Arc,
    atomic::{AtomicU64, Ordering},
};

/// Adobe PV3–6 approximations. No resident transaction crosses this wrapper:
/// GPU native stages return host tiles before compatibility CPU work begins.
pub struct AdobeStageOp {
    native: Arc<dyn StageOp>,
    profile: Option<Arc<pipeline_adobe::dcp::DcpProfile>>,
    temperature: f32,
    tint: engine_api::color::ColorMatrix3,
    counts: [AtomicU64; StageId::COUNT],
}
impl AdobeStageOp {
    /// Wrap the chosen native CPU/GPU backend.
    pub fn new(native: Arc<dyn StageOp>) -> Self {
        Self {
            native,
            profile: None,
            temperature: 6504.,
            tint: engine_api::color::ColorMatrix3::IDENTITY,
            counts: Default::default(),
        }
    }
    pub(crate) fn with_profile(
        native: Arc<dyn StageOp>,
        profile: Arc<pipeline_adobe::dcp::DcpProfile>,
        image: &crate::RawImage,
        settings: &engine_api::recipe::DevelopSettings,
    ) -> EngineResult<Self> {
        use engine_api::{color::ColorMatrix3, recipe::settings::WhiteBalanceMode};
        let m = image.metadata();
        let camera_xyz = pipeline_cpu::camera_to_xyz(ColorMatrix3(std::array::from_fn(|r| {
            m.cam_xyz[r].map(f64::from)
        })))?;
        let wb = &settings.white_balance;
        let (temperature, tint) = match wb.mode {
            WhiteBalanceMode::AsShot => {
                pipeline_cpu::as_shot_temperature_tint(camera_xyz, m.as_shot_wb)?
            }
            WhiteBalanceMode::Custom => (wb.temperature, wb.tint),
            WhiteBalanceMode::Daylight | WhiteBalanceMode::Flash => (5503., 0.),
            WhiteBalanceMode::Cloudy => (6504., 0.),
            WhiteBalanceMode::Shade => (7504., 0.),
            WhiteBalanceMode::Tungsten => (2856., 0.),
            WhiteBalanceMode::Fluorescent => (4230., 0.),
            WhiteBalanceMode::Auto => {
                return Err(engine_api::EngineError::invalid(
                    "white balance",
                    "Auto is not implemented",
                ));
            }
        };
        let mut tinted = wb.clone();
        tinted.mode = WhiteBalanceMode::Custom;
        tinted.temperature = temperature;
        tinted.tint = tint;
        let mut neutral = tinted.clone();
        neutral.tint = 0.;
        let tint = if tint == 0. {
            ColorMatrix3::IDENTITY
        } else {
            pipeline_cpu::white_balance_matrix(&tinted, camera_xyz, m.as_shot_wb)?
                * pipeline_cpu::white_balance_matrix(&neutral, camera_xyz, m.as_shot_wb)?
                    .inverse()?
        };
        Ok(Self {
            profile: Some(profile),
            temperature,
            tint,
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
                pipeline_cpu::map_rgb(&mut input, |p| pipeline_adobe::basic_tone(p, s))?;
                return Ok(input);
            }
            if let Some(profile) = &self.profile {
                if stage == StageId::CameraProfile {
                    pipeline_cpu::map_rgb(&mut input, |p| {
                        profile.apply_without_tone(p, self.temperature)
                    })?;
                    return Ok(input);
                }
                if stage == StageId::WhiteBalance {
                    pipeline_cpu::apply_matrix(&mut input, self.tint)?;
                    return Ok(input);
                }
            }
            return CpuStageOp.run(stage, op, input);
        }
        if matches!(op, Op::Display { .. }) {
            // Reuse the native CPU output primitive without a second sigmoid.
            return CpuStageOp::display_linear(input);
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
                let mut extra = (*s).clone();
                extra.curves = Default::default();
                extra.curves.parametric = s.curves.parametric.clone();
                let mut output = pipeline_cpu::tone_extra_image(&input, &extra)?;
                let to_pro = WorkingSpace::LinearRec2020
                    .conversion_to(WorkingSpace::LinearProPhoto, ChromaticAdaptation::Bradford)?;
                let from_pro = to_pro.inverse()?;
                for coord in output.coords() {
                    cancel.check()?;
                    let mut tile = output.tile(coord, 0, 1)?;
                    pipeline_cpu::apply_matrix(&mut tile, to_pro)?;
                    pipeline_cpu::map_rgb(&mut tile, |p| {
                        pipeline_adobe::curves::apply(
                            if self.profile.is_none() {
                                p.map(pipeline_adobe::curves::default_tone)
                            } else {
                                p
                            },
                            &s.curves,
                        )
                    })?;
                    pipeline_cpu::apply_matrix(&mut tile, from_pro)?;
                    output.put(&tile)?;
                }
                Ok(output)
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
                let mut output = input.clone();
                for coord in input.coords() {
                    cancel.check()?;
                    let mut tile = self.run(stage, op, input.tile(coord, halo, 1)?)?;
                    // Profile tone runs once, not in upstream WB assembly.
                    if matches!(op, Op::Tone(_))
                        && let Some(profile) = &self.profile
                    {
                        pipeline_cpu::map_rgb(&mut tile, |p| profile.apply_tone(p))?;
                    }
                    output.put(&tile)?;
                }
                Ok(output)
            }
            _ => self.native.run_image(stage, op, input, cancel),
        }
    }
}
