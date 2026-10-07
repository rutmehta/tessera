use super::*;

impl Renderer {
    /// Whole-sensor barrier. CFA inference is memoized at Denoise; RGB fallback
    /// inference is memoized as the Demosaic tail.
    /// Partial cache residency is not enough: recompute the whole image rather
    /// than mixing inferred tiles with independently padded model patches.
    pub(super) fn denoised_demosaic(
        &self,
        r: &Resolved<'_>,
        cancel: &CancellationToken,
    ) -> EngineResult<pipeline_cpu::Image> {
        let blank = |channels| {
            pipeline_cpu::Image::new(
                r.sensor.width,
                r.sensor.height,
                vec![vec![0.0; r.sensor.area() as usize]; channels],
            )
        };
        let mut out = blank(3)?;
        let cacheable = self.config.graph.node(StageId::Demosaic).cacheable;
        let key = |c| PipelineGraph::memo_key(r.image.id(), &r.chain, StageId::Demosaic, c);
        let mut complete = cacheable;
        if cacheable {
            for c in out.coords() {
                cancel.check()?;
                if let Some(t) = self.cache.get(&key(c)) {
                    out.put(&to_f32(&t)?)?;
                } else {
                    complete = false;
                    break;
                }
            }
        }
        if complete {
            return Ok(out);
        }
        let raw = pipeline_cpu::Image::from_pyramid(r.image.cfa().pyramid())?;
        let mut linear = blank(1)?;
        for c in raw.coords() {
            cancel.check()?;
            linear.put(&self.ops.run(
                StageId::Linearize,
                &Op::Highlights {
                    cfa: r.cfa,
                    mode: r.highlights,
                },
                raw.tile(c, r.lin_halo, r.period)?,
            )?)?;
        }
        drop(raw);
        let raw_selected = pipeline_cpu::cfa_denoise_selected(&r.settings.denoise)
            && matches!(r.cfa, CfaLayout::Bayer(_));
        if raw_selected {
            let cache_raw = self.config.graph.node(StageId::Denoise).cacheable;
            // Full-strength inference has its own identity, independent of Amount.
            let mut full_settings = r.settings.clone();
            full_settings.denoise.amount = 100.0;
            let full_chain = self.stage_chain(&full_settings);
            let raw_key = |c| {
                let mut key =
                    PipelineGraph::memo_key(r.image.id(), &full_chain, StageId::Denoise, c);
                key.params_hash = ParamHash::chain(
                    key.params_hash,
                    ParamHash::of(StageId::Denoise, &"cpu-fullstrength-cfa-v1"),
                );
                key
            };
            let mut cached = blank(1)?;
            let mut complete = cache_raw;
            if cache_raw {
                for c in cached.coords() {
                    cancel.check()?;
                    if let Some(t) = self.cache.get(&raw_key(c)) {
                        cached.put(&to_f32(&t)?)?;
                    } else {
                        complete = false;
                        break;
                    }
                }
            }
            let restored = if complete {
                cached
            } else {
                let restored = if self.cfa_supported(r.cfa, r.settings) {
                    self.full_cfa(r, cancel)?.blend_cpu(&linear, 1.0)?
                } else {
                    pipeline_cpu::raw_denoise(
                        linear.clone(),
                        r.cfa,
                        &full_settings.denoise,
                        self.denoiser.as_deref(),
                    )?
                };
                if cache_raw {
                    for c in restored.coords() {
                        cancel.check()?;
                        // Keep CFA in f32: no first-render/cache-hit rounding mismatch.
                        self.cache.insert(raw_key(c), restored.tile(c, 0, 1)?);
                    }
                }
                restored
            };
            let alpha = r.settings.denoise.amount / 100.0;
            linear = pipeline_cpu::Image::new(
                r.sensor.width,
                r.sensor.height,
                vec![
                    linear.planes()[0]
                        .iter()
                        .zip(&restored.planes()[0])
                        .map(|(&a, &b)| {
                            if alpha == 0.0 {
                                a
                            } else if alpha == 1.0 {
                                b
                            } else {
                                a * (1.0 - alpha) + b * alpha
                            }
                        })
                        .collect(),
                ],
            )?;
        }
        for c in linear.coords() {
            cancel.check()?;
            out.put(&self.ops.run(
                StageId::Demosaic,
                &Op::Demosaic {
                    cfa: r.cfa,
                    algorithm: r.algorithm,
                },
                linear.tile(c, r.dem_halo, r.period)?,
            )?)?;
        }
        drop(linear);
        cancel.check()?;
        let camera_xyz = WorkingSpace::LinearRec2020.to_xyz() * r.profile;
        if !raw_selected {
            out = pipeline_cpu::post_demosaic_denoise(
                out,
                camera_xyz,
                &r.settings.denoise,
                self.denoiser.as_deref(),
            )?;
        }
        cancel.check()?;
        if cacheable {
            for c in out.coords() {
                cancel.check()?;
                self.cache.insert(key(c), to_f16(&out.tile(c, 0, 1)?)?);
            }
        }
        Ok(out)
    }
}
