use super::*;
use crate::{
    cfa::PackedCfa,
    resident::{ResidentBatch, ResidentTile},
};

impl Renderer {
    pub(super) fn cfa_supported(&self, cfa: CfaLayout, settings: &DevelopSettings) -> bool {
        pipeline_cpu::cfa_denoise_selected(&settings.denoise)
            && crate::cfa::bayer_turns(cfa).is_some()
            && self
                .cfa_denoiser
                .as_ref()
                .is_some_and(|d| d.supports(cfa, &settings.denoise))
    }
    fn cfa_full_key(&self, r: &Resolved<'_>) -> engine_api::stage::MemoKey {
        let mut settings = r.settings.clone();
        settings.denoise.amount = 100.0;
        let mut k = PipelineGraph::memo_key(
            r.image.id(),
            &self.stage_chain(&settings),
            StageId::Denoise,
            TileCoord::new(0, 0, 0),
        );
        k.params_hash = ParamHash::chain(
            k.params_hash,
            ParamHash::of(
                StageId::Denoise,
                &(
                    "packed-cfa-full-v1",
                    r.sensor.width,
                    r.sensor.height,
                    crate::cfa::bayer_turns(r.cfa),
                ),
            ),
        );
        k
    }
    /// One latest full sensor inference per renderer, shared by snapshots.
    /// Budget is separate from tile storage and capped at the configured cache
    /// budget. Hold the mutex through inference to coalesce concurrent requests.
    pub(super) fn full_cfa(
        &self,
        r: &Resolved<'_>,
        cancel: &CancellationToken,
    ) -> EngineResult<Arc<PackedCfa>> {
        cancel.check()?;
        if let Some(full) = r.cfa_full.get() {
            return Ok(full.clone());
        }
        let key = self.cfa_full_key(r);
        let mut memo = self
            .cfa_memo
            .lock()
            .map_err(|_| EngineError::internal("CFA memo poisoned"))?;
        if let Some((k, output)) = &*memo
            && *k == key
        {
            let _ = r.cfa_full.set(output.clone());
            return Ok(output.clone());
        }
        let raw = pipeline_cpu::Image::from_pyramid(r.image.cfa().pyramid())?;
        let mut linear = pipeline_cpu::Image::new(
            r.sensor.width,
            r.sensor.height,
            vec![vec![0.0; r.sensor.area() as usize]],
        )?;
        // Runtime currently accepts host tensors. Reconstruct its input on CPU
        // once, avoiding resident pixel readbacks and per-tile model padding.
        for c in raw.coords() {
            cancel.check()?;
            linear.put(&CpuStageOp.run(
                StageId::Linearize,
                &Op::Highlights {
                    cfa: r.cfa,
                    mode: r.highlights,
                },
                raw.tile(c, r.lin_halo, r.period)?,
            )?)?;
        }
        let backend = self
            .cfa_denoiser
            .as_ref()
            .ok_or_else(|| EngineError::internal("missing CFA capability"))?;
        let full = Arc::new(backend.infer_keyed(&linear, r.cfa, &r.settings.denoise, key)?);
        if full.frame() != r.sensor || Some(full.turns()) != crate::cfa::bayer_turns(r.cfa) {
            return Err(EngineError::invalid(
                "CFA inference",
                "backend changed sensor layout/rotation",
            ));
        }
        cancel.check()?;
        let _ = r.cfa_full.set(full.clone());
        if full.bytes() <= self.config.cache_budget_bytes {
            *memo = Some((key, full.clone()));
        }
        Ok(full)
    }
    pub(super) fn resident_cfa(
        &self,
        r: &Resolved<'_>,
        batch: &mut dyn ResidentBatch,
        input: &ResidentTile,
        origin: (u32, u32),
        cancel: &CancellationToken,
    ) -> EngineResult<ResidentTile> {
        if !pipeline_cpu::denoise_active(&r.settings.denoise) {
            return Ok(input.clone());
        }
        cancel.check()?;
        let mut key = self.cfa_full_key(r);
        key.tile = input.coord;
        key.params_hash = ParamHash::chain(
            key.params_hash,
            ParamHash::of(
                StageId::Denoise,
                &(
                    "resident-cfa-page-v1",
                    origin,
                    input.layout.extent.width,
                    input.layout.extent.height,
                    input.layout.halo,
                ),
            ),
        );
        let restored = if let Some(t) = batch.cached(&key)? {
            t
        } else {
            let full = self.full_cfa(r, cancel)?;
            let t = batch.upload_cfa(&full, input.coord, input.layout, origin)?;
            batch.cache_exact(key, &t)?
        };
        cancel.check()?;
        batch.blend_cfa(input, &restored, r.settings.denoise.amount / 100.0)
    }
}
