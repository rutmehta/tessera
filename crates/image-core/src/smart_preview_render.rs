//! Explicit CPU-only camera-linear dispatch. No original-sized caches or hooks.
use super::*;

fn original_required(reason: &str) -> EngineError {
    EngineError::Unsupported {
        what: format!("smart preview: original required: {reason}"),
    }
}

impl Renderer {
    pub(super) fn validate_camera_linear_proxy(
        &self,
        image: &RawImage,
        settings: &DevelopSettings,
    ) -> EngineResult<()> {
        use engine_api::recipe::{MaskKind, ProcessFamily};
        if self.config.process_version.family != ProcessFamily::Native
            || self.config.process_version.revision != 2
        {
            return Err(original_required("Native revision 2 required"));
        }
        let proxy = image
            .camera_linear_proxy()
            .ok_or_else(|| EngineError::invalid("source", "camera-linear proxy required"))?;
        proxy.validate_prefix(settings)?;
        if self.depth_visualisation
            || settings.effects.lens_blur.is_some()
            || !settings.locals.retouch.is_empty()
        {
            return Err(original_required(
                "depth, lens blur and retouch need original dependencies",
            ));
        }
        for group in &settings.locals.adjustments {
            for component in &group.components {
                if !matches!(
                    &component.kind,
                    MaskKind::Linear { .. }
                        | MaskKind::Radial { .. }
                        | MaskKind::Brush { .. }
                        | MaskKind::LuminanceRange { .. }
                        | MaskKind::ColorRange { .. }
                ) {
                    return Err(original_required(
                        "AI/depth masks need original dependencies",
                    ));
                }
            }
        }
        pipeline_cpu::validate_settings(settings)
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
        self.validate_camera_linear_proxy(image, settings)?;
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
        // Prefix validation, original calibration/WB, captured optics, manual
        // masks and geometry all run once in their scalar reference order.
        // EXIF orientation remains the caller's responsibility, as for RAW.
        let developed = pipeline_cpu::render_linear_scaled(
            settings,
            &pipeline_cpu::RenderSource::CameraLinear(image.camera_linear_proxy().unwrap()),
            1 << level,
        )?;
        cancel.check()?;
        let mut seen = HashSet::new();
        for &coord in coords {
            cancel.check()?;
            if !seen.insert(coord) {
                continue;
            }
            let mut tile = developed.tile(TileCoord::new(0, coord.x, coord.y), 0, 1)?;
            if let Some(op) = output.display_op(settings.output.gamut_mapping) {
                tile = CpuStageOp.run(StageId::Output, &op, tile)?;
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
