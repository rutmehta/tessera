use compositor::{Depth, Raster, Rect};
use engine_api::tile::Extent;
use filters::caf::{FillParams, fill};
use merge::{LinearImage, Result};
use std::sync::atomic::AtomicBool;

/// Production-equivalent CAF adapter for callers above compositor/merge.
pub fn caf(image: &LinearImage, coverage: &[bool]) -> Result<Vec<[f32; 3]>> {
    let (w, h) = (image.width, image.height);
    let mut raster = Raster::new(
        Extent {
            width: w as u32,
            height: h as u32,
        },
        4,
        Depth::F32,
        0.,
    );
    raster
        .edit_region(Rect::new(0, 0, w as i64, h as i64), 1, |x, y, p| {
            let c = image.pixels[y as usize * w + x as usize];
            *p = [c[0], c[1], c[2], 1.];
        })
        .map_err(|e| e.to_string())?;
    let mask: Vec<_> = coverage.iter().map(|v| if *v { 0. } else { 1. }).collect();
    let result = fill(
        &raster,
        &mask,
        &FillParams::default(),
        &AtomicBool::new(false),
    )
    .map_err(|e| e.to_string())?;
    Ok((0..w * h)
        .map(|i| {
            let p = result.composite.pixel((i % w) as u32, (i / w) as u32);
            [p[0], p[1], p[2]]
        })
        .collect())
}
