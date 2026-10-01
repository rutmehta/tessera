use super::*;
use crate::resident::{
    DisplayHistogram, LocalToneOptions, ResidentBatch, ResidentOutput, SurfaceTarget,
};
use engine_api::recipe::settings::ToneSettings;

/// Whole-level WB padding: the largest `pipeline_cpu::detail_halo`.
const LEVEL_PAD: u16 = pipeline_cpu::DETAIL_HALO;

fn metrics_output_key(r: &Resolved<'_>) -> engine_api::stage::MemoKey {
    let mut k = PipelineGraph::memo_key(
        r.image.id(),
        &r.chain,
        StageId::Output,
        TileCoord::new(0, 0, 0),
    );
    let frame = r.image.level_extent(0);
    k.params_hash = ParamHash::chain(
        k.params_hash,
        ParamHash::of(
            StageId::Output,
            &("critic-output-v1", frame.width, frame.height),
        ),
    );
    k
}

pub(super) fn has_presence(s: &ToneSettings) -> bool {
    s.texture != 0.0 || s.clarity != 0.0 || s.dehaze != 0.0
}

/// Dehaze statistics depend on everything upstream of Dehaze, never on the
/// Dehaze amount or the curves applied after it.
fn dehaze_statistics_key(r: &Resolved<'_>, level: u8) -> engine_api::stage::MemoKey {
    let upstream = ToneSettings {
        dehaze: 0.0,
        curves: Default::default(),
        ..r.settings.tone.clone()
    };
    let frame = r.image.level_extent(level);
    let mut key = PipelineGraph::memo_key(
        r.image.id(),
        &r.chain,
        StageId::Detail,
        TileCoord::new(level, 0, 0),
    );
    key.stage = StageId::Tone;
    key.params_hash = ParamHash::chain(
        key.params_hash,
        ParamHash::of(
            StageId::Tone,
            &(&upstream, frame.width, frame.height, "dehaze-statistics-v1"),
        ),
    );
    key
}

/// The in-frame rows (of `n`) that a gather of `rows` with `halo` reads,
/// folding out-of-frame positions to the same `period` phase.
fn fold_rows(rows: std::ops::Range<u32>, halo: u16, period: u32, n: u32) -> std::ops::Range<u32> {
    let halo = i64::from(halo);
    let (mut first, mut last) = (u32::MAX, 0);
    for v in i64::from(rows.start) - halo..i64::from(rows.end) + halo {
        let y = crate::resample::clamp_phase(v, n, period);
        first = first.min(y);
        last = last.max(y);
    }
    first..last + 1
}

/// Gather halo for lateral CA: the largest displacement plus bilinear support.
fn ca_halo(plan: &pipeline_cpu::CaPlan, sensor: Extent) -> EngineResult<u16> {
    let max = plan
        .max_displacement(sensor.width, sensor.height)
        .ok_or_else(|| {
            engine_api::EngineError::invalid("lateral CA", "noninvertible channel map")
        })?;
    let halo = max.ceil() + 2.;
    if halo.is_nan() || halo > f64::from(engine_api::tile::MAX_HALO) {
        return Err(engine_api::EngineError::Unsupported {
            what: "lateral CA displacement exceeds the resident halo".into(),
        });
    }
    Ok(halo as u16)
}

impl Renderer {
    /// Reduce full-resolution, quantized SDR output on the resident backend.
    /// None means unsupported: callers must measure full-resolution CPU output,
    /// never substitute a pyramid preview. Reuses the render's memoized stages.
    pub fn render_output_metrics(
        &self,
        image: &RawImage,
        settings: &DevelopSettings,
        cancel: &CancellationToken,
    ) -> EngineResult<Option<crate::resident::OutputMetrics>> {
        if image.camera_linear_proxy().is_some() {
            cancel.check()?;
            self.validate_camera_linear_proxy(image, settings)?;
            return Ok(None);
        }
        cancel.check()?;
        self.validate_settings(settings)?;
        let lens = self.interactive_lens_plan(image, settings, cancel)?;
        if lens.is_none() && !crate::resident_export_lens_supported(&settings.lens) {
            return Ok(None);
        }
        let mut r = self.resolve(image, settings)?;
        r.lens = lens.as_ref().filter(|plan| !plan.is_identity());
        r.cache_lens = true;
        if !r.allow_resident || !self.supports_resident(&r, Some(0)) || self.is_adobe() {
            return Ok(None);
        }
        let Some(mut batch) = self.ops.begin_resident() else {
            return Ok(None);
        };
        if !batch.enable_metrics() {
            return Ok(None);
        }
        let extent = Self::lens_output_extent(image, 0, r.lens);
        let coords = Self::tiles_in_extent(extent, 0, PixelRect::full(extent));
        Ok(self
            .run_resident(&r, &coords, RenderOutput::Display, cancel, batch, None)?
            .metrics)
    }
    /// Resident-only output, with explicit capability failure and cancellation.
    /// Export backends can retain float Output samples instead of display U8.
    pub fn render_resident_region(
        &self,
        image: &RawImage,
        settings: &DevelopSettings,
        level: u8,
        rect: PixelRect,
        cancel: &CancellationToken,
    ) -> EngineResult<Option<Vec<Tile>>> {
        if image.camera_linear_proxy().is_some() {
            cancel.check()?;
            self.validate_camera_linear_proxy(image, settings)?;
            return Ok(None);
        }
        if !crate::resident_export_lens_supported(&settings.lens)
            || settings.geometry != Default::default()
        {
            return Ok(None);
        }
        self.render_resident_lens(image, settings, level, rect, None, cancel)
    }

