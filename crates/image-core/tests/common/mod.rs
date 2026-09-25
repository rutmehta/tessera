#![allow(dead_code)]

use std::sync::Arc;

use engine_api::color::ColorMatrix3;
use engine_api::id::ImageId;
use engine_api::tile::{Extent, TILE_SIZE, Tile};
use image_core::RawImage;
use raw_decode::{CfaImage, CfaLayout, RawMetadata};

pub const RGGB: CfaLayout = CfaLayout::Bayer([[0, 1], [3, 2]]);

pub fn xtrans() -> CfaLayout {
    // Fujifilm's 6x6 layout (0 = R, 1 = G, 2 = B).
    CfaLayout::XTrans([
        [1, 1, 0, 1, 1, 2],
        [1, 1, 2, 1, 1, 0],
        [2, 0, 1, 0, 2, 1],
        [1, 1, 2, 1, 1, 0],
        [1, 1, 0, 1, 1, 2],
        [0, 2, 1, 2, 0, 1],
    ])
}

pub fn metadata(width: u32, height: u32, cfa: CfaLayout, crop: [u32; 4]) -> RawMetadata {
    RawMetadata {
        make: "Synthetic".into(),
        model: "Test".into(),
        lens: None,
        iso: 100.0,
        shutter_s: 0.01,
        aperture: 4.0,
        focal_mm: 50.0,
        capture_time: 0,
        orientation: 1,
        width,
        height,
        cfa_layout: cfa,
        black_levels: [0.0; 4],
        white_level: 16383,
        as_shot_wb: [2.0, 1.0, 1.6, 1.0],
        camera_to_xyz: ColorMatrix3([[0.0; 3]; 3]),
        cam_xyz: [
            [0.9, 0.2, -0.1],
            [-0.3, 1.2, 0.1],
            [0.0, 0.1, 0.8],
            [0.0; 3],
        ],
        rgb_cam: [[0.0; 4]; 3],
        default_crop: crop,
        has_gain_map: false,
        has_opcode_list: false,
    }
}

/// Deterministic scene: gradients, texture and clipped highlight patches.
pub fn samples(width: u32, height: u32, cfa: CfaLayout) -> Vec<f32> {
    let mut state = 0x2545_f491_4f6c_dd1du64;
    let mut out = Vec::with_capacity((width * height) as usize);
    for y in 0..height {
        for x in 0..width {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            let noise = (state >> 40) as f32 / (1u64 << 24) as f32;
            let c = cfa.channel_at(x, y);
            let gain = [0.5, 1.0, 0.7, 1.0][c];
            let base = 0.05 + 0.6 * x as f32 / width as f32 + 0.2 * (y as f32 / 37.0).sin().abs();
            let mut v = gain * base + 0.05 * noise;
            let (cx, cy) = (
                x as i64 - width as i64 * 2 / 3,
                y as i64 - height as i64 / 3,
            );
            if cx * cx + cy * cy < 40 * 40 {
                v = if c == 1 { 1.2 } else { 1.05 + 0.1 * noise };
            }
            out.push(v);
        }
    }
    out
}

pub fn synthetic(id: u128, width: u32, height: u32, cfa: CfaLayout, crop: [u32; 4]) -> RawImage {
    let cfa_image = CfaImage::from_linear(width, height, samples(width, height, cfa)).unwrap();
    RawImage::new(
        ImageId(id),
        Arc::new(cfa_image),
        Arc::new(metadata(width, height, cfa, crop)),
    )
    .unwrap()
}

/// Interleaves U8 display tiles into an RGB buffer of `extent`.
pub fn assemble_u8(extent: Extent, tiles: &[Tile]) -> Vec<u8> {
    let mut out = vec![0u8; extent.area() as usize * 3];
    let mut covered = 0u64;
    for t in tiles {
        let l = t.layout();
        let n = l.plane_len();
        let d = t.samples::<u8>().unwrap();
        let (ox, oy) = t.coord().pixel_origin(TILE_SIZE);
        for y in 0..l.extent.height {
            for x in 0..l.extent.width {
                let i = (y * l.extent.width + x) as usize;
                let o = (((oy + y) * extent.width + ox + x) * 3) as usize;
                for c in 0..3 {
                    out[o + c] = d[c * n + i];
                }
            }
        }
        covered += l.extent.area();
    }
    assert_eq!(covered, extent.area(), "tiles must cover the level exactly");
    out
}

/// Planar F32 scene-linear tiles into three full planes of `extent`.
pub fn assemble_f32(extent: Extent, tiles: &[Tile]) -> Vec<Vec<f32>> {
    let mut out = vec![vec![0f32; extent.area() as usize]; 3];
    for t in tiles {
        let l = t.layout();
        let n = l.plane_len();
        let d = t.samples::<f32>().unwrap();
        let (ox, oy) = t.coord().pixel_origin(TILE_SIZE);
        for y in 0..l.extent.height {
            for x in 0..l.extent.width {
                let i = (y * l.extent.width + x) as usize;
                let o = ((oy + y) * extent.width + ox + x) as usize;
                for (c, plane) in out.iter_mut().enumerate() {
                    plane[o] = d[c * n + i];
                }
            }
        }
    }
    out
}

pub fn max_u8_diff(a: &[u8], b: &[u8]) -> u8 {
    assert_eq!(a.len(), b.len());
    a.iter()
        .zip(b)
        .map(|(x, y)| x.abs_diff(*y))
        .max()
        .unwrap_or(0)
}

pub fn max_f32_diff(a: &[Vec<f32>], b: &[Vec<f32>]) -> f32 {
    assert_eq!(a.len(), b.len());
    a.iter()
        .zip(b)
        .flat_map(|(p, q)| {
            assert_eq!(p.len(), q.len());
            p.iter().zip(q).map(|(x, y)| (x - y).abs())
        })
        .fold(0.0, f32::max)
}
