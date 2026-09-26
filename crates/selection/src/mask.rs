//! Dense masks and images.

use compositor::{Depth, Raster, Rect};
use engine_api::tile::{Extent, TILE_SIZE};
use engine_api::{EngineError, EngineResult};

/// A dense soft selection, row-major, values `0..=1`.
#[derive(Debug, Clone, PartialEq)]
pub struct Mask {
    width: u32,
    height: u32,
    data: Vec<f32>,
}

impl Mask {
    /// All zero.
    pub fn new(width: u32, height: u32) -> Self {
        Self::filled(width, height, 0.0)
    }

    /// Constant.
    pub fn filled(width: u32, height: u32, v: f32) -> Self {
        Self {
            width,
            height,
            data: vec![v; width as usize * height as usize],
        }
    }

    /// From row-major data.
    pub fn from_vec(width: u32, height: u32, data: Vec<f32>) -> EngineResult<Self> {
        if data.len() != width as usize * height as usize {
            return Err(EngineError::invalid("mask", "size does not match data"));
        }
        Ok(Self {
            width,
            height,
            data,
        })
    }

    /// Evaluates `f(x, y)` at every pixel.
    pub fn from_fn(width: u32, height: u32, mut f: impl FnMut(u32, u32) -> f32) -> Self {
        let mut data = Vec::with_capacity(width as usize * height as usize);
        for y in 0..height {
            for x in 0..width {
                data.push(f(x, y));
            }
        }
        Self {
            width,
            height,
            data,
        }
    }

    /// Width.
    pub fn width(&self) -> u32 {
        self.width
    }

    /// Height.
    pub fn height(&self) -> u32 {
        self.height
    }

    /// Size as an extent.
    pub fn extent(&self) -> Extent {
        Extent::new(self.width, self.height)
    }

    /// Row-major values.
    pub fn data(&self) -> &[f32] {
        &self.data
    }

    /// Mutable row-major values.
    pub fn data_mut(&mut self) -> &mut [f32] {
        &mut self.data
    }

    /// Value at `(x, y)`; zero outside.
    #[inline]
    pub fn get(&self, x: i64, y: i64) -> f32 {
        if x < 0 || y < 0 || x >= i64::from(self.width) || y >= i64::from(self.height) {
            0.0
        } else {
            self.data[y as usize * self.width as usize + x as usize]
        }
    }

    /// Sets `(x, y)` (ignored outside).
    #[inline]
    pub fn set(&mut self, x: i64, y: i64, v: f32) {
        if x >= 0 && y >= 0 && x < i64::from(self.width) && y < i64::from(self.height) {
            self.data[y as usize * self.width as usize + x as usize] = v;
        }
    }

    /// Sum of values (selected area in pixels).
    pub fn area(&self) -> f64 {
        self.data.iter().map(|v| f64::from(*v)).sum()
    }

    /// Intersection over union of the `> 0.5` regions (1 when both empty).
    pub fn iou(&self, other: &Mask) -> f32 {
        let (mut i, mut u) = (0u64, 0u64);
        for (a, b) in self.data.iter().zip(&other.data) {
            let (a, b) = (*a > 0.5, *b > 0.5);
            i += u64::from(a && b);
            u += u64::from(a || b);
        }
        if u == 0 { 1.0 } else { i as f32 / u as f32 }
    }

    /// Tight bounds of values above `threshold`.
    pub fn bounds(&self, threshold: f32) -> Option<Rect> {
        let mut r = Rect::default();
        for y in 0..self.height as i64 {
            for x in 0..self.width as i64 {
                if self.get(x, y) > threshold {
                    r = r.union(&Rect::new(x, y, x + 1, y + 1));
                }
            }
        }
        (!r.is_empty()).then_some(r)
    }