    /// [`Renderer::render_resident_region`] with resolved lens corrections
    /// (and crop/straighten/Transform), in the reference's order: lateral CA
    /// on sensor-frame camera RGB, vignetting after white balance, and the
    /// composed geometry map after Effects, before Output. With a map, `rect`
    /// addresses the mapped output frame ([`Renderer::lens_output_extent`]);
    /// its rows are rendered from the input rows the map reads. Lens-plan
    /// stages are never memoized. The plan must match `settings`.
    pub fn render_resident_lens(
        &self,
        image: &RawImage,
        settings: &DevelopSettings,
        level: u8,
        rect: PixelRect,
        lens: Option<&pipeline_cpu::LensPlan>,
        cancel: &CancellationToken,
    ) -> EngineResult<Option<Vec<Tile>>> {
        if image.camera_linear_proxy().is_some() {
            cancel.check()?;
            self.validate_camera_linear_proxy(image, settings)?;
            return Ok(None);
        }
        cancel.check()?;
        self.validate_settings(settings)?;
        let mut r = self.resolve(image, settings)?;
        r.lens = lens.filter(|l| !l.is_identity());
        if level > MAX_LEVEL || !self.supports_resident(&r, Some(level)) || self.is_adobe() {
            return Ok(None);
        }
        let Some(mut batch) = self.ops.begin_resident() else {
            return Ok(None);
        };
        let Some(map) = r.lens.and_then(|l| l.map.as_ref()) else {
            let coords = Self::tiles_for(image, level, rect);
            return Ok(Some(
                self.run_resident(&r, &coords, RenderOutput::Display, cancel, batch, None)?
                    .tiles,
            ));
        };
        let frame = image.level_extent(level);
        let (w, h) = map.output_extent(frame.width, frame.height);
        let out = Extent::new(w, h);
        let rows = rect.y..(u64::from(rect.y) + u64::from(rect.height)).min(u64::from(h)) as u32;
        if rows.is_empty() || !rect.y.is_multiple_of(TILE_SIZE) {
            return Err(engine_api::EngineError::invalid(
                "lens region",
                "tile-aligned rows inside the mapped frame required",
            ));
        }
        let (first, end) = map.source_rows(rows.clone(), frame.width, frame.height);
        let coords = Self::tiles_for(
            image,
            level,
            PixelRect::new(0, first, frame.width, end - first),
        );
        let developed =
            self.develop_tiles(&r, &coords, RenderOutput::SceneLinear, cancel, &mut *batch)?;
        cancel.check()?;
        let developed: HashMap<_, _> = developed.into_iter().map(|t| (t.coord, t)).collect();
        let band_coord = TileCoord::new(level, 0, rows.start / TILE_SIZE);
        let mapped = batch.remap(
            frame,
            &developed,
            (first, end),
            map,
            out,
            rows.clone(),
            band_coord,
        )?;
        drop(developed);
        let mapped = match RenderOutput::Display.display_op(settings.output.gamut_mapping) {
            Some(display) => batch.run(&display, &mapped)?,
            None => mapped,
        };
        let mut tiles = Vec::new();
        for y in rows.start / TILE_SIZE..rows.end.div_ceil(TILE_SIZE) {
            for x in 0..w.div_ceil(TILE_SIZE) {
                let (ox, oy) = (x * TILE_SIZE, y * TILE_SIZE);
                let extent = Extent::new((w - ox).min(TILE_SIZE), (rows.end - oy).min(TILE_SIZE));
                tiles.push(batch.crop(
                    &mapped,
                    TileCoord::new(level, x, y),
                    (ox, oy - rows.start),
                    extent,
                )?);
            }
        }
        drop(mapped);
        Ok(Some(batch.finish(tiles, true, None, cancel)?.tiles))
    }

    /// Export rows `rows` of the output frame at `level` (the mapped frame
    /// with a lens map, [`Renderer::lens_output_extent`]) rendered as
    /// full-width bands, one dispatch per stage instead of one per 256² tile,
    /// read back once as interleaved RGB into `dst` (after the backend's
    /// export resize, if configured). Returns false, without GPU work, when
    /// the backend or recipe needs the tiled path ([`Renderer::render_resident_lens`]):
    /// Texture/Clarity/Dehaze (whole-level barrier), or no band support.
    /// Stages and their arithmetic are those of the tiled path; only the
    /// scheduling granularity differs, so results match it sample for sample
    /// up to GPU reassociation.
    #[allow(clippy::too_many_arguments)]
    pub fn render_export_rows(
        &self,
        image: &RawImage,
        settings: &DevelopSettings,
        level: u8,
        rows: std::ops::Range<u32>,
        lens: Option<&pipeline_cpu::LensPlan>,
        dst: &mut [f32],
        cancel: &CancellationToken,
    ) -> EngineResult<bool> {
        if image.camera_linear_proxy().is_some() {
            cancel.check()?;
            self.validate_camera_linear_proxy(image, settings)?;
            return Ok(false);
        }
        cancel.check()?;
        self.validate_settings(settings)?;
        let mut r = self.resolve(image, settings)?;
        r.lens = lens.filter(|l| !l.is_identity());
        if image.rgb().is_some()
            || level > MAX_LEVEL
            || has_presence(&settings.tone)
            || !self.supports_resident(&r, Some(level))
            || self.is_adobe()
        {
            return Ok(false);
        }
        let Some(mut batch) = self.ops.begin_resident() else {
            return Ok(false);
        };
        if !batch.supports_bands() {
            return Ok(false);
        }
        let frame = image.level_extent(level);
        let band = match r.lens.and_then(|l| l.map.as_ref()) {
            Some(map) => {
                let (w, h) = map.output_extent(frame.width, frame.height);
                if rows.is_empty() || rows.end > h {
                    return Err(engine_api::EngineError::invalid(
                        "export rows",
                        "outside the mapped frame",
                    ));
                }
                let (first, end) = map.source_rows(rows.clone(), frame.width, frame.height);
                let developed = self.develop_band(
                    &r,
                    level,
                    first..end,
                    RenderOutput::SceneLinear,
                    cancel,
                    &mut *batch,
                )?;
                let mapped = batch.remap_rows(
                    frame,
                    &developed,
                    (first, end),
                    map,
                    Extent::new(w, h),
                    rows.clone(),
                )?;
                drop(developed);
                match RenderOutput::Display.display_op(settings.output.gamut_mapping) {
                    Some(display) => batch.run_at(&display, &mapped, (0, rows.start))?,
                    None => mapped,
                }
            }
            None => {
                if rows.is_empty() || rows.end > frame.height {
                    return Err(engine_api::EngineError::invalid(
                        "export rows",
                        "outside the level frame",
                    ));
                }
                self.develop_band(&r, level, rows, RenderOutput::Display, cancel, &mut *batch)?
            }
        };
        cancel.check()?;
        batch.finish_rows(band, dst, cancel)?;
        Ok(true)
    }

