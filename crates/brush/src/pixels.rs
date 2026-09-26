//! Cached random access to a [`Raster`]'s normalized pixels.

use std::collections::HashMap;
use std::sync::Arc;

use compositor::Raster;
use engine_api::tile::TILE_SIZE;

/// Read-only pixel cache over a raster snapshot (decoded tiles are kept).
#[derive(Debug, Clone)]
pub struct Pixels {
    raster: Raster,
    tiles: HashMap<(u32, u32), Arc<Vec<f32>>>,
}

impl Pixels {
    /// Wraps a snapshot (cloning a raster only clones tile `Arc`s).
    pub fn new(raster: &Raster) -> Self {
        Self {
            raster: raster.clone(),
            tiles: HashMap::new(),
        }
    }

    /// The snapshot.
    pub fn raster(&self) -> &Raster {
        &self.raster
    }

    /// Normalized straight pixel; outside the canvas reads transparent zero.
    pub fn get(&mut self, x: i64, y: i64) -> [f32; 4] {
        let e = self.raster.extent();
        if x < 0 || y < 0 || x >= i64::from(e.width) || y >= i64::from(e.height) {
            return [0.0; 4];
        }
        let (x, y) = (x as u32, y as u32);
        let (tx, ty) = (x / TILE_SIZE, y / TILE_SIZE);
        let raster = &self.raster;
        let t = self.tiles.entry((tx, ty)).or_insert_with(|| {
            let mut v = Vec::new();
            // A decode error leaves the default-filled buffer.
            let _ = raster.read_tile(tx, ty, &mut v);
            if v.is_empty() {
                v.resize(raster.layout(tx, ty).len(), raster.default_value());
            }
            Arc::new(v)
        });
        let l = self.raster.layout(tx, ty);
        let (stride, plane) = (l.stride(), l.plane_len());
        let i = (y % TILE_SIZE) as usize * stride + (x % TILE_SIZE) as usize;
        let mut px = [0.0; 4];
        let n = usize::from(self.raster.channels()).min(4);
        for (c, p) in px.iter_mut().enumerate().take(n) {
            *p = t[c * plane + i];
        }
        px
    }

    /// Bilinear sample at continuous coordinates (pixel centres at `i + 0.5`).
    pub fn sample(&mut self, x: f32, y: f32) -> [f32; 4] {
        let (fx, fy) = (x - 0.5, y - 0.5);
        let (x0, y0) = (fx.floor(), fy.floor());
        let (ax, ay) = (fx - x0, fy - y0);
        let (x0, y0) = (x0 as i64, y0 as i64);
        if ax == 0.0 && ay == 0.0 {
            return self.get(x0, y0);
        }
        let p00 = self.get(x0, y0);
        let p10 = self.get(x0 + 1, y0);
        let p01 = self.get(x0, y0 + 1);
        let p11 = self.get(x0 + 1, y0 + 1);
        std::array::from_fn(|c| {
            let a = p00[c] + (p10[c] - p00[c]) * ax;
            let b = p01[c] + (p11[c] - p01[c]) * ax;
            a + (b - a) * ay
        })
    }
}