    /// A single-channel raster (tiles are only stored where the mask is
    /// non-zero; F32 is the document selection depth).
    pub fn to_raster(&self, depth: Depth) -> EngineResult<Raster> {
        let mut r = Raster::new(self.extent(), 1, depth, 0.0);
        let (cols, rows) = r.grid();
        let ts = i64::from(TILE_SIZE);
        for ty in 0..rows {
            for tx in 0..cols {
                let tr = Rect::new(
                    i64::from(tx) * ts,
                    i64::from(ty) * ts,
                    (i64::from(tx) * ts + ts).min(i64::from(self.width)),
                    (i64::from(ty) * ts + ts).min(i64::from(self.height)),
                );
                let any = (tr.y0..tr.y1).any(|y| (tr.x0..tr.x1).any(|x| self.get(x, y) != 0.0));
                if any {
                    r.edit_region(tr, 0, |x, y, p| p[0] = self.get(i64::from(x), i64::from(y)))?;
                }
            }
        }
        Ok(r)
    }

    /// The first channel of a raster.
    pub fn from_raster(r: &Raster) -> EngineResult<Mask> {
        let img = dense(r)?;
        let e = r.extent();
        Ok(Mask {
            width: e.width,
            height: e.height,
            data: img.into_iter().map(|p| p[0]).collect(),
        })
    }
}

/// Reads a whole raster as straight normalized RGBA-ish pixels (missing
/// channels read 0).
fn dense(r: &Raster) -> EngineResult<Vec<[f32; 4]>> {
    let e = r.extent();
    let (w, h) = (e.width as usize, e.height as usize);
    let mut out = vec![[0.0f32; 4]; w * h];
    let (cols, rows) = r.grid();
    let mut buf = Vec::new();
    let n = usize::from(r.channels()).min(4);
    for ty in 0..rows {
        for tx in 0..cols {
            r.read_tile(tx, ty, &mut buf)?;
            let l = r.layout(tx, ty);
            let (stride, plane) = (l.stride(), l.plane_len());
            let (ox, oy) = ((tx * TILE_SIZE) as usize, (ty * TILE_SIZE) as usize);
            for y in 0..l.extent.height as usize {
                for x in 0..l.extent.width as usize {
                    let o = &mut out[(oy + y) * w + ox + x];
                    for c in 0..n {
                        o[c] = buf[c * plane + y * stride + x];
                    }
                }
            }
        }
    }
    Ok(out)
}

/// A dense straight-RGBA image (display-referred, normalized).
#[derive(Debug, Clone, PartialEq)]
pub struct Image {
    /// Width.
    pub width: u32,
    /// Height.
    pub height: u32,
    /// Row-major pixels.
    pub data: Vec<[f32; 4]>,
}

impl Image {
    /// Evaluates `f(x, y)` at every pixel.
    pub fn from_fn(width: u32, height: u32, mut f: impl FnMut(u32, u32) -> [f32; 4]) -> Self {
        let mut data = Vec::with_capacity(width as usize * height as usize);
        for y in 0..height {
            for x in 0..width {
                data.push(f(x, y));
            }
        }
        Self {
            width,
            height,
            data,
        }
    }

    /// Reads a 1/3/4-channel raster (gray expands to RGB; missing alpha = 1).
    pub fn from_raster(r: &Raster) -> EngineResult<Image> {
        let ch = r.channels();
        let mut data = dense(r)?;
        for p in &mut data {
            match ch {
                1 => *p = [p[0], p[0], p[0], 1.0],
                3 => p[3] = 1.0,
                _ => {}
            }
        }
        let e = r.extent();
        Ok(Image {
            width: e.width,
            height: e.height,
            data,
        })
    }

    /// Pixel with clamp-to-edge addressing.
    #[inline]
    pub fn get(&self, x: i64, y: i64) -> [f32; 4] {
        let x = x.clamp(0, i64::from(self.width) - 1) as usize;
        let y = y.clamp(0, i64::from(self.height) - 1) as usize;
        self.data[y * self.width as usize + x]
    }

    /// Rec. 709 luma of every pixel.
    pub fn luminance(&self) -> Vec<f32> {
        self.data
            .iter()
            .map(|p| 0.2126 * p[0] + 0.7152 * p[1] + 0.0722 * p[2])
            .collect()
    }
}