    /// Full-width rows `rows` of the level frame, developed through
    /// `output`'s display op (Tone/Color/Effects included), as one band. The
    /// sensor stage runs on the sensor rows the band needs: each stage's
    /// input is the previous stage's band gathered once with its halo.
    fn develop_band(
        &self,
        r: &Resolved<'_>,
        level: u8,
        rows: std::ops::Range<u32>,
        output: RenderOutput,
        cancel: &CancellationToken,
        batch: &mut dyn ResidentBatch,
    ) -> EngineResult<crate::resident::ResidentTile> {
        let frame = r.image.level_extent(level);
        let sensor = r.sensor;
        let detail_halo = pipeline_cpu::detail_halo(&r.settings.detail);
        // Rows each stage produces, from the output back to the sensor.
        let balanced = fold_rows(rows.clone(), detail_halo, 1, frame.height);
        let [_, top, _, h] = r.crop;
        let scale = 1u32 << level;
        let resampled = top + balanced.start * scale..top + (balanced.end * scale).min(h);
        let ca = r.lens.and_then(|l| l.ca.as_ref());
        let ca_halo = match ca {
            Some(plan) => ca_halo(plan, sensor)?,
            None => 0,
        };
        let demosaiced = fold_rows(resampled.clone(), ca_halo, 1, sensor.height);
        let linear = fold_rows(demosaiced.clone(), r.dem_halo, r.period, sensor.height);
        let raw = fold_rows(linear.clone(), r.lin_halo, r.period, sensor.height);
        cancel.check()?;
        let pixels = r.image.cfa().pyramid().pixels();
        let t = batch.upload_rows(pixels, sensor.width, raw.clone())?;
        let t = batch.gather_rows(sensor, &t, raw.start, linear.clone(), r.lin_halo, r.period)?;
        let t = batch.run_at(
            &Op::Highlights {
                cfa: r.cfa,
                mode: r.highlights,
            },
            &t,
            (0, linear.start),
        )?;
        let t = self.resident_cfa(r, batch, &t, (0, linear.start), cancel)?;
        let t = batch.gather_rows(
            sensor,
            &t,
            linear.start,
            demosaiced.clone(),
            r.dem_halo,
            r.period,
        )?;
        let t = batch.run_at(
            &Op::Demosaic {
                cfa: r.cfa,
                algorithm: r.algorithm,
            },
            &t,
            (0, demosaiced.start),
        )?;
        cancel.check()?;
        let (t, t_row) = match ca {
            Some(plan) => {
                let t = batch.gather_rows(
                    sensor,
                    &t,
                    demosaiced.start,
                    resampled.clone(),
                    ca_halo,
                    1,
                )?;
                (
                    batch.lateral_ca_at(&t, (0, resampled.start), sensor, plan)?,
                    resampled.start,
                )
            }
            None => (t, demosaiced.start),
        };
        let t = batch.resample_rows(r.crop, level, balanced.clone(), &t, t_row)?;
        let origin = (0, balanced.start);
        let t = batch.run_at(&Op::Matrix(r.profile), &t, origin)?;
        let t = batch.run_at(&Op::Matrix(r.wb), &t, origin)?;
        let t = match r.lens.and_then(|l| l.vignette.as_ref()) {
            Some(plan) => batch.lens_gain_at(&t, origin, frame, plan)?,
            None => t,
        };
        cancel.check()?;
        let t = if detail_halo > 0 {
            let t = batch.gather_rows(frame, &t, balanced.start, rows.clone(), detail_halo, 1)?;
            batch.run_at(&Op::Detail(&r.settings.detail), &t, (0, rows.start))?
        } else {
            // Disabled Detail still validates all controls (see develop_tiles).
            batch.run_at(&Op::Detail(&r.settings.detail), &t, (0, rows.start))?
        };
        let pre_curve = r.settings.color_before_curves();
        let post_curve = r.settings.color_after_curves();
        let mut chain = vec![Op::Tone(&r.settings.tone)];
        if pre_curve.monochrome.as_ref().is_some_and(|m| m.enabled) {
            chain.push(Op::Color(&pre_curve));
        }
        chain.push(Op::ToneExtra(&r.settings.tone));
        chain.extend([
            Op::Color(&post_curve),
            Op::EffectsInCrop(&r.settings.effects, frame, &r.settings.geometry.crop),
        ]);
        if let Some(display) = output.display_op(r.settings.output.gamut_mapping) {
            chain.push(display);
        }
        batch.run_chain_at(&chain, &t, (0, rows.start))
    }

    /// Output extent at `level` of a lens plan's composed map (the active
    /// area when the plan has no map).
    pub fn lens_output_extent(
        image: &RawImage,
        level: u8,
        lens: Option<&pipeline_cpu::LensPlan>,
    ) -> Extent {
        let frame = image.level_extent(level);
        match lens.and_then(|l| l.map.as_ref()) {
            Some(map) => {
                let (w, h) = map.output_extent(frame.width, frame.height);
                Extent::new(w, h)
            }
            None => frame,
        }
    }

    /// Whether this backend can keep demosaic and subsequent stages resident.
    /// CFA inference initially consumes a host tensor and uploads its packed
    /// result once; warm tone edits reuse the resident stages. This query does
    /// not load a model and is not a frame-time guarantee.
    pub fn can_render_resident(
        &self,
        image: &RawImage,
        settings: &DevelopSettings,
    ) -> EngineResult<bool> {
        if image.camera_linear_proxy().is_some() {
            return self.camera_linear_resident_supported(image, settings);
        }
        self.validate_settings(settings)?;
        let lens = self.interactive_lens_plan(image, settings, &CancellationToken::new())?;
        if lens.is_none() && !crate::resident_export_lens_supported(&settings.lens) {
            return Ok(false);
        }
        let mut r = self.resolve(image, settings)?;
        r.lens = lens.as_ref();
        r.cache_lens = true;
        Ok(self.supports_resident(&r, None))
    }

    /// `level`: the level to render, when known. Texture/Clarity/Dehaze need a
    /// backend whole-level barrier that fits that level (any level when None).
    pub(super) fn supports_resident(&self, r: &Resolved<'_>, level: Option<u8>) -> bool {
        if r.image.camera_linear_proxy().is_some() && level != Some(0) {
            return false;
        }
        let s = r.settings;
        if s.tone.legacy_pv2010.is_some() {
            return false;
        }
        if (r.image.rgb().is_none() && pipeline_cpu::denoise_active(&s.denoise) && !self.cfa_supported(r.cfa, s))
            || (r.image.rgb().is_none() && !matches!(r.cfa, CfaLayout::Bayer(_) | CfaLayout::XTrans(_)))
            // Local adjustment operators/rasterization use the whole-image
            // nonresident path until all local kernels are resident-capable.
            || !s.locals.adjustments.is_empty()
            // Point Color uses the CPU color stage in the nonresident chain.
            || !s.color.point_colors.is_empty()
            || s.effects.lens_blur.is_some()
            || self.depth_visualisation
            // Manual legacy CA cannot use an unplanned resident prefix: it must
            // run after demosaic and before the camera/white-balance matrix.
            || ((s.lens.legacy_ca_red.unwrap_or(0.) != 0.
                || s.lens.legacy_ca_blue.unwrap_or(0.) != 0.) && r.lens.is_none())
            // Geometry is resident only through an export lens plan's map.
            || (s.geometry != Default::default()
                && r.lens.is_none())
        {
            return false;
        }
        let Some(batch) = self.ops.begin_resident() else {
            return false;
        };
        if r.image.rgb().is_none()
            && pipeline_cpu::denoise_active(&s.denoise)
            && !batch.supports_cfa()
        {
            return false;
        }
        if !has_presence(&s.tone) {
            return true;
        }
        let frame = r.image.level_extent(level.unwrap_or(0).min(MAX_LEVEL));
        // Without a known level, probe the smallest frame: capability only.
        let frame = if level.is_some() {
            frame
        } else {
            Extent::new(1, 1)
        };
        batch.supports_local_tone(frame)
    }
    /// Direct display render to an RGBA8 IOSurface. Returns false if the
    /// backend/settings require the conventional CPU tile delivery path.
    pub fn render_to_surface(
        &self,
        image: &RawImage,
        settings: &DevelopSettings,
        level: u8,
        surface: u32,
        cancel: &CancellationToken,
    ) -> EngineResult<bool> {
        if image.camera_linear_proxy().is_some() {
            cancel.check()?;
            self.validate_camera_linear_proxy(image, settings)?;
            let extent = Self::output_extent(image, settings, level)?;
            let coords = Self::tiles_in_extent(extent, level, PixelRect::full(extent));
            return self
                .try_camera_linear_resident(
                    image,
                    settings,
                    &coords,
                    RenderOutput::Display,
                    cancel,
                    Some(SurfaceTarget {
                        id: surface,
                        histogram: false,
                    }),
                )
                .map(|result| result.is_some());
        }
        cancel.check()?;
        let r = self.resolve(image, settings)?;
        if level > MAX_LEVEL || !self.supports_resident(&r, Some(level)) {
            return Ok(false);
        }
        let Some(batch) = self.ops.begin_resident() else {
            return Ok(false);
        };
        let coords = Self::tiles_for(image, level, PixelRect::full(image.level_extent(level)));
        self.run_resident(
            &r,
            &coords,
            RenderOutput::Display,
            cancel,
            batch,
            Some(SurfaceTarget {
                id: surface,
                histogram: false,
            }),
        )?;
        Ok(true)
    }

