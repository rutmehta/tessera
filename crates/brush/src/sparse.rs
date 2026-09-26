//! Sparse 64² tiled canvas-space buffers for per-stroke state.

use std::collections::HashMap;

use compositor::Rect;

const T: i64 = 64;

/// A sparse, unbounded 2-D grid of `V` (absent tiles read as `V::default()`).
#[derive(Debug, Clone, Default)]
pub struct Sparse<V: Copy + Default> {
    tiles: HashMap<(i64, i64), Box<[V]>>,
}

impl<V: Copy + Default> Sparse<V> {
    /// An empty grid.
    pub fn new() -> Self {
        Self {
            tiles: HashMap::new(),
        }
    }

    #[inline]
    fn key(x: i64, y: i64) -> ((i64, i64), usize) {
        (
            (x.div_euclid(T), y.div_euclid(T)),
            (y.rem_euclid(T) * T + x.rem_euclid(T)) as usize,
        )
    }

    /// Value at `(x, y)`.
    #[inline]
    pub fn get(&self, x: i64, y: i64) -> V {
        let (k, i) = Self::key(x, y);
        self.tiles.get(&k).map_or_else(V::default, |t| t[i])
    }

    /// True if `(x, y)` lies in an allocated tile.
    pub fn contains(&self, x: i64, y: i64) -> bool {
        self.tiles.contains_key(&Self::key(x, y).0)
    }

    /// Mutable value at `(x, y)`, allocating its tile.
    #[inline]
    pub fn get_mut(&mut self, x: i64, y: i64) -> &mut V {
        let (k, i) = Self::key(x, y);
        &mut self
            .tiles
            .entry(k)
            .or_insert_with(|| vec![V::default(); (T * T) as usize].into_boxed_slice())[i]
    }

    /// Row-major copy of `r`.
    pub fn read_rect(&self, r: Rect) -> Vec<V> {
        let mut out = Vec::with_capacity(r.area().max(0) as usize);
        for y in r.y0..r.y1 {
            for x in r.x0..r.x1 {
                out.push(self.get(x, y));
            }
        }
        out
    }

    /// Writes a row-major block over `r`.
    pub fn write_rect(&mut self, r: Rect, data: &[V]) {
        let w = r.width().max(0) as usize;
        for y in r.y0..r.y1 {
            for x in r.x0..r.x1 {
                *self.get_mut(x, y) = data[(y - r.y0) as usize * w + (x - r.x0) as usize];
            }
        }
    }

    /// Union of allocated tile rectangles.
    pub fn bounds(&self) -> Option<Rect> {
        let mut r = Rect::default();
        for &(tx, ty) in self.tiles.keys() {
            r = r.union(&Rect::new(tx * T, ty * T, tx * T + T, ty * T + T));
        }
        (!r.is_empty()).then_some(r)
    }

    /// Drops every tile.
    pub fn clear(&mut self) {
        self.tiles.clear();
    }
}
