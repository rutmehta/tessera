use crate::{DemosaicAlgorithm, Image, SigmoidSettings};
use engine_api::{
    EngineError, EngineResult,
    color::{ColorMatrix3, WorkingSpace},
    recipe::{
        DevelopSettings,
        settings::{DemosaicMethod, DisplayTransform},
    },
    tile::{Pyramid, TILE_SIZE},
};
use raw_decode::{CfaImage, CfaLayout, RawMetadata};
pub type Rgb8Image = image::RgbImage;

/// CFA needs its metadata because the engine-api pyramid intentionally holds
/// only samples. RGB input is already scene-linear Rec.2020 with D65 white.
pub enum RenderSource<'a> {
    Cfa {
        image: &'a CfaImage,
        metadata: &'a RawMetadata,
    },
    Rgb(&'a Image),
}

pub fn render(settings: &DevelopSettings, source: &RenderSource<'_>) -> EngineResult<Rgb8Image> {
    render_scaled(settings, source, 1)
}

/// Full-resolution operators followed by an area average in linear light.
/// Never decimate the CFA: doing so aliases Bayer/X-Trans colour phases.
pub fn render_scaled(
    settings: &DevelopSettings,
    source: &RenderSource<'_>,
    scale: u32,
) -> EngineResult<Rgb8Image> {
    let rgb = render_linear_scaled(settings, source, scale)?;
    let mut out = Rgb8Image::new(rgb.width(), rgb.height());
    for coord in rgb.coords() {
        let tile = crate::display(
            &rgb.tile(coord, 0, 1)?,
            SigmoidSettings::default(),
            settings.output.gamut_mapping,
        )?;
        let l = tile.layout();
        let n = l.plane_len();
        let data = tile.samples::<u8>()?;
        let (ox, oy) = coord.pixel_origin(TILE_SIZE);
        for y in 0..l.extent.height {
            for x in 0..l.extent.width {
                let i = (y * l.extent.width + x) as usize;
                out.put_pixel(
                    ox + x,
                    oy + y,
                    image::Rgb([data[i], data[n + i], data[2 * n + i]]),
                );
            }
        }
    }
    Ok(out)
}

/// Scene-linear Rec.2020 through Geometry, then linear-light box downsampling.
/// Output (display) is not applied. Default recipes preserve the M1 path.
pub fn render_linear_scaled(
    settings: &DevelopSettings,
    source: &RenderSource<'_>,
    scale: u32,
) -> EngineResult<Image> {
    validate_settings(settings)?;
    if scale == 0 {
        return Err(EngineError::invalid("scale", "must be positive"));
    }
    let (mut rgb, mut crop) = match source {
        RenderSource::Rgb(image) => {
            if image.planes().len() != 3 {
                return Err(EngineError::invalid("RGB", "three planes required"));
            }
            let mut out = (*image).clone();
            let matrix = crate::white_balance_matrix(
                &settings.white_balance,
                WorkingSpace::LinearRec2020.to_xyz(),
                [1.0; 4],
            )?;
            for coord in out.coords() {
                let mut t = out.tile(coord, 0, 1)?;
                crate::apply_matrix(&mut t, matrix)?;
                out.put(&t)?;
            }
            (out, [0, 0, image.width(), image.height()])
        }
        RenderSource::Cfa { image, metadata } => {
            if image.pyramid().extent().width != metadata.width
                || image.pyramid().extent().height != metadata.height
            {
                return Err(EngineError::invalid(
                    "metadata",
                    "dimensions do not match CFA",
                ));
            }
            let cfa = metadata.cfa_layout;
            crate::mosaic::validate_cfa(cfa)?;
            let period = if matches!(cfa, CfaLayout::XTrans(_)) {
                6
            } else {
                2
            };
            if metadata.width < period || metadata.height < period {
                return Err(EngineError::invalid(
                    "CFA",
                    "image must contain a complete CFA period",
                ));
            }
            let camera_xyz = crate::camera_to_xyz(ColorMatrix3(std::array::from_fn(|r| {
                metadata.cam_xyz[r].map(f64::from)
            })))?;
            let profile = WorkingSpace::LinearRec2020.to_xyz().inverse()? * camera_xyz;
            let wb = crate::white_balance_matrix(
                &settings.white_balance,
                camera_xyz,
                metadata.as_shot_wb,
            )?;
            let algorithm = match settings.demosaic.method {
                DemosaicMethod::Auto => DemosaicAlgorithm::MalvarHeCutler,
                DemosaicMethod::Bilinear => DemosaicAlgorithm::Bilinear,
                _ => {
                    return Err(EngineError::invalid(
                        "demosaic",
                        "only Auto (MHC) and Bilinear implemented",
                    ));
                }
            };
            let raw = Image::from_pyramid(image.pyramid())?;
            let mut recovered = Image::blank(raw.width(), raw.height(), 1);
            for coord in raw.coords() {
                let t = raw.tile(coord, 4, period)?;
                recovered.put(&crate::reconstruct_highlights(
                    &t,
                    cfa,
                    settings.linearize.highlight_reconstruction,
                )?)?;
            }
            drop(raw);
            let mut out = Image::blank(recovered.width(), recovered.height(), 3);
            for coord in recovered.coords() {
                let mut t = crate::demosaic(&recovered.tile(coord, 3, period)?, cfa, algorithm)?;
                // Contract ordering is CameraProfile THEN WhiteBalance.
                crate::apply_matrix(&mut t, profile)?;
                crate::apply_matrix(&mut t, wb)?;
                out.put(&t)?;
            }
            (out, metadata.default_crop)
        }
    };
    if has_m2_settings(settings) {
        rgb = rgb.downsample_crop(crop, 1)?;
        crop = [0, 0, rgb.width(), rgb.height()];
    }
    if settings.detail != Default::default() {
        let input = rgb.clone();
        for coord in input.coords() {
            let mut tile = input.tile(coord, crate::detail_halo(&settings.detail), 1)?;
            crate::detail(&mut tile, &settings.detail)?;
            rgb.put(&tile)?;
        }
    }
    for coord in rgb.coords() {
        let mut tile = rgb.tile(coord, 0, 1)?;
        crate::tone(&mut tile, &settings.tone)?;
        rgb.put(&tile)?;
    }
    if has_m2_settings(settings) {
        // Remove masked sensor margins before estimating global airlight.
        rgb = rgb.downsample_crop(crop, 1)?;
        rgb = crate::tone_extra_image(&rgb, &settings.tone)?;
        for coord in rgb.coords() {
            let mut tile = rgb.tile(coord, 0, 1)?;
            crate::color(&mut tile, &settings.color)?;
            rgb.put(&tile)?;
        }
        let extent = engine_api::tile::Extent::new(rgb.width(), rgb.height());
        for coord in rgb.coords() {
            let mut tile = rgb.tile(coord, 0, 1)?;
            crate::effects_in_crop(
                &mut tile,
                &settings.effects,
                extent,
                &settings.geometry.crop,
            )?;
            rgb.put(&tile)?;
        }
        rgb = crate::geometry(&rgb, &settings.geometry)?;
        rgb.downsample_crop([0, 0, rgb.width(), rgb.height()], scale)
    } else {
        rgb.downsample_crop(crop, scale)
    }
}