    pub(super) fn resolve_interactive_lens(
        &self,
        image: &RawImage,
        settings: &DevelopSettings,
    ) -> EngineResult<pipeline_cpu::ResolvedLens> {
        let key = ParamHash::of(
            StageId::Lens,
            &(&settings.lens, &settings.demosaic, &settings.linearize),
        );
        let cached = self
            .lens_resolution
            .lock()
            .map_err(|_| engine_api::EngineError::internal("lens resolution cache poisoned"))?
            .clone();
        if let Some((id, hash, lens)) = cached
            && id == image.id()
            && hash == key
        {
            return Ok(lens);
        }
        let lens = if let Some(rgb) = image.rgb() {
            pipeline_cpu::resolve_lens(rgb.pixels(), &settings.lens, None, &Default::default())?
        } else {
            pipeline_cpu::resolve_lens_sensor(
                image.cfa().pyramid().pixels(),
                image.metadata(),
                settings,
                &Default::default(),
            )?
        };
        *self
            .lens_resolution
            .lock()
            .map_err(|_| engine_api::EngineError::internal("lens resolution cache poisoned"))? =
            Some((image.id(), key, lens.clone()));
        Ok(lens)
    }

    /// Resolve the same composed lens map used by resident export.
    pub(super) fn interactive_lens_plan(
        &self,
        image: &RawImage,
        settings: &DevelopSettings,
        cancel: &CancellationToken,
    ) -> EngineResult<Option<pipeline_cpu::LensPlan>> {
        cancel.check()?;
        if crate::resident_export_lens_supported(&settings.lens)
            && settings.geometry == Default::default()
        {
            return Ok(None);
        }
        let resolved = self.resolve_interactive_lens(image, settings)?;
        if let Some(analyzed) = self.interactive_upright_analysis(image, settings, cancel)? {
            return resolved.plan_with_upright(settings, image.metadata(), &analyzed);
        }
        resolved.plan(settings, image.metadata())
    }

    /// Cache L0 Upright independently of whether the optics have a resident plan.
    pub(super) fn interactive_upright_analysis(
        &self,
        image: &RawImage,
        settings: &DevelopSettings,
        cancel: &CancellationToken,
    ) -> EngineResult<Option<pipeline_cpu::UprightAnalysis>> {
        cancel.check()?;
        use engine_api::recipe::settings::UprightMode;
        if settings.geometry.upright.has_saved_solution() {
            return Ok(None);
        }
        if !matches!(
            settings.geometry.upright.mode,
            UprightMode::Off | UprightMode::Guided
        ) {
            let mut analysis_settings = settings.clone();
            // Manual geometry is downstream of the image used to detect lines.
            analysis_settings.geometry.transform = Default::default();
            let key = ParamHash::of(
                StageId::Geometry,
                &(
                    self.stage_chain(&analysis_settings)[StageId::Effects.index()].1,
                    self.depth_visualisation,
                    self.depth.as_ref().map(|p| Arc::as_ptr(p) as usize),
                    &analysis_settings,
                ),
            );
            let cached = *self.geometry_analysis.lock().map_err(|_| {
                engine_api::EngineError::internal("geometry analysis cache poisoned")
            })?;
            if let Some((id, hash, analyzed)) = cached
                && id == image.id()
                && hash == key
            {
                return Ok(Some(analyzed));
            }
            let analysis = self.develop_before_geometry(image, settings, 0, cancel)?;
            let resolved = self.resolve_interactive_lens(image, settings)?;
            let analyzed = resolved.analyze_upright(settings, &analysis)?;
            cancel.check()?;
            *self.geometry_analysis.lock().map_err(|_| {
                engine_api::EngineError::internal("geometry analysis cache poisoned")
            })? = Some((image.id(), key, analyzed));
            return Ok(Some(analyzed));
        }
        Ok(None)
    }

    /// Direct surface presentation with only a 4 KiB histogram readback.
    pub fn render_surface(
        &self,
        image: &RawImage,
        settings: &DevelopSettings,
        level: u8,
        surface: u32,
        cancel: &CancellationToken,
    ) -> EngineResult<Option<DisplayHistogram>> {
        self.render_surface_as(
            image,
            settings,
            level,
            surface,
            RenderOutput::Display,
            cancel,
        )
    }

