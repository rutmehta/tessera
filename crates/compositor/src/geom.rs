//! Integer rectangles, affine transforms and the process-wide revision clock.

use std::sync::atomic::{AtomicU64, Ordering};

use engine_api::tile::{Extent, TILE_SIZE, TileCoord};
use serde::{Deserialize, Serialize};

/// Half-open pixel rectangle `[x0, x1) × [y0, y1)`. Signed so smart-object
/// bounds may extend past the canvas.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub struct Rect {
    /// Left edge (inclusive).
    pub x0: i64,
    /// Top edge (inclusive).
    pub y0: i64,
    /// Right edge (exclusive).
    pub x1: i64,
    /// Bottom edge (exclusive).
    pub y1: i64,
}

impl Rect {
    /// A rectangle from its edges.
    pub const fn new(x0: i64, y0: i64, x1: i64, y1: i64) -> Self {
        Self { x0, y0, x1, y1 }
    }

    /// `[0, w) × [0, h)`.
    pub fn of_extent(e: Extent) -> Self {
        Self::new(0, 0, i64::from(e.width), i64::from(e.height))
    }

    /// True when the rectangle holds no pixels.
    pub fn is_empty(&self) -> bool {
        self.x1 <= self.x0 || self.y1 <= self.y0
    }

    /// Width (0 when empty).
    pub fn width(&self) -> i64 {
        (self.x1 - self.x0).max(0)
    }

    /// Height (0 when empty).
    pub fn height(&self) -> i64 {
        (self.y1 - self.y0).max(0)
    }

    /// Pixel count.
    pub fn area(&self) -> i64 {
        self.width() * self.height()
    }

    /// Intersection (possibly empty).
    pub fn intersect(&self, o: &Rect) -> Rect {
        Rect::new(
            self.x0.max(o.x0),
            self.y0.max(o.y0),
            self.x1.min(o.x1),
            self.y1.min(o.y1),
        )
    }

    /// Bounding union; an empty operand is ignored.
    pub fn union(&self, o: &Rect) -> Rect {
        if self.is_empty() {
            return *o;
        }
        if o.is_empty() {
            return *self;
        }
        Rect::new(
            self.x0.min(o.x0),
            self.y0.min(o.y0),
            self.x1.max(o.x1),
            self.y1.max(o.y1),
        )
    }

    /// True if the rectangles share at least one pixel.
    pub fn intersects(&self, o: &Rect) -> bool {
        !self.intersect(o).is_empty()
    }

    /// Grows every edge by `d` pixels.
    pub fn inflate(&self, d: i64) -> Rect {
        Rect::new(self.x0 - d, self.y0 - d, self.x1 + d, self.y1 + d)
    }

    /// The level-`level` pixels whose mip footprint touches this level-0
    /// rectangle (rounded outward).
    pub fn to_level(&self, level: u8) -> Rect {
        let d = 1i64 << level;
        Rect::new(
            self.x0.div_euclid(d),
            self.y0.div_euclid(d),
            (self.x1 + d - 1).div_euclid(d),
            (self.y1 + d - 1).div_euclid(d),
        )
    }

    /// The level-0 footprint of this level-`level` rectangle.
    pub fn to_level0(&self, level: u8) -> Rect {
        let d = 1i64 << level;
        Rect::new(self.x0 * d, self.y0 * d, self.x1 * d, self.y1 * d)
    }

    /// The interior of `coord` in pixels of its own level.
    pub fn of_tile(coord: TileCoord, level_extent: Extent) -> Rect {
        let (ox, oy) = coord.pixel_origin(TILE_SIZE);
        let x1 = (ox + TILE_SIZE).min(level_extent.width);
        let y1 = (oy + TILE_SIZE).min(level_extent.height);
        Rect::new(i64::from(ox), i64::from(oy), i64::from(x1), i64::from(y1))
    }
}

/// 2-D affine map `x' = a·x + b·y + c`, `y' = d·x + e·y + f`.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Affine {
    /// Row-major coefficients `[a, b, c, d, e, f]`.
    pub m: [f64; 6],
}

