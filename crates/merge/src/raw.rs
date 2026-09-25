//! CFA adapter: level-normalized mosaic -> unbalanced linear camera RGB.
use crate::{LinearImage, Result};
use engine_api::{
    color::ColorMatrix3,
    tile::{Pyramid, TILE_SIZE},
};
use pipeline_cpu::{DemosaicAlgorithm, Image, demosaic};
use raw_decode::{CfaImage, CfaLayout, RawMetadata};
/// Demosaic with the pipeline MHC/Bayer or X-Trans fallback, then active crop.
/// No camera profile, WB, lens correction, highlight reconstruction or tone.
/// LibRaw orientation is not applied: merge inputs must share sensor orientation.
pub fn from_cfa(cfa: &CfaImage, metadata: &RawMetadata) -> Result<LinearImage> {
    let extent = cfa.pyramid().extent();
    if extent.width != metadata.width
        || extent.height != metadata.height
        || extent.area() > 64 * 1024 * 1024
    {
        return Err("invalid CFA dimensions".into());
    }
    let matrix: [[f64; 3]; 3] = std::array::from_fn(|i| metadata.cam_xyz[i].map(f64::from));
    ColorMatrix3(matrix).inverse().map_err(|e| e.to_string())?;
    let wb = metadata.as_shot_wb;
    if wb[..3].iter().any(|v| !v.is_finite() || *v <= 0.) {
        return Err("invalid as-shot WB multipliers".into());
    }
    let neutral = std::array::from_fn(|i| wb[1] as f64 / wb[i] as f64);
    let [left, top, width, height] = metadata.default_crop;
    if width == 0
        || height == 0
        || left.checked_add(width).is_none_or(|v| v > extent.width)
        || top.checked_add(height).is_none_or(|v| v > extent.height)
    {
        return Err("invalid active crop".into());
    }
    let source = Image::from_pyramid(cfa.pyramid()).map_err(|e| e.to_string())?;
    let mut pixels = vec![[0.; 3]; width as usize * height as usize];
    let period = if matches!(metadata.cfa_layout, CfaLayout::XTrans(_)) {
        6
    } else {
        2
    };
    for coord in source.coords() {
        let tile = source.tile(coord, 3, period).map_err(|e| e.to_string())?;
        let rgb = demosaic(
            &tile,
            metadata.cfa_layout,
            DemosaicAlgorithm::MalvarHeCutler,
        )
        .map_err(|e| e.to_string())?;
        let data = rgb.samples::<f32>().map_err(|e| e.to_string())?;
        let e = rgb.layout().extent;
        let n = e.area() as usize;
        let (ox, oy) = coord.pixel_origin(TILE_SIZE);
        for y in 0..e.height {
            for x in 0..e.width {
                let (gx, gy) = (ox + x, oy + y);
                if gx >= left && gx < left + width && gy >= top && gy < top + height {
                    let src = (y * e.width + x) as usize;
                    let dst = ((gy - top) * width + gx - left) as usize;
                    pixels[dst] = [data[src], data[n + src], data[2 * n + src]];
                }
            }
        }
    }
    let image = LinearImage {
        width: width as usize,
        height: height as usize,
        pixels,
        color_matrix: matrix,
        as_shot_neutral: neutral,
    };
    image.validate()?;
    Ok(image)
}