    /// [`Renderer::render_surface`] with a choice of display output: an
    /// RGBA8 surface takes [`RenderOutput::Display`], an RGBA16F (EDR)
    /// surface takes [`RenderOutput::DisplayLinear`]. The histogram is of
    /// display-encoded sRGB either way (EDR values clip into the top bin).
    pub fn render_surface_as(
        &self,
        image: &RawImage,
        settings: &DevelopSettings,
        level: u8,
        surface: u32,
        output: RenderOutput,
        cancel: &CancellationToken,
    ) -> EngineResult<Option<DisplayHistogram>> {
        if image.camera_linear_proxy().is_some() {
            cancel.check()?;
            self.validate_camera_linear_proxy(image, settings)?;
            if output == RenderOutput::SceneLinear {
                return Err(EngineError::invalid("IOSurface", "display output required"));
            }
            let extent = Self::output_extent(image, settings, level)?;
            let coords = Self::tiles_in_extent(extent, level, PixelRect::full(extent));
            return self
                .try_camera_linear_resident(
                    image,
                    settings,
                    &coords,
                    output,
                    cancel,
                    Some(SurfaceTarget {
                        id: surface,
                        histogram: true,
                    }),
                )?
                .map(|result| {
                    result
                        .histogram
                        .ok_or_else(|| EngineError::internal("resident histogram missing"))
                })
                .transpose();
        }
        if output == RenderOutput::SceneLinear {
            return Err(engine_api::EngineError::invalid(
                "IOSurface",
                "display output required",
            ));
        }
        cancel.check()?;
        let lens = self.interactive_lens_plan(image, settings, cancel)?;
        if lens.is_none() && !crate::resident_export_lens_supported(&settings.lens) {
            return Ok(None);
        }
        let mut r = self.resolve(image, settings)?;
        r.lens = lens.as_ref();
        r.cache_lens = true;
        if level > MAX_LEVEL || !self.supports_resident(&r, Some(level)) {
            return Ok(None);
        }
        let Some(batch) = self.ops.begin_resident() else {
            return Ok(None);
        };
        let extent = Self::output_extent(image, settings, level)?;
        let coords = Self::tiles_in_extent(extent, level, PixelRect::full(extent));
        self.run_resident(
            &r,
            &coords,
            output,
            cancel,
            batch,
            Some(SurfaceTarget {
                id: surface,
                histogram: true,
            }),
        )
        .and_then(|out| {
            out.histogram
                .map(Some)
                .ok_or_else(|| engine_api::EngineError::internal("resident histogram missing"))
        })
    }

    /// Scene-linear WB tiles of the output level: memoized WB, the extra
    /// output-level demosaic checkpoint, or the streamed sensor chain.
    fn balanced_tiles(
        &self,
        r: &Resolved<'_>,
        needed: impl IntoIterator<Item = TileCoord>,
        batch: &mut dyn ResidentBatch,
        cancel: &CancellationToken,
    ) -> EngineResult<HashMap<TileCoord, crate::resident::ResidentTile>> {
        let key = |stage, c| PipelineGraph::memo_key(r.image.id(), &r.chain, stage, c);
        let cache = |batch: &mut dyn ResidentBatch, key, tile: &crate::resident::ResidentTile| {
            batch.cache_exact(key, tile)
        };
        // Explicit export plans can come from external calibrations. Interactive
        // plans use the recipe Lens hash; CA-corrected demosaic checkpoints
        // still precede that hash and remain uncached.
        let cache_dem = self.config.graph.node(StageId::Demosaic).cacheable
            && r.lens
                .is_none_or(|p| p.ca.is_none() && p.vignette.is_none());
        let cache_wb = self.config.graph.node(StageId::WhiteBalance).cacheable
            && (r.cache_lens
                || r.lens
                    .is_none_or(|p| p.ca.is_none() && p.vignette.is_none()));
        let ca = r.lens.and_then(|l| l.ca.as_ref());
        let ca_halo = match ca {
            Some(plan) => ca_halo(plan, r.sensor)?,
            None => 0,
        };
        let mut balanced = HashMap::new();
        for c in needed {
            cancel.check()?;
            if let Some(proxy) = r.image.camera_linear_proxy() {
                if c.level != 0 {
                    return Err(EngineError::internal("camera-linear resident requires L0"));
                }
                let t = if let Some(t) = batch.cached(&key(StageId::WhiteBalance, c))? {
                    t
                } else {
                    let source_key = key(StageId::Demosaic, c);
                    let camera = if let Some(t) = batch.cached(&source_key)? {
                        t
                    } else {
                        let uploaded = batch.upload(&proxy.pixels().tile(c, 0, 1)?)?;
                        // upload_cached/cache would add an f16 conversion for this
                        // stage, invalidating signed HDR / codec F32 fallback.
                        batch.cache_exact(source_key, &uploaded)?
                    };
                    let t = batch.run(&Op::Matrix(r.profile), &camera)?;
                    let t = batch.run(&Op::Matrix(r.wb), &t)?;
                    let t = match r.lens.and_then(|l| l.vignette.as_ref()) {
                        Some(plan) => batch.lens_gain(&t, r.image.active_extent(), plan)?,
                        None => t,
                    };
                    batch.cache_exact(key(StageId::WhiteBalance, c), &t)?
                };
                balanced.insert(c, t);
                continue;
            }
            let t = if cache_wb && let Some(t) = batch.cached(&key(StageId::WhiteBalance, c))? {
                t
            } else {
                // Domain-separate output-frame demosaic keys from sensor-frame
                // level-zero tiles, including a crop offset at level zero.
                let mut level_key = key(StageId::Demosaic, c);
                level_key.params_hash = ParamHash::chain(
                    level_key.params_hash,
                    ParamHash::of(StageId::Demosaic, &("output-demosaic-v1", r.crop)),
                );
                let t = if cache_dem && let Some(t) = batch.cached(&level_key)? {
                    t
                } else {
                    let mut sampled = None;
                    // Stream sensor dependencies; no full-frame f32 map. Each
                    // chunk can reuse earlier scratch buffers in this encoder.
                    for sources in resample_sources(r.crop, c).chunks(16) {
                        cancel.check()?;
                        let mut dem = HashMap::new();
                        let mut missing = Vec::new();
                        // Lateral CA reads demosaiced neighbours of each source.
                        let wanted: BTreeSet<TileCoord> = if ca.is_some() {
                            sources
                                .iter()
                                .flat_map(|&d| gather_sources(r.sensor, d, ca_halo, 1))
                                .collect()
                        } else {
                            sources.iter().copied().collect()
                        };
                        for &d in &wanted {
                            if cache_dem
                                && let Some(t) = batch.cached(&key(StageId::Demosaic, d))?
                            {
                                dem.insert(d, t);
                            } else if let Some(rgb) = r.image.rgb() {
                                let t = batch.upload(&rgb.pixels().tile(d, 0, 1)?)?;
                                let t = batch.cache_exact(key(StageId::Demosaic, d), &t)?;
                                dem.insert(d, t);
                            } else {
                                missing.push(d);
                            }
                        }
                        let need_lin: BTreeSet<_> = missing
                            .iter()
                            .flat_map(|&d| gather_sources(r.sensor, d, r.dem_halo, r.period))
                            .collect();
                        let need_raw: BTreeSet<_> = need_lin
                            .iter()
                            .flat_map(|&l| gather_sources(r.sensor, l, r.lin_halo, r.period))
                            .collect();
                        let mut raw = HashMap::new();
                        for d in need_raw {
                            cancel.check()?;
                            let k = key(StageId::Decode, d);
                            let t = if let Some(t) = batch.cached(&k)? {
                                t
                            } else {
                                let t =
                                    engine_api::tile::Pyramid::tile(r.image.cfa().pyramid(), d)?;
                                batch.upload_cached(k, &t)?
                            };
                            raw.insert(d, t);
                        }
                        let mut linear = HashMap::new();
                        for d in need_lin {
                            cancel.check()?;
                            let t = batch.gather(r.sensor, d, r.lin_halo, r.period, &raw)?;
                            let t = batch.run(
                                &Op::Highlights {
                                    cfa: r.cfa,
                                    mode: r.highlights,
                                },
                                &t,
                            )?;
                            let t =
                                self.resident_cfa(r, batch, &t, d.pixel_origin(TILE_SIZE), cancel)?;
                            linear.insert(d, t);
                        }
                        drop(raw);
                        for d in missing {
                            cancel.check()?;
                            let t = batch.gather(r.sensor, d, r.dem_halo, r.period, &linear)?;
                            let t = batch.run(
                                &Op::Demosaic {
                                    cfa: r.cfa,
                                    algorithm: r.algorithm,
                                },
                                &t,
                            )?;
                            let t = if cache_dem {
                                cache(batch, key(StageId::Demosaic, d), &t)?
                            } else {
                                t
                            };
                            dem.insert(d, t);
                        }
                        drop(linear);
                        if let Some(plan) = ca {
                            let mut corrected = HashMap::with_capacity(sources.len());
                            for &d in sources {
                                cancel.check()?;
                                let t = batch.gather(r.sensor, d, ca_halo, 1, &dem)?;
                                corrected.insert(d, batch.lateral_ca(&t, r.sensor, plan)?);
                            }
                            dem = corrected;
                        }
                        sampled = Some(batch.resample(r.crop, c, &dem, sampled)?);
                        drop(dem);
                        batch.checkpoint(cancel)?;
                    }
                    let t = sampled
                        .ok_or_else(|| engine_api::EngineError::internal("no resample sources"))?;
                    // At level 0 this checkpoint is a crop of the memoized
                    // sensor demosaic: retaining it duplicates a whole frame and
                    // evicted WB/Detail tiles on large images (per-edit re-decode).
                    if cache_dem && c.level > 0 {
                        // This is an extra checkpoint not present in the scalar
                        // graph. Do not add a second f16 rounding before WB.
                        batch.cache_exact(level_key, &t)?
                    } else {
                        t
                    }
                };
                // Both matrices are linear: transform only the requested level.
                let t = batch.run(&Op::Matrix(r.profile), &t)?;
                let t = batch.run(&Op::Matrix(r.wb), &t)?;
                let t = match r.lens.and_then(|l| l.vignette.as_ref()) {
                    Some(plan) => batch.lens_gain(&t, r.image.level_extent(c.level), plan)?,
                    None => t,
                };
                if cache_wb {
                    cache(batch, key(StageId::WhiteBalance, c), &t)?
                } else {
                    t
                }
            };
            balanced.insert(c, t);
        }
        Ok(balanced)
    }

