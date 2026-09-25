use super::*;

impl Renderer {
    /// Whole-sensor barrier, memoizing the RGB inference result as Demosaic.
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
        out = pipeline_cpu::post_demosaic_denoise(
            out,
            camera_xyz,
            &r.settings.denoise,
            self.denoiser.as_deref(),
        )?;
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
