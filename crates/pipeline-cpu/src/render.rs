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
    validate_settings(settings)?;
    if scale == 0 {
        return Err(EngineError::invalid("scale", "must be positive"));
    }
    let (mut rgb, crop) = match source {
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
    for coord in rgb.coords() {
        let mut tile = rgb.tile(coord, 0, 1)?;
        crate::tone(&mut tile, &settings.tone)?;
        rgb.put(&tile)?;
    }
    let rgb = rgb.downsample_crop(crop, scale)?;
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

/// M1 has explicit no-op stages at their default values. Reject changed
/// out-of-scope controls instead of producing a deceptively successful render.
fn validate_settings(s: &DevelopSettings) -> EngineResult<()> {
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
    supported.tone.exposure = s.tone.exposure;
    supported.tone.contrast = s.tone.contrast;
    supported.tone.highlights = s.tone.highlights;
    supported.tone.shadows = s.tone.shadows;
    supported.tone.whites = s.tone.whites;
    supported.tone.blacks = s.tone.blacks;
    supported.tone.display_transform = s.tone.display_transform;
    supported.output.gamut_mapping = s.output.gamut_mapping;
    if s != &supported
        || !matches!(
            s.tone.display_transform,
            DisplayTransform::Native | DisplayTransform::Sigmoid
        )
    {
        return Err(EngineError::invalid(
            "settings",
            "non-default operator not implemented by M1 reference renderer",
        ));
    }
    Ok(())
}