    /// Shared resident tail, including geometry and optional display conversion.
    /// SceneLinear leaves the result unencoded so proxy pyramid reduction can
    /// occur after all nonlinear development, exactly like the scalar reference.
    fn develop_resident_level(
        &self,
        r: &Resolved<'_>,
        all: &[TileCoord],
        output: RenderOutput,
        cancel: &CancellationToken,
        batch: &mut dyn ResidentBatch,
    ) -> EngineResult<(crate::resident::ResidentTile, Extent)> {
        let level = all[0].level;
        let frame = r.image.level_extent(level);
        let lc = TileCoord::new(level, 0, 0);
        let level_key = |stage, pad: u16| {
            let mut k = PipelineGraph::memo_key(r.image.id(), &r.chain, stage, lc);
            k.params_hash = ParamHash::chain(
                k.params_hash,
                ParamHash::of(
                    stage,
                    &("resident-level-v1", pad, frame.width, frame.height),
                ),
            );
            k
        };
        let cache_wb = self.config.graph.node(StageId::WhiteBalance).cacheable
            && (r.cache_lens
                || r.lens
                    .is_none_or(|p| p.ca.is_none() && p.vignette.is_none()));
        let cache_detail = self.config.graph.node(StageId::Detail).cacheable
            && (r.cache_lens
                || r.lens
                    .is_none_or(|p| p.ca.is_none() && p.vignette.is_none()));
        let detail_key = level_key(StageId::Detail, 0);
        let developed = if cache_detail && let Some(t) = batch.cached(&detail_key)? {
            t
        } else {
            let wb_key = level_key(StageId::WhiteBalance, LEVEL_PAD);
            let padded = if cache_wb && let Some(t) = batch.cached(&wb_key)? {
                t
            } else {
                let tiles = self.balanced_tiles(r, all.iter().copied(), batch, cancel)?;
                cancel.check()?;
                let t = batch.gather_level(frame, lc, LEVEL_PAD, &tiles)?;
                if cache_wb {
                    batch.cache_exact(wb_key, &t)?
                } else {
                    t
                }
            };
            // Detail consumes the padded neighbours and returns the interior.
            let t = batch.run(&Op::Detail(&r.settings.detail), &padded)?;
            if cache_detail {
                batch.cache_exact(detail_key, &t)?
            } else {
                t
            }
        };
        cancel.check()?;
        let curves_only = ToneSettings {
            texture: 0.0,
            clarity: 0.0,
            dehaze: 0.0,
            ..r.settings.tone.clone()
        };
        let mut t = developed;
        let mut chain = if has_presence(&r.settings.tone) {
            let toned = batch.run(&Op::Tone(&r.settings.tone), &t)?;
            let toned = if r
                .settings
                .color
                .monochrome
                .as_ref()
                .is_some_and(|m| m.enabled)
            {
                batch.run(&Op::Color(&r.settings.color_before_curves()), &toned)?
            } else {
                toned
            };
            let options = LocalToneOptions {
                preview: level > 0 && self.config.preview_approximations,
                statistics_key: dehaze_statistics_key(r, level),
            };
            let input = HashMap::from([(lc, toned)]);
            t = batch
                .local_tone(&r.settings.tone, frame, &input, &[lc], &options)?
                .remove(&lc)
                .ok_or_else(|| engine_api::EngineError::internal("local tone output missing"))?;
            vec![Op::ToneExtra(&curves_only)]
        } else {
            vec![Op::Tone(&r.settings.tone), Op::ToneExtra(&r.settings.tone)]
        };
        let pre_curve = r.settings.color_before_curves();
        let post_curve = r.settings.color_after_curves();
        if !has_presence(&r.settings.tone)
            && pre_curve.monochrome.as_ref().is_some_and(|m| m.enabled)
        {
            chain.insert(chain.len() - 1, Op::Color(&pre_curve));
        }
        chain.extend([
            Op::Color(&post_curve),
            Op::EffectsInCrop(&r.settings.effects, frame, &r.settings.geometry.crop),
        ]);
        let map = r.lens.and_then(|p| p.map.as_ref());
        if map.is_none()
            && let Some(display) = output.display_op(r.settings.output.gamut_mapping)
        {
            chain.push(display);
        }
        // Previews develop in their own pixel domain (grain scale), as below.
        t.coord.level = 0;
        let mut t = batch.run_chain(&chain, &t)?;
        t.coord = lc;
        let frame = if let Some(map) = map {
            let (w, h) = map.output_extent(frame.width, frame.height);
            let out = Extent::new(w, h);
            t = batch.remap_rows(frame, &t, (0, frame.height), map, out, 0..h)?;
            if let Some(display) = output.display_op(r.settings.output.gamut_mapping) {
                t = batch.run(&display, &t)?;
            }
            out
        } else {
            frame
        };
        Ok((t, frame))
    }