/// Whether a recipe needs M2 neighbourhood, colour, effect or geometry passes.
pub fn has_m2_settings(s: &DevelopSettings) -> bool {
    s.detail != Default::default()
        || s.color != Default::default()
        || s.effects != Default::default()
        || s.geometry != Default::default()
        || s.tone.texture != 0.0
        || s.tone.clarity != 0.0
        || s.tone.dehaze != 0.0
        || s.tone.curves != Default::default()
}

/// Reject changed out-of-scope controls instead of silently ignoring them.
/// Public so tiled renderers built on these operators apply the same scope.
pub fn validate_settings(s: &DevelopSettings) -> EngineResult<()> {
    use engine_api::recipe::settings::HighlightReconstruction;
    if !matches!(
        s.demosaic.method,
        DemosaicMethod::Auto | DemosaicMethod::Bilinear
    ) || !matches!(
        s.linearize.highlight_reconstruction,
        HighlightReconstruction::Clip | HighlightReconstruction::ReconstructColor
    ) {
        return Err(EngineError::invalid(
            "settings",
            "unsupported M1 reconstruction/demosaic method",
        ));
    }
    let default = DevelopSettings::default();
    let mut supported = default.clone();
    supported.linearize = s.linearize.clone();
    supported.demosaic.method = s.demosaic.method;
    supported.white_balance = s.white_balance.clone();
    supported.tone = s.tone.clone();
    supported.detail = s.detail.clone();
    supported.color.vibrance = s.color.vibrance;
    supported.color.saturation = s.color.saturation;
    supported.color.hsl = s.color.hsl.clone();
    supported.color.grading = s.color.grading.clone();
    supported.effects.vignette = s.effects.vignette.clone();
    supported.effects.grain = s.effects.grain.clone();
    supported.geometry.crop = s.geometry.crop.clone();
    supported.output.gamut_mapping = s.output.gamut_mapping;
    if s != &supported
        || !matches!(
            s.tone.display_transform,
            DisplayTransform::Native | DisplayTransform::Sigmoid
        )
    {
        return Err(EngineError::invalid(
            "settings",
            "non-default operator not implemented by CPU reference renderer",
        ));
    }
    Ok(())
}
