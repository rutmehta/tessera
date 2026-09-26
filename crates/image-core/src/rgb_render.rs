//! Exact f32 RGB Develop checkpoints, independent of the legacy f16 tile memo.
use super::*;
use pipeline_cpu::{Image, ResolvedLens};
use std::collections::VecDeque;

/// Persistent RGB payload and per-stage execution counters (cache hits excluded).
#[derive(Clone, Debug, Default)]
pub struct RgbCacheStats {
    pub bytes: usize,
    pub executions: [u64; StageId::COUNT],
}
#[derive(Default)]
pub(super) struct RgbMemo {
    entries: VecDeque<(ParamHash, Arc<Image>, ResolvedLens)>,
    stats: RgbCacheStats,
}
impl RgbMemo {
    fn get(&mut self, key: ParamHash) -> Option<(Arc<Image>, ResolvedLens)> {
        let i = self.entries.iter().position(|e| e.0 == key)?;
        let entry = self.entries.remove(i)?;
        let value = (entry.1.clone(), entry.2.clone());
        self.entries.push_back(entry);
        Some(value)
    }
    fn insert(&mut self, key: ParamHash, image: Arc<Image>, lens: ResolvedLens, budget: usize) {
        let bytes = image.width() as usize * image.height() as usize * 12;
        if bytes > budget {
            return;
        }
        while self.stats.bytes + bytes > budget || self.entries.len() >= 64 {
            if let Some((_, old, _)) = self.entries.pop_front() {
                self.stats.bytes -= old.width() as usize * old.height() as usize * 12;
            } else {
                break;
            }
        }
        self.stats.bytes += bytes;
        self.entries.push_back((key, image, lens));
    }
}

// Disable all creative operators. The public resolved reference entry point
// supplies the private optics operators without duplicating their mathematics.
fn neutral() -> DevelopSettings {
    let mut s = DevelopSettings::default();
    s.detail.sharpening.amount = 0.;
    s.detail.noise_reduction.color = 0.;
    s.lens.profile = engine_api::recipe::settings::LensProfileSource::None;
    s.lens.remove_chromatic_aberration = false;
    s
}

impl Renderer {
    pub fn rgb_cache_stats(&self) -> RgbCacheStats {
        self.rgb_memo
            .lock()
            .expect("RGB memo poisoned")
            .stats
            .clone()
    }