    /// Whole-level resident develop: one level-sized tile per stage instead
    /// of one dispatch per pyramid tile. The WB level (padded by the largest
    /// Detail halo, edges replicated like `gather`) and the developed level
    /// are memoized, so point-stage edits cost one fused pass plus output.
    #[allow(clippy::too_many_arguments)]
    fn run_resident_level(
        &self,
        r: &Resolved<'_>,
        all: &[TileCoord],
        coords: &[TileCoord],
        output: RenderOutput,
        cancel: &CancellationToken,
        mut batch: Box<dyn ResidentBatch + '_>,
        surface: Option<SurfaceTarget>,
    ) -> EngineResult<ResidentOutput> {
        let level = all[0].level;
        let (mut t, frame) = self.develop_resident_level(r, all, output, cancel, &mut *batch)?;
        if batch.metrics_enabled() && level == 0 && r.lens.is_none() {
            t = batch.cache_exact(metrics_output_key(r), &t)?;
        }
        cancel.check()?;
        let finished = if surface.is_some() || batch.metrics_enabled() {
            vec![t]
        } else {
            coords
                .iter()
                .map(|&c| {
                    let (x, y) = c.pixel_origin(TILE_SIZE);
                    let extent = Extent::new(
                        (frame.width - x).min(TILE_SIZE),
                        (frame.height - y).min(TILE_SIZE),
                    );
                    batch.crop(&t, c, (x, y), extent)
                })
                .collect::<EngineResult<Vec<_>>>()?
        };
        batch.finish(finished, output == RenderOutput::Display, surface, cancel)
    }

