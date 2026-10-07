//! Camera-linear dispatch: qualified unmapped resident tail, otherwise scalar CPU.
use super::*;
use crate::resident::{ResidentOutput, SurfaceTarget};

fn original_required(reason: &str) -> EngineError {
    EngineError::Unsupported {
        what: format!("smart preview: original required: {reason}"),
    }
}

impl Renderer {
    /// Render-only proxy adaptation using this renderer's registered resources.
    /// A registered depth provider must still return a valid map at render time.
    pub fn proxy_render_plan(
        &self,
        proxy: &pipeline_cpu::CameraLinearProxy,
        settings: &DevelopSettings,
    ) -> (DevelopSettings, Vec<&'static str>) {
        proxy.render_plan_with_resources(
            settings,
            self.mask_cache.has_hooks(),
            self.depth.is_some(),
            self.retouch.is_some(),
        )
    }

    pub(super) fn validate_camera_linear_proxy(
        &self,
        image: &RawImage,
        settings: &DevelopSettings,
    ) -> EngineResult<()> {
        use engine_api::recipe::{MaskKind, ProcessFamily};
        let planned;
        let settings = if let Some(proxy) = image.camera_linear_proxy() {
            planned = proxy
                .render_plan_with_resources(
                    settings,
                    self.mask_cache.has_hooks(),
                    self.depth.is_some(),
                    self.retouch.is_some(),
                )
                .0;
            &planned
        } else {
            settings
        };
        let external_dng = image
            .camera_linear_proxy()
            .is_some_and(|p| p.is_external_dng());
        if !external_dng
            && (self.config.process_version.family != ProcessFamily::Native
                || self.config.process_version.revision != 2)
        {
            return Err(original_required("Native revision 2 required"));
        }
        let proxy = image
            .camera_linear_proxy()
            .ok_or_else(|| EngineError::invalid("source", "camera-linear proxy required"))?;
        proxy.validate_prefix(settings)?;
        if self.depth_visualisation
            || (settings.effects.lens_blur.is_some() && self.depth.is_none())
            || (!settings.locals.retouch.is_empty() && self.retouch.is_none())
        {
            return Err(original_required(
                "depth, lens blur and retouch need original dependencies",
            ));
        }
        for group in settings.locals.adjustments.iter().filter(|g| g.enabled) {
            for component in group
                .components
                .iter()
                .flat_map(engine_api::recipe::MaskComponent::active_leaves)
            {
                if !matches!(
                    &component.kind,
                    MaskKind::Linear { .. }
                        | MaskKind::Radial { .. }
                        | MaskKind::Brush { .. }
                        | MaskKind::LuminanceRange { .. }
                        | MaskKind::ColorRange { .. }
                ) && !(external_dng && component.kind.is_ai() && self.mask_cache.has_hooks())
                {
                    return Err(original_required(
                        "AI/depth masks need original dependencies",
                    ));
                }
            }
        }
        self.validate_settings(settings)
            .map_err(|error| original_required(&format!("unsupported CPU settings: {error}")))
    }

