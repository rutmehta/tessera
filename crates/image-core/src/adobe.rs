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
                let curves = s.curves_extended.as_ref().unwrap_or(&s.curves);
                pipeline_adobe::curves::validate_domain(curves, s.curves_extended.is_some())?;
                let mut extra = (*s).clone();
                extra.curves_extended = None;
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
                        pipeline_cpu::map_rgb(&mut tile, |p| {
                            profile.apply_tone(profile.apply_look(p))
                        })?;
                    }
                    output.put(&tile)?;
                }
                Ok(output)
            }
            _ => self.native.run_image(stage, op, input, cancel),
        }
    }
}