    /// Camera-linear coarse previews share the L0 resident tail. Only after
    /// geometry do we box-average linear pixels, then apply display conversion.
    /// No original RAW path calls this function.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn run_camera_linear_coarse(
        &self,
        r: &Resolved<'_>,
        coords: &[TileCoord],
        output: RenderOutput,
        cancel: &CancellationToken,
        mut batch: Box<dyn ResidentBatch + '_>,
        surface: Option<SurfaceTarget>,
    ) -> EngineResult<Option<ResidentOutput>> {
        let input = r.image.level_extent(0);
        if !batch.supports_level(input, LEVEL_PAD) {
            return Ok(None);
        }
        let all = Self::tiles_for(r.image, 0, PixelRect::full(input));
        let (linear, frame) =
            self.develop_resident_level(r, &all, RenderOutput::SceneLinear, cancel, &mut *batch)?;
        cancel.check()?;
        let crop = [0, 0, frame.width, frame.height];
        // Keep the generic ResidentBatch resample contract: regular L0 tiles,
        // including odd edge tiles, rather than a backend-specific giant tile.
        let needed: BTreeSet<_> = coords
            .iter()
            .flat_map(|&c| resample_sources(crop, c))
            .collect();
        let mut sources = HashMap::new();
        for c in needed {
            cancel.check()?;
            let (x, y) = c.pixel_origin(TILE_SIZE);
            let extent = Extent::new(
                (frame.width - x).min(TILE_SIZE),
                (frame.height - y).min(TILE_SIZE),
            );
            sources.insert(c, batch.crop(&linear, c, (x, y), extent)?);
        }
        let mut reduced = Vec::with_capacity(coords.len());
        for &c in coords {
            cancel.check()?;
            let mut tile = batch.resample(crop, c, &sources, None)?;
            if let Some(display) = output.display_op(r.settings.output.gamut_mapping) {
                tile = batch.run(&display, &tile)?;
            }
            reduced.push(tile);
        }
        batch
            .finish(reduced, output == RenderOutput::Display, surface, cancel)
            .map(Some)
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn run_resident(
        &self,
        r: &Resolved<'_>,
        coords: &[TileCoord],
        output: RenderOutput,
        cancel: &CancellationToken,
        mut batch: Box<dyn ResidentBatch + '_>,
        surface: Option<SurfaceTarget>,
    ) -> EngineResult<ResidentOutput> {
        if let Some(first) = coords.first() {
            let frame = r.image.level_extent(first.level);
            let all = Self::tiles_for(r.image, first.level, PixelRect::full(frame));
            // A prior critic reduction retained the encoded output. Face/noise
            // crops read just their tiles from it; repeated metrics dispatch only
            // the reduction. Its key includes the entire recipe chain and frame.
            if first.level == 0
                && output == RenderOutput::Display
                && r.lens.is_none()
                && let Some(t) = batch.cached(&metrics_output_key(r))?
            {
                let finished = if batch.metrics_enabled()
                    || (surface.is_some() && coords.len() == all.len())
                {
                    vec![t]
                } else {
                    coords
                        .iter()
                        .map(|&c| {
                            let (x, y) = c.pixel_origin(TILE_SIZE);
                            batch.crop(
                                &t,
                                c,
                                (x, y),
                                Extent::new(
                                    (frame.width - x).min(TILE_SIZE),
                                    (frame.height - y).min(TILE_SIZE),
                                ),
                            )
                        })
                        .collect::<EngineResult<Vec<_>>>()?
                };
                return batch.finish(finished, true, surface, cancel);
            }
            // `coords` are unique tiles of one level: whole-level requests
            // (every surface frame) run as one level-sized tile.
            if (coords.len() == all.len() || r.lens.and_then(|p| p.map.as_ref()).is_some())
                && batch.supports_level(frame, LEVEL_PAD)
            {
                return self.run_resident_level(r, &all, coords, output, cancel, batch, surface);
            }
        }
        if let Some(map) = r.lens.and_then(|p| p.map.as_ref()) {
            let Some(first) = coords.first() else {
                return batch.finish(Vec::new(), output == RenderOutput::Display, surface, cancel);
            };
            let frame = r.image.level_extent(first.level);
            let (w, h) = map.output_extent(frame.width, frame.height);
            let out = Extent::new(w, h);
            let all = Self::tiles_for(r.image, first.level, PixelRect::full(frame));
            let developed =
                self.develop_tiles(r, &all, RenderOutput::SceneLinear, cancel, &mut *batch)?;
            let tiles = developed.into_iter().map(|t| (t.coord, t)).collect();
            let mut mapped = batch.remap(
                frame,
                &tiles,
                (0, frame.height),
                map,
                out,
                0..h,
                TileCoord::new(first.level, 0, 0),
            )?;
            if let Some(display) = output.display_op(r.settings.output.gamut_mapping) {
                mapped = batch.run(&display, &mapped)?;
            }
            let finished = if surface.is_some() {
                vec![mapped]
            } else {
                coords
                    .iter()
                    .map(|&c| {
                        let (x, y) = c.pixel_origin(TILE_SIZE);
                        batch.crop(
                            &mapped,
                            c,
                            (x, y),
                            Extent::new((w - x).min(TILE_SIZE), (h - y).min(TILE_SIZE)),
                        )
                    })
                    .collect::<EngineResult<Vec<_>>>()?
            };
            return batch.finish(finished, output == RenderOutput::Display, surface, cancel);
        }
        let mut finished = self.develop_tiles(r, coords, output, cancel, &mut *batch)?;
        if batch.metrics_enabled()
            && let Some(first) = coords.first()
        {
            let frame = r.image.level_extent(first.level);
            if batch.supports_output_level(frame) {
                // >16MP operators still run per tile, but the reduction and
                // subsequent face crops use one integer-indexed encoded frame.
                let tiles = finished.into_iter().map(|t| (t.coord, t)).collect();
                let t = batch.gather_level(frame, TileCoord::new(0, 0, 0), 0, &tiles)?;
                let t = batch.cache_exact(metrics_output_key(r), &t)?;
                finished = vec![t];
            }
        }
        batch.finish(finished, output == RenderOutput::Display, surface, cancel)
    }

    /// The resident tile path up to (and including) `output`'s display op:
    /// halo-free developed tiles for `coords`, still on the backend.
    fn develop_tiles(
        &self,
        r: &Resolved<'_>,
        coords: &[TileCoord],
        output: RenderOutput,
        cancel: &CancellationToken,
        batch: &mut dyn ResidentBatch,
    ) -> EngineResult<Vec<crate::resident::ResidentTile>> {
        let key = |stage, c| PipelineGraph::memo_key(r.image.id(), &r.chain, stage, c);
        // Creative curves/color can amplify f16 checkpoint error beyond the
        // full-chain tolerance. Retain f32 checkpoints for every recipe so a
        // later creative edit cannot reuse lower-precision neutral entries.
        // Their full payload still counts against the configured cache budget.
        let cache = |batch: &mut dyn ResidentBatch, key, tile: &crate::resident::ResidentTile| {
            batch.cache_exact(key, tile)
        };
        let cache_detail = self.config.graph.node(StageId::Detail).cacheable
            && (r.cache_lens
                || r.lens
                    .is_none_or(|p| p.ca.is_none() && p.vignette.is_none()));
        let halo = pipeline_cpu::detail_halo(&r.settings.detail);
        let presence = has_presence(&r.settings.tone);
        // Texture/Clarity/Dehaze read neighbourhoods and global statistics of
        // the whole developed level, like the scalar image-level barrier.
        let work: Vec<TileCoord> = match coords.first() {
            Some(first) if presence => Self::tiles_for(
                r.image,
                first.level,
                PixelRect::full(r.image.level_extent(first.level)),
            ),
            _ => coords.to_vec(),
        };
        let mut detailed = HashMap::new();
        let mut needed = BTreeSet::new();
        for &c in &work {
            if halo > 0
                && cache_detail
                && let Some(t) = batch.cached(&key(StageId::Detail, c))?
            {
                detailed.insert(c, t);
            } else if halo > 0 {
                needed.extend(gather_sources(r.image.level_extent(c.level), c, halo, 1));
            } else {
                needed.insert(c);
            }
        }
        let balanced = self.balanced_tiles(r, needed, &mut *batch, cancel)?;
        let mut finished = Vec::with_capacity(coords.len());
        let mut developed = HashMap::new();
        for &c in &work {
            cancel.check()?;
            let t = if let Some(t) = detailed.remove(&c) {
                t
            } else if halo > 0 {
                let t = batch.gather(r.image.level_extent(c.level), c, halo, 1, &balanced)?;
                let t = batch.run(&Op::Detail(&r.settings.detail), &t)?;
                if cache_detail {
                    cache(&mut *batch, key(StageId::Detail, c), &t)?
                } else {
                    t
                }
            } else {
                // Disabled Detail still validates all controls, just as the
                // scalar reference does (zero halo is not a validation bypass).
                batch.run(&Op::Detail(&r.settings.detail), &balanced[&c])?
            };
            developed.insert(c, t);
        }
        drop(balanced);
        let curves_only = ToneSettings {
            texture: 0.0,
            clarity: 0.0,
            dehaze: 0.0,
            ..r.settings.tone.clone()
        };
        if presence && let Some(first) = coords.first() {
            let level = first.level;
            let mut toned = HashMap::with_capacity(developed.len());
            for (c, t) in developed.drain() {
                cancel.check()?;
                let t = batch.run(&Op::Tone(&r.settings.tone), &t)?;
                let t = if r
                    .settings
                    .color
                    .monochrome
                    .as_ref()
                    .is_some_and(|m| m.enabled)
                {
                    batch.run(&Op::Color(&r.settings.color_before_curves()), &t)?
                } else {
                    t
                };
                toned.insert(c, t);
            }
            let options = LocalToneOptions {
                preview: level > 0 && self.config.preview_approximations,
                statistics_key: dehaze_statistics_key(r, level),
            };
            let frame = r.image.level_extent(level);
            developed = batch.local_tone(&r.settings.tone, frame, &toned, coords, &options)?;
        }
        for &c in coords {
            cancel.check()?;
            let t = developed
                .remove(&c)
                .ok_or_else(|| engine_api::EngineError::internal("developed tile missing"))?;
            let mut chain = if presence {
                vec![Op::ToneExtra(&curves_only)]
            } else {
                vec![Op::Tone(&r.settings.tone), Op::ToneExtra(&r.settings.tone)]
            };
            let pre_curve = r.settings.color_before_curves();
            let post_curve = r.settings.color_after_curves();
            if !presence && pre_curve.monochrome.as_ref().is_some_and(|m| m.enabled) {
                chain.insert(chain.len() - 1, Op::Color(&pre_curve));
            }
            chain.extend([
                Op::Color(&post_curve),
                Op::EffectsInCrop(
                    &r.settings.effects,
                    r.image.level_extent(c.level),
                    &r.settings.geometry.crop,
                ),
            ]);
            if let Some(display) = output.display_op(r.settings.output.gamut_mapping) {
                chain.push(display);
            }
            // The whole-image reference develops previews in their own pixel
            // domain (including grain scale), not the sensor-resolution domain.
            let mut t = t;
            t.coord.level = 0;
            let mut t = batch.run_chain(&chain, &t)?;
            t.coord = c;
            finished.push(t);
        }
        Ok(finished)
    }
}