    pub(super) fn run_camera_linear_proxy(
        &self,
        image: &RawImage,
        settings: &DevelopSettings,
        coords: &[TileCoord],
        output: RenderOutput,
        cancel: &CancellationToken,
        sink: &mut dyn FnMut(Tile),
    ) -> EngineResult<()> {
        cancel.check()?;
        let planned;
        let settings = if let Some(proxy) = image.camera_linear_proxy() {
            planned = proxy
                .render_plan_with_resources(
                    settings,
                    self.mask_cache.has_hooks(),
                    self.depth.is_some(),
                    self.retouch.is_some(),
                )
                .0;
            &planned
        } else {
            settings
        };
        if image.camera_linear_proxy().is_some() {
            self.validate_camera_linear_proxy(image, settings)?;
        } else {
            self.validate_settings(settings)?;
        }
        let Some(first) = coords.first() else {
            return cancel.check();
        };
        let level = first.level;
        let extent = Self::output_extent(image, settings, level)?;
        let grid = extent.tile_grid(TILE_SIZE);
        if coords
            .iter()
            .any(|c| c.level != level || c.x >= grid.0 || c.y >= grid.1)
        {
            return Err(EngineError::invalid(
                "tiles",
                "coordinates must share one level and lie inside cropped output",
            ));
        }
        cancel.check()?;
        // Every resident proxy level develops at L0, then reduces linear
        // post-geometry pixels before display encoding.
        let mut unique_seen = HashSet::new();
        let unique: Vec<_> = coords
            .iter()
            .copied()
            .filter(|c| unique_seen.insert(*c))
            .collect();
        if image.metadata().catalog_orientation.is_none_or(|o| o == 1)
            && let Some(rendered) =
                self.try_camera_linear_resident(image, settings, &unique, output, cancel, None)?
        {
            for tile in rendered.tiles {
                cancel.check()?;
                sink(tile);
            }
            return cancel.check();
        }
        // Prefix validation, original calibration/WB, captured optics, manual
        // masks and geometry all run once in their scalar reference order.
        // EXIF orientation remains the caller's responsibility, as for RAW.
        let source = match image.camera_linear_proxy() {
            Some(proxy) => pipeline_cpu::RenderSource::CameraLinear(proxy),
            None => pipeline_cpu::RenderSource::Cfa {
                image: image.cfa(),
                metadata: image.metadata(),
            },
        };
        let locals = |input: &pipeline_cpu::Image,
                      groups: &[engine_api::recipe::LocalAdjustment]| {
            if !self.mask_cache.has_hooks() {
                return pipeline_cpu::locals_image(input, groups, Default::default());
            }
            let mut planes = input.planes().to_vec();
            let upstream = self.stage_chain(settings)[StageId::Color.index()].1;
            for group in groups
                .iter()
                .filter(|g| g.enabled && g.amount != 0. && !g.components.is_empty())
            {
                cancel.check()?;
                // This scalar route develops at full active resolution and only
                // then reduces. External rasters use that same oriented L0 frame.
                let mask =
                    self.mask_cache
                        .rasterize(input, group, 0, upstream, Default::default())?;
                let adjusted = pipeline_cpu::adjust_local(input, &group.params, group.amount)?;
                let blended = pipeline_cpu::blend_local(input, &adjusted, &mask)?;
                for ((out, original), changed) in
                    planes.iter_mut().zip(input.planes()).zip(blended.planes())
                {
                    for ((out, original), changed) in out.iter_mut().zip(original).zip(changed) {
                        *out += changed - original;
                    }
                }
            }
            pipeline_cpu::Image::new(input.width(), input.height(), planes)
        };
        let depth_effects = |input: &pipeline_cpu::Image| self.apply_depth_effects(input, settings);
        let context = pipeline_cpu::LensContext {
            retouch: self.retouch.clone(),
            depth_effects: self
                .depth
                .as_ref()
                .map(|_| &depth_effects as &pipeline_cpu::DepthEffectHook<'_>),
            ..Default::default()
        };
        let developed = if self.is_adobe() {
            pipeline_adobe::render_linear_scaled_with_resources(
                settings,
                &source,
                1 << level,
                self.dcp.as_ref().map(|(p, _)| p.as_ref()),
                Some(&locals),
                &context,
            )?
        } else {
            pipeline_cpu::render_linear_scaled_with_local_hook(
                settings,
                &source,
                1 << level,
                &context,
                None,
                self.denoiser.as_deref(),
                &locals,
            )?
        };
        cancel.check()?;
        let mut seen = HashSet::new();
        for &coord in coords {
            cancel.check()?;
            if !seen.insert(coord) {
                continue;
            }
            let mut tile = developed.tile(TileCoord::new(0, coord.x, coord.y), 0, 1)?;
            if let Some(op) = output.display_op(settings.output.gamut_mapping) {
                tile = if self.is_adobe() {
                    match output {
                        RenderOutput::Display => CpuStageOp::display_linear(tile)?,
                        RenderOutput::DisplayLinear(headroom) => {
                            let matrix = engine_api::color::WorkingSpace::LinearSrgb
                                .to_xyz()
                                .inverse()?
                                * engine_api::color::WorkingSpace::LinearRec2020.to_xyz();
                            pipeline_cpu::apply_matrix(&mut tile, matrix)?;
                            pipeline_cpu::map_rgb(&mut tile, |v| {
                                v.map(|c| c.clamp(0., headroom.get()))
                            })?;
                            tile
                        }
                        RenderOutput::SceneLinear => unreachable!(),
                    }
                } else {
                    CpuStageOp.run(StageId::Output, &op, tile)?
                };
                tile = if op.is_encoded_display() {
                    Tile::from_samples(coord, tile.layout(), tile.samples::<u8>()?.to_vec())?
                } else {
                    Tile::from_samples(coord, tile.layout(), tile.samples::<f32>()?.to_vec())?
                };
            } else {
                tile = Tile::from_samples(coord, tile.layout(), tile.samples::<f32>()?.to_vec())?;
            }
            cancel.check()?;
            sink(tile);
        }
        cancel.check()
    }
}

impl Renderer {
    fn resolve_camera_linear<'a>(
        &self,
        image: &'a RawImage,
        settings: &'a DevelopSettings,
        tail: &'a pipeline_cpu::LensPlan,
    ) -> EngineResult<Resolved<'a>> {
        let proxy = image
            .camera_linear_proxy()
            .ok_or_else(|| EngineError::invalid("source", "camera-linear proxy required"))?;
        // resolve uses original calibration only; addressing is explicitly the
        // proxy frame. Neither original metadata nor source semantics are edited.
        let mut r = self.resolve(image, settings)?;
        r.sensor = image.active_extent();
        r.crop = [0, 0, r.sensor.width, r.sensor.height];
        r.period = 1;
        r.lin_halo = 0;
        r.dem_halo = 0;
        r.allow_resident = true;
        r.lens = Some(tail);
        r.cache_lens = true;
        let domain = ParamHash::of(
            StageId::Decode,
            &(
                "camera-linear-resident-l0-v1",
                pipeline_cpu::CameraLinearProxy::GENERATOR_REVISION,
                proxy.scale(),
                r.crop,
            ),
        );
        for (_, hash) in &mut r.chain {
            *hash = ParamHash::chain(domain, *hash);
        }
        Ok(r)
    }

    pub(super) fn camera_linear_resident_supported(
        &self,
        image: &RawImage,
        settings: &DevelopSettings,
    ) -> EngineResult<bool> {
        if let Some(prepared) = self.prepare_dcp(image, settings)? {
            return prepared.camera_linear_resident_supported(image, settings);
        }
        let planned = image
            .camera_linear_proxy()
            .unwrap()
            .render_plan_with_resources(
                settings,
                self.mask_cache.has_hooks(),
                self.depth.is_some(),
                self.retouch.is_some(),
            )
            .0;
        let settings = &planned;
        self.validate_camera_linear_proxy(image, settings)?;
        let Some(tail) = image
            .camera_linear_proxy()
            .unwrap()
            .resident_tail_plan(settings)?
        else {
            return Ok(false);
        };
        // Mapped geometry is not numerically qualified for proxy GPU rendering:
        // scalar f64 coordinates and cancellation-heavy HDR Lanczos differ from
        // shader arithmetic. Keep capability and dispatch on the exact CPU path.
        if tail.map.is_some() {
            return Ok(false);
        }
        let r = self.resolve_camera_linear(image, settings, &tail)?;
        // Develop's level-independent query also governs adaptive coarse frames.
        // Advertise resident interaction only when the shared full linear tail
        // fits, even if this backend could execute isolated L0 tiles.
        Ok(self.supports_resident(&r, Some(0))
            && self.ops.begin_resident().is_some_and(|batch| {
                batch.supports_level(image.level_extent(0), pipeline_cpu::DETAIL_HALO)
            }))
    }

    pub(super) fn try_camera_linear_resident(
        &self,
        image: &RawImage,
        settings: &DevelopSettings,
        coords: &[TileCoord],
        output: RenderOutput,
        cancel: &CancellationToken,
        surface: Option<SurfaceTarget>,
    ) -> EngineResult<Option<ResidentOutput>> {
        if let Some(prepared) = self.prepare_dcp(image, settings)? {
            return prepared
                .try_camera_linear_resident(image, settings, coords, output, cancel, surface);
        }
        let planned = image
            .camera_linear_proxy()
            .unwrap()
            .render_plan_with_resources(
                settings,
                self.mask_cache.has_hooks(),
                self.depth.is_some(),
                self.retouch.is_some(),
            )
            .0;
        let settings = &planned;
        cancel.check()?;
        self.validate_camera_linear_proxy(image, settings)?;
        let Some(first) = coords.first() else {
            return Ok(None);
        };
        let level = first.level;
        if coords.iter().any(|c| c.level != level) {
            return Err(EngineError::invalid(
                "tiles",
                "camera-linear coordinates must share one level",
            ));
        }
        let Some(tail) = image
            .camera_linear_proxy()
            .unwrap()
            .resident_tail_plan(settings)?
        else {
            return Ok(None);
        };
        // Mapped geometry is not numerically qualified for proxy GPU rendering:
        // scalar f64 coordinates and cancellation-heavy HDR Lanczos differ from
        // shader arithmetic. Keep capability and dispatch on the exact CPU path.
        if tail.map.is_some() {
            return Ok(None);
        }
        let r = self.resolve_camera_linear(image, settings, &tail)?;
        if !self.supports_resident(&r, Some(0)) {
            return Ok(None);
        }
        let Some(batch) = self.ops.begin_resident() else {
            return Ok(None);
        };
        let extent = Self::output_extent(image, settings, level)?;
        let grid = extent.tile_grid(TILE_SIZE);
        if coords.iter().any(|c| c.x >= grid.0 || c.y >= grid.1) {
            return Err(EngineError::invalid(
                "tiles",
                "outside camera-linear output",
            ));
        }
        if level > 0 {
            return self.run_camera_linear_coarse(&r, coords, output, cancel, batch, surface);
        }
        self.run_resident(&r, coords, output, cancel, batch, surface)
            .map(Some)
    }
}