    /// Full-resolution native RGB Develop, including automatic optics. Source
    /// IDs must name immutable pixels; change `revision` when a layer changes.
    /// Returns scene-linear f32, never consulting the f16 tile cache. Clones of
    /// this renderer share a byte-bounded LRU. Oversize frames render uncached.
    /// Lens alignment is cached before WB; profile gain remains after WB.
    pub fn render_rgb_linear(
        &self,
        image: &RawImage,
        revision: u64,
        settings: &DevelopSettings,
        cancel: &CancellationToken,
    ) -> EngineResult<Arc<Image>> {
        cancel.check()?;
        if self.is_adobe() {
            return Err(EngineError::Unsupported {
                what: "native RGB Develop entry point requires native process version".into(),
            });
        }
        self.validate_settings(settings)?;
        let source = image
            .rgb()
            .ok_or_else(|| EngineError::invalid("source", "RGB required"))?
            .pixels();
        let mut key = ParamHash::of(
            StageId::Decode,
            &(
                image.id(),
                revision,
                source.width(),
                source.height(),
                self.config.process_version,
            ),
        );
        let mut memo = self.rgb_memo.lock().expect("RGB memo poisoned");
        key = ParamHash::chain(key, ParamHash::of(StageId::Lens, &settings.lens));
        let lens_key = key;
        let mut keys = Vec::new();
        for stage in [
            StageId::WhiteBalance,
            StageId::Detail,
            StageId::Tone,
            StageId::Color,
            StageId::Locals,
            StageId::Effects,
            StageId::Geometry,
        ] {
            let hash = match stage {
                StageId::WhiteBalance => ParamHash::of(stage, &settings.white_balance),
                StageId::Detail => ParamHash::of(stage, &settings.detail),
                StageId::Tone => ParamHash::of(stage, &settings.tone),
                StageId::Color => ParamHash::of(stage, &settings.color),
                StageId::Locals => ParamHash::of(stage, &settings.locals),
                // Effects are anchored to the eventual crop.
                StageId::Effects => {
                    ParamHash::of(stage, &(&settings.effects, &settings.geometry.crop))
                }
                _ => ParamHash::of(stage, &settings.geometry),
            };
            key = ParamHash::chain(key, hash);
            keys.push((stage, key));
        }
        // Search from the latest stage: an evicted prefix is irrelevant when
        // its descendant is still resident (especially under small budgets).
        let cached = keys
            .iter()
            .enumerate()
            .rev()
            .find_map(|(i, (_, key))| memo.get(*key).map(|value| (i + 1, value)));
        let start = cached.as_ref().map_or(0, |(i, _)| *i);
        key = lens_key;
        let (mut rgb, correction) = if let Some((_, (rgb, correction))) = cached {
            (rgb, correction)
        } else if let Some((rgb, correction)) = memo.get(key) {
            (rgb, correction)
        } else {
            let correction =
                pipeline_cpu::resolve_lens(source, &settings.lens, None, &Default::default())?;
            let mut prefix = neutral();
            prefix.lens = settings.lens.clone();
            // Defer the common warp until after effects. CA still uses the
            // original calibration and is applied before WB by the reference.
            prefix.lens.distortion_scale = 0.;
            prefix.lens.manual_distortion = 0.;
            prefix.lens.vignetting_scale = 0.;
            prefix.lens.manual_vignetting = 0.;
            let rgb = Arc::new(pipeline_cpu::render_linear_scaled_resolved(
                &prefix,
                &pipeline_cpu::RenderSource::Rgb(source),
                1,
                &correction,
            )?);
            cancel.check()?;
            memo.stats.executions[StageId::Lens.index()] += 1;
            memo.insert(
                key,
                rgb.clone(),
                correction.clone(),
                self.config.cache_budget_bytes,
            );
            (rgb, correction)
        };
        let extent = Extent::new(source.width(), source.height());
        for (stage, key) in keys.into_iter().skip(start) {
            cancel.check()?;
            let run = |op| self.ops.run_image(stage, &op, (*rgb).clone(), cancel);
            let next = match stage {
                StageId::WhiteBalance => {
                    let mut gain = neutral();
                    gain.white_balance = settings.white_balance.clone();
                    gain.lens.distortion_scale = 0.;
                    gain.lens.vignetting_scale = settings.lens.vignetting_scale;
                    gain.lens.manual_vignetting = settings.lens.manual_vignetting;
                    gain.lens.manual_vignetting_midpoint = settings.lens.manual_vignetting_midpoint;
                    pipeline_cpu::render_linear_scaled_resolved(
                        &gain,
                        &pipeline_cpu::RenderSource::Rgb(&rgb),
                        1,
                        &correction,
                    )?
                }
                StageId::Detail => run(Op::Detail(&settings.detail))?,
                StageId::Tone => {
                    let toned = run(Op::Tone(&settings.tone))?;
                    self.ops
                        .run_image(stage, &Op::ToneExtra(&settings.tone), toned, cancel)?
                }
                StageId::Color => run(Op::Color(&settings.color))?,
                StageId::Locals => pipeline_cpu::locals_image(
                    &rgb,
                    &settings.locals.adjustments,
                    Default::default(),
                )?,
                StageId::Effects => run(Op::EffectsInCrop(
                    &settings.effects,
                    extent,
                    &settings.geometry.crop,
                ))?,
                _ => {
                    let mut suffix = neutral();
                    suffix.geometry = settings.geometry.clone();
                    suffix.lens.distortion_scale = settings.lens.distortion_scale;
                    suffix.lens.manual_distortion = settings.lens.manual_distortion;
                    suffix.lens.vignetting_scale = 0.;
                    pipeline_cpu::render_linear_scaled_resolved(
                        &suffix,
                        &pipeline_cpu::RenderSource::Rgb(&rgb),
                        1,
                        &correction,
                    )?
                }
            };
            cancel.check()?;
            rgb = Arc::new(next);
            memo.stats.executions[stage.index()] += 1;
            memo.insert(
                key,
                rgb.clone(),
                correction.clone(),
                self.config.cache_budget_bytes,
            );
        }
        cancel.check()?;
        Ok(rgb)
    }
}
