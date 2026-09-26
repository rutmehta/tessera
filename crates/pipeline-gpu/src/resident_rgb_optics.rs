//! RGB-only optics bridge using the existing resident lens kernels.
//!
//! No pixel uploads/readbacks or analysis occur here. Auto lens/CA analysis
//! must be supplied as a real `ResolvedLens` for the original RGB input;
//! absent analysis is a capability miss, never an identity correction.
use engine_api::{
    EngineError, EngineResult,
    color::ColorMatrix3,
    recipe::{DevelopSettings, settings::LensProfileSource},
    tile::Extent,
};
use pipeline_cpu::{LensPlan, ResolvedLens};

/// Prepared optics for one whole RGB frame. `new` is the capability query:
/// `Ok(None)` requires CPU fallback (notably unresolved Auto, defringe,
/// independent manual CA, embedded warps, Upright and EXIF orientation).
/// A supplied resolution must belong to these settings and this RGB image.
#[derive(Clone, Debug)]
pub struct RgbOpticsPlan {
    frame: Extent,
    lens: LensPlan,
}
impl RgbOpticsPlan {
    /// Output dimensions after the composed lens/geometry map.
    pub fn output_extent(&self) -> Extent {
        self.lens.map.as_ref().map_or(self.frame, |p| {
            let (w, h) = p.output_extent(self.frame.width, self.frame.height);
            Extent::new(w, h)
        })
    }

    fn check(
        &self,
        batch: &dyn image_core::resident::ResidentBatch,
        tile: &image_core::resident::ResidentTile,
    ) -> EngineResult<()> {
        if tile.layout.extent != self.frame || tile.layout.halo != 0 || tile.layout.channels != 3 {
            return Err(EngineError::invalid(
                "RGB optics",
                "whole-frame halo-free RGB required",
            ));
        }
        if !batch.supports_level(self.frame, 0) || !batch.supports_level(self.output_extent(), 0) {
            return Err(EngineError::Unsupported {
                what: "RGB optics frame exceeds resident capabilities".into(),
            });
        }
        Ok(())
    }

    /// Profile/image-calibrated RGB CA before white balance and Detail.
    /// Input is one full frame imported via the same-device resident buffer
    /// bridge (planar f32) or uploaded/gathered by this batch. No submission
    /// or readback occurs. The complete frame supplies bilinear neighbours.
    /// Defringe actually precedes WB in the CPU RGB reference; it is rejected
    /// by `new`, not incorrectly moved into the post-WB stage.
    pub fn before_white_balance(
        &self,
        batch: &mut dyn image_core::resident::ResidentBatch,
        tile: &image_core::resident::ResidentTile,
    ) -> EngineResult<image_core::resident::ResidentTile> {
        self.check(batch, tile)?;
        match &self.lens.ca {
            Some(p) => batch.lateral_ca_at(tile, (0, 0), self.frame, p),
            None => Ok(tile.clone()),
        }
    }

    /// Profile gain then manual vignetting, after WB and before Detail.
    pub fn after_white_balance(
        &self,
        batch: &mut dyn image_core::resident::ResidentBatch,
        tile: &image_core::resident::ResidentTile,
    ) -> EngineResult<image_core::resident::ResidentTile> {
        self.check(batch, tile)?;
        match &self.lens.vignette {
            Some(p) => batch.lens_gain_at(tile, (0, 0), self.frame, p),
            None => Ok(tile.clone()),
        }
    }

    /// Composed distortion/transform/crop with Lanczos-3 after Effects.
    /// Retain the exported ResidentBuffer guard and finish the batch with an
    /// empty output vector to submit GPU-only; dropping the batch discards work.
    pub fn after_effects(
        &self,
        batch: &mut dyn image_core::resident::ResidentBatch,
        tile: &image_core::resident::ResidentTile,
    ) -> EngineResult<image_core::resident::ResidentTile> {
        self.check(batch, tile)?;
        match &self.lens.map {
            Some(p) => batch.remap_rows(
                self.frame,
                tile,
                (0, self.frame.height),
                p,
                self.output_extent(),
                0..self.output_extent().height,
            ),
            None => Ok(tile.clone()),
        }
    }

    pub fn new(
        settings: &DevelopSettings,
        frame: Extent,
        resolved: Option<&ResolvedLens>,
    ) -> EngineResult<Option<Self>> {
        if frame.width == 0 || frame.height == 0 {
            return Err(EngineError::invalid(
                "RGB optics",
                "nonempty frame required",
            ));
        }
        let manual;
        let resolved = match resolved {
            Some(r) => r,
            None => {
                // Even source=None can request image-derived CA; its enable
                // flag, not the scale, determines whether CPU analysis runs.
                if settings.lens.profile != LensProfileSource::None
                    || settings.lens.remove_chromatic_aberration
                {
                    return Ok(None);
                }
                // The resolver has no public identity constructor. With both
                // analysis paths explicitly disabled these placeholder samples
                // are never inspected; only manual control validation runs.
                let unused = pipeline_cpu::Image::new(1, 1, vec![vec![0.]; 3])?;
                manual =
                    pipeline_cpu::resolve_lens(&unused, &settings.lens, None, &Default::default())?;
                &manual
            }
        };
        // plan() only consumes CFA kind and crop for RGB. Unsupported means
        // *not a Bayer sensor*, avoiding the database-CA-on-Bayer restriction.
        let metadata = raw_decode::RawMetadata {
            make: String::new(),
            model: String::new(),
            lens: None,
            iso: 0.,
            shutter_s: 0.,
            aperture: 0.,
            focal_mm: 0.,
            capture_time: 0,
            orientation: 1,
            width: frame.width,
            height: frame.height,
            cfa_layout: raw_decode::CfaLayout::Unsupported,
            black_levels: [0.; 4],
            white_level: 1,
            as_shot_wb: [1.; 4],
            camera_to_xyz: ColorMatrix3([[0.; 3]; 3]),
            cam_xyz: [[0.; 3]; 4],
            rgb_cam: [[0.; 4]; 3],
            default_crop: [0, 0, frame.width, frame.height],
            has_gain_map: false,
            has_opcode_list: false,
            opcode_lists: [None, None, None],
        };
        Ok(resolved
            .plan(settings, &metadata)?
            .map(|lens| Self { frame, lens }))
    }
}