impl Default for Affine {
    fn default() -> Self {
        Self::IDENTITY
    }
}

impl Affine {
    /// The identity map.
    pub const IDENTITY: Affine = Affine {
        m: [1.0, 0.0, 0.0, 0.0, 1.0, 0.0],
    };

    /// Scale then translate.
    pub fn scale_translate(sx: f64, sy: f64, tx: f64, ty: f64) -> Self {
        Self {
            m: [sx, 0.0, tx, 0.0, sy, ty],
        }
    }

    /// Maps a point.
    pub fn apply(&self, x: f64, y: f64) -> (f64, f64) {
        let m = &self.m;
        (m[0] * x + m[1] * y + m[2], m[3] * x + m[4] * y + m[5])
    }

    /// Determinant of the linear part.
    pub fn det(&self) -> f64 {
        self.m[0] * self.m[4] - self.m[1] * self.m[3]
    }

    /// The inverse map, or `None` when singular or non-finite.
    pub fn inverse(&self) -> Option<Affine> {
        let det = self.det();
        if !det.is_finite() || det.abs() < 1e-12 || self.m.iter().any(|v| !v.is_finite()) {
            return None;
        }
        let [a, b, c, d, e, f] = self.m;
        let (ia, ib, id, ie) = (e / det, -b / det, -d / det, a / det);
        Some(Affine {
            m: [ia, ib, -(ia * c + ib * f), id, ie, -(id * c + ie * f)],
        })
    }

    /// Integer bounding box of the image of `r` (rounded outward).
    pub fn map_rect(&self, r: &Rect) -> Rect {
        let pts = [
            self.apply(r.x0 as f64, r.y0 as f64),
            self.apply(r.x1 as f64, r.y0 as f64),
            self.apply(r.x0 as f64, r.y1 as f64),
            self.apply(r.x1 as f64, r.y1 as f64),
        ];
        let (mut x0, mut y0, mut x1, mut y1) = (f64::MAX, f64::MAX, f64::MIN, f64::MIN);
        for (x, y) in pts {
            x0 = x0.min(x);
            y0 = y0.min(y);
            x1 = x1.max(x);
            y1 = y1.max(y);
        }
        Rect::new(
            x0.floor() as i64,
            y0.floor() as i64,
            x1.ceil() as i64,
            y1.ceil() as i64,
        )
    }
}

static REV_CLOCK: AtomicU64 = AtomicU64::new(1);
static KEY_CLOCK: AtomicU64 = AtomicU64::new(1);

/// Allocates a fresh revision. Revisions are process-wide and strictly
/// increasing, which is what makes "maximum revision over a footprint" a
/// sound cache stamp (see COMPOSITOR.md §5).
pub(crate) fn next_rev() -> u64 {
    REV_CLOCK.fetch_add(1, Ordering::Relaxed)
}

/// Makes sure future revisions exceed `rev` (after loading a file).
pub(crate) fn observe_rev(rev: u64) {
    REV_CLOCK.fetch_max(rev.saturating_add(1), Ordering::Relaxed);
}

/// A process-unique key for a document instance (cache namespace).
pub(crate) fn next_doc_key() -> u64 {
    KEY_CLOCK.fetch_add(1, Ordering::Relaxed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rect_levels() {
        let r = Rect::new(5, 7, 9, 8);
        assert_eq!(r.to_level(2), Rect::new(1, 1, 3, 2));
        assert_eq!(Rect::new(1, 1, 3, 2).to_level0(2), Rect::new(4, 4, 12, 8));
        assert!(Rect::new(0, 0, 0, 5).is_empty());
        assert_eq!(r.union(&Rect::default()), r);
    }

    #[test]
    fn affine_inverse() {
        let t = Affine {
            m: [2.0, 0.5, 3.0, -0.25, 1.5, -7.0],
        };
        let i = t.inverse().unwrap();
        let (x, y) = t.apply(11.0, -4.0);
        let (u, v) = i.apply(x, y);
        assert!((u - 11.0).abs() < 1e-9 && (v + 4.0).abs() < 1e-9);
    }
}
