//! Tiled, planar, copy-on-write image buffers and pyramid addressing.
//!
//! Images are cut into square tiles of [`TILE_SIZE`] pixels at every pyramid
//! level. Level 0 is full resolution; level `n` is downsampled by `2^n`. A
//! tile stores each channel as a separate plane and carries a halo of extra
//! pixels on every side so neighbourhood operators can run without fetching
//! neighbours. Tile payloads are reference counted: cloning a [`Tile`] is
//! O(1) and mutation copies only when the buffer is shared.

use std::fmt;
use std::sync::Arc;

use half::f16;
use serde::{Deserialize, Serialize};

use crate::error::{EngineError, EngineResult};

/// Edge length, in pixels, of the interior of every tile.
pub const TILE_SIZE: u32 = 256;

/// Largest halo the engine allocates; operators needing a larger support
/// must fetch neighbouring tiles explicitly.
pub const MAX_HALO: u16 = 32;

/// Width and height in pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub struct Extent {
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
}

impl Extent {
    /// Creates an extent.
    pub const fn new(width: u32, height: u32) -> Self {
        Self { width, height }
    }

    /// Pixel count.
    pub const fn area(self) -> u64 {
        self.width as u64 * self.height as u64
    }

    /// Extent of this image at pyramid `level` (each level halves, rounding
    /// up, never below 1×1).
    pub fn at_level(self, level: u8) -> Self {
        let shrink = |v: u32| -> u32 {
            if level >= 32 {
                return 1;
            }
            let d = 1u64 << level;
            (u64::from(v).div_ceil(d)).max(1) as u32
        };
        Self::new(shrink(self.width), shrink(self.height))
    }

    /// Number of tiles across and down at `tile_size`.
    pub fn tile_grid(self, tile_size: u32) -> (u32, u32) {
        (
            self.width.div_ceil(tile_size),
            self.height.div_ceil(tile_size),
        )
    }

    /// Number of pyramid levels needed until the whole image fits in one
    /// tile (always at least 1).
    pub fn level_count(self, tile_size: u32) -> u8 {
        let mut levels = 1u8;
        while {
            let e = self.at_level(levels - 1);
            e.width > tile_size || e.height > tile_size
        } {
            levels += 1;
        }
        levels
    }
}

/// Address of one tile in an image pyramid.
///
/// Ordering is `(level, y, x)`, i.e. coarse levels first, then raster order,
/// which is the order progressive renderers want.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TileCoord {
    /// Pyramid level; 0 is full resolution.
    pub level: u8,
    /// Column, in tiles.
    pub x: u32,
    /// Row, in tiles.
    pub y: u32,
}

impl Ord for TileCoord {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        (self.level, self.y, self.x).cmp(&(other.level, other.y, other.x))
    }
}

impl PartialOrd for TileCoord {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl TileCoord {
    /// Creates a coordinate.
    pub const fn new(level: u8, x: u32, y: u32) -> Self {
        Self { level, x, y }
    }

    /// Top-left pixel of the tile interior at its own level.
    pub const fn pixel_origin(self, tile_size: u32) -> (u32, u32) {
        (self.x * tile_size, self.y * tile_size)
    }

    /// The tile one level coarser that contains this one.
    pub const fn parent(self) -> Self {
        Self::new(self.level + 1, self.x / 2, self.y / 2)
    }

    /// The (up to) four tiles one level finer covered by this one, or `None`
    /// at level 0. Callers clip against the finer level's tile grid.
    pub const fn children(self) -> Option<[Self; 4]> {
        if self.level == 0 {
            return None;
        }
        let (l, x, y) = (self.level - 1, self.x * 2, self.y * 2);
        Some([
            Self::new(l, x, y),
            Self::new(l, x + 1, y),
            Self::new(l, x, y + 1),
            Self::new(l, x + 1, y + 1),
        ])
    }
}

impl fmt::Display for TileCoord {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "L{}/{},{}", self.level, self.x, self.y)
    }
}

/// Sample storage format of a tile. All formats are planar.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TileFormat {
    /// 32-bit float; the only format used for in-flight pipeline math.
    F32Planar,
    /// 16-bit float; allowed for cached stage outputs only.
    F16Planar,
    /// 16-bit unsigned integer; layered documents and raw CFA data.
    U16,
    /// 8-bit unsigned integer; layered documents, masks, thumbnails.
    U8,
}

impl TileFormat {
    /// Bytes per sample.
    pub const fn bytes_per_sample(self) -> usize {
        match self {
            Self::F32Planar => 4,
            Self::F16Planar | Self::U16 => 2,
            Self::U8 => 1,
        }
    }

    /// True for floating-point formats (unbounded, scene-referred values allowed).
    pub const fn is_float(self) -> bool {
        matches!(self, Self::F32Planar | Self::F16Planar)
    }
}

/// A sample type that can live in a [`Tile`]. Sealed: exactly the four
/// [`TileFormat`] types implement it.
pub trait Sample: Copy + Default + Send + Sync + 'static + sealed::Sealed {
    /// The format this type stores as.
    const FORMAT: TileFormat;
    #[doc(hidden)]
    fn slice(buffer: &TileBuffer) -> Option<&[Self]>;
    #[doc(hidden)]
    fn slice_mut(buffer: &mut TileBuffer) -> Option<&mut [Self]>;
    #[doc(hidden)]
    fn wrap(data: Vec<Self>) -> TileBuffer;
}

mod sealed {
    pub trait Sealed {}
    impl Sealed for f32 {}
    impl Sealed for half::f16 {}
    impl Sealed for u16 {}
    impl Sealed for u8 {}
}

/// Reference-counted sample storage behind a [`Tile`].
#[derive(Clone)]
pub enum TileBuffer {
    /// f32 samples.
    F32(Arc<Vec<f32>>),
    /// f16 samples.
    F16(Arc<Vec<f16>>),
    /// u16 samples.
    U16(Arc<Vec<u16>>),
    /// u8 samples.
    U8(Arc<Vec<u8>>),
}

impl TileBuffer {
    fn zeroed(format: TileFormat, len: usize) -> Self {
        match format {
            TileFormat::F32Planar => Self::F32(Arc::new(vec![0.0; len])),
            TileFormat::F16Planar => Self::F16(Arc::new(vec![f16::ZERO; len])),
            TileFormat::U16 => Self::U16(Arc::new(vec![0; len])),
            TileFormat::U8 => Self::U8(Arc::new(vec![0; len])),
        }
    }

    fn format(&self) -> TileFormat {
        match self {
            Self::F32(_) => TileFormat::F32Planar,
            Self::F16(_) => TileFormat::F16Planar,
            Self::U16(_) => TileFormat::U16,
            Self::U8(_) => TileFormat::U8,
        }
    }

    fn len(&self) -> usize {
        match self {
            Self::F32(v) => v.len(),
            Self::F16(v) => v.len(),
            Self::U16(v) => v.len(),
            Self::U8(v) => v.len(),
        }
    }

    fn ptr_eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::F32(a), Self::F32(b)) => Arc::ptr_eq(a, b),
            (Self::F16(a), Self::F16(b)) => Arc::ptr_eq(a, b),
            (Self::U16(a), Self::U16(b)) => Arc::ptr_eq(a, b),
            (Self::U8(a), Self::U8(b)) => Arc::ptr_eq(a, b),
            _ => false,
        }
    }

    fn strong_count(&self) -> usize {
        match self {
            Self::F32(v) => Arc::strong_count(v),
            Self::F16(v) => Arc::strong_count(v),
            Self::U16(v) => Arc::strong_count(v),
            Self::U8(v) => Arc::strong_count(v),
        }
    }
}

macro_rules! impl_sample {
    ($t:ty, $variant:ident, $format:expr) => {
        impl Sample for $t {
            const FORMAT: TileFormat = $format;
            fn slice(buffer: &TileBuffer) -> Option<&[Self]> {
                match buffer {
                    TileBuffer::$variant(v) => Some(v.as_slice()),
                    _ => None,
                }
            }
            fn slice_mut(buffer: &mut TileBuffer) -> Option<&mut [Self]> {
                match buffer {
                    TileBuffer::$variant(v) => Some(Arc::make_mut(v).as_mut_slice()),
                    _ => None,
                }
            }
            fn wrap(data: Vec<Self>) -> TileBuffer {
                TileBuffer::$variant(Arc::new(data))
            }
        }
    };
}

impl_sample!(f32, F32, TileFormat::F32Planar);
impl_sample!(f16, F16, TileFormat::F16Planar);
impl_sample!(u16, U16, TileFormat::U16);
impl_sample!(u8, U8, TileFormat::U8);

/// Geometry of a tile's planar buffer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TileLayout {
    /// Interior size (≤ [`TILE_SIZE`]; edge tiles may be smaller).
    pub extent: Extent,
    /// Halo width in pixels on every side.
    pub halo: u16,
    /// Number of channel planes.
    pub channels: u8,
}

impl TileLayout {
    /// Samples per row including both halos.
    pub const fn stride(&self) -> usize {
        self.extent.width as usize + 2 * self.halo as usize
    }

    /// Rows per plane including both halos.
    pub const fn rows(&self) -> usize {
        self.extent.height as usize + 2 * self.halo as usize
    }

    /// Samples per plane.
    pub const fn plane_len(&self) -> usize {
        self.stride() * self.rows()
    }

    /// Total samples.
    pub const fn len(&self) -> usize {
        self.plane_len() * self.channels as usize
    }

    /// True if the layout holds no samples.
    pub const fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Index of the sample at interior-relative `(x, y)` in `channel`.
    /// Coordinates may be negative or exceed the interior by up to the halo.
    pub fn index(&self, channel: u8, x: i32, y: i32) -> Option<usize> {
        let h = i64::from(self.halo);
        let (px, py) = (i64::from(x) + h, i64::from(y) + h);
        if channel >= self.channels
            || px < 0
            || py < 0
            || px >= self.stride() as i64
            || py >= self.rows() as i64
        {
            return None;
        }
        Some(channel as usize * self.plane_len() + py as usize * self.stride() + px as usize)
    }
}

/// One tile of one image at one pyramid level: planar samples plus halo.
///
/// Cloning is O(1) and shares the buffer; the first mutable access on a
/// shared tile copies it (copy-on-write). This is what makes history, virtual
/// copies and cache hand-off cheap.
#[derive(Clone)]
pub struct Tile {
    coord: TileCoord,
    layout: TileLayout,
    buffer: TileBuffer,
}

impl Tile {
    /// Allocates a zero-filled tile.
    pub fn zeroed(coord: TileCoord, format: TileFormat, layout: TileLayout) -> EngineResult<Self> {
        Self::check_layout(&layout)?;
        Ok(Self {
            coord,
            layout,
            buffer: TileBuffer::zeroed(format, layout.len()),
        })
    }

    /// Wraps existing planar samples. `data.len()` must equal `layout.len()`.
    pub fn from_samples<T: Sample>(
        coord: TileCoord,
        layout: TileLayout,
        data: Vec<T>,
    ) -> EngineResult<Self> {
        Self::check_layout(&layout)?;
        if data.len() != layout.len() {
            return Err(EngineError::invalid(
                "data",
                format!(
                    "expected {} samples for {:?}, got {}",
                    layout.len(),
                    layout,
                    data.len()
                ),
            ));
        }
        Ok(Self {
            coord,
            layout,
            buffer: T::wrap(data),
        })
    }

    fn check_layout(layout: &TileLayout) -> EngineResult<()> {
        if layout.extent.width == 0 || layout.extent.height == 0 {
            return Err(EngineError::invalid(
                "extent",
                "tile interior must be non-empty",
            ));
        }
        if layout.extent.width > TILE_SIZE || layout.extent.height > TILE_SIZE {
            return Err(EngineError::invalid(
                "extent",
                format!("tile interior exceeds {TILE_SIZE}"),
            ));
        }
        if layout.halo > MAX_HALO {
            return Err(EngineError::invalid(
                "halo",
                format!("halo exceeds {MAX_HALO}"),
            ));
        }
        if layout.channels == 0 {
            return Err(EngineError::invalid(
                "channels",
                "at least one channel required",
            ));
        }
        Ok(())
    }

    /// Pyramid address.
    pub fn coord(&self) -> TileCoord {
        self.coord
    }

    /// Buffer geometry.
    pub fn layout(&self) -> TileLayout {
        self.layout
    }

    /// Sample format.
    pub fn format(&self) -> TileFormat {
        self.buffer.format()
    }

    /// Halo width in pixels.
    pub fn halo(&self) -> u16 {
        self.layout.halo
    }

    /// All samples, planar, if `T` matches the tile's format.
    pub fn samples<T: Sample>(&self) -> EngineResult<&[T]> {
        T::slice(&self.buffer).ok_or_else(|| self.format_mismatch(T::FORMAT))
    }

    /// Mutable samples; copies the buffer first if it is shared.
    pub fn samples_mut<T: Sample>(&mut self) -> EngineResult<&mut [T]> {
        let format = self.format();
        let coord = self.coord;
        T::slice_mut(&mut self.buffer).ok_or_else(|| {
            EngineError::invalid(
                "sample type",
                format!("tile {coord} is {format:?}, requested {:?}", T::FORMAT),
            )
        })
    }

    /// One channel plane (halo included).
    pub fn plane<T: Sample>(&self, channel: u8) -> EngineResult<&[T]> {
        let len = self.layout.plane_len();
        let start = self.checked_plane_start(channel)?;
        Ok(&self.samples::<T>()?[start..start + len])
    }

    /// Mutable channel plane; copy-on-write.
    pub fn plane_mut<T: Sample>(&mut self, channel: u8) -> EngineResult<&mut [T]> {
        let len = self.layout.plane_len();
        let start = self.checked_plane_start(channel)?;
        Ok(&mut self.samples_mut::<T>()?[start..start + len])
    }

    fn checked_plane_start(&self, channel: u8) -> EngineResult<usize> {
        if channel >= self.layout.channels {
            return Err(EngineError::invalid(
                "channel",
                format!("{channel} ≥ {}", self.layout.channels),
            ));
        }
        Ok(channel as usize * self.layout.plane_len())
    }

    /// True if both tiles share the same underlying buffer.
    pub fn shares_buffer_with(&self, other: &Tile) -> bool {
        self.buffer.ptr_eq(&other.buffer)
    }

    /// True if no other `Tile` references this buffer (mutation will not copy).
    pub fn is_unique(&self) -> bool {
        self.buffer.strong_count() == 1
    }

    /// Payload size in bytes.
    pub fn byte_len(&self) -> usize {
        self.buffer.len() * self.format().bytes_per_sample()
    }

    fn format_mismatch(&self, requested: TileFormat) -> EngineError {
        EngineError::invalid(
            "sample type",
            format!(
                "tile {} is {:?}, requested {:?}",
                self.coord,
                self.format(),
                requested
            ),
        )
    }
}

impl fmt::Debug for Tile {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Tile")
            .field("coord", &self.coord)
            .field("format", &self.format())
            .field("layout", &self.layout)
            .finish_non_exhaustive()
    }
}

/// A multi-resolution tiled image.
///
/// Implemented by decoded sources, cached stage outputs, layer rasters and
/// masks. Implementations may render tiles lazily; `tile` may block, so call
/// it from a job, never from the UI thread.
pub trait Pyramid: Send + Sync {
    /// Full-resolution (level 0) size.
    fn extent(&self) -> Extent;

    /// Sample format of every tile.
    fn format(&self) -> TileFormat;

    /// Channel count of every tile.
    fn channels(&self) -> u8;

    /// Halo width of every tile.
    fn halo(&self) -> u16;

    /// Interior tile edge; defaults to [`TILE_SIZE`].
    fn tile_size(&self) -> u32 {
        TILE_SIZE
    }

    /// Number of levels; defaults to "until the image fits in one tile".
    fn level_count(&self) -> u8 {
        self.extent().level_count(self.tile_size())
    }

    /// Size at `level`.
    fn level_extent(&self, level: u8) -> Extent {
        self.extent().at_level(level)
    }

    /// Whether `coord` addresses a tile inside the pyramid.
    fn contains(&self, coord: TileCoord) -> bool {
        if coord.level >= self.level_count() {
            return false;
        }
        let (cols, rows) = self.level_extent(coord.level).tile_grid(self.tile_size());
        coord.x < cols && coord.y < rows
    }

    /// Interior extent of the tile at `coord` (edge tiles are clipped).
    fn tile_extent(&self, coord: TileCoord) -> Extent {
        let e = self.level_extent(coord.level);
        let ts = self.tile_size();
        let (ox, oy) = coord.pixel_origin(ts);
        Extent::new(
            e.width.saturating_sub(ox).min(ts),
            e.height.saturating_sub(oy).min(ts),
        )
    }

    /// Produces the tile at `coord`. Returns [`EngineError::InvalidArgument`]
    /// for coordinates outside the pyramid.
    fn tile(&self, coord: TileCoord) -> EngineResult<Tile>;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn layout(w: u32, h: u32, halo: u16, channels: u8) -> TileLayout {
        TileLayout {
            extent: Extent::new(w, h),
            halo,
            channels,
        }
    }

    #[test]
    fn extent_levels() {
        let e = Extent::new(8192, 5464); // 45 MP
        assert_eq!(e.at_level(1), Extent::new(4096, 2732));
        assert_eq!(e.at_level(3), Extent::new(1024, 683));
        assert_eq!(e.level_count(256), 6); // 8192/32 = 256
        assert_eq!(Extent::new(10, 10).level_count(256), 1);
        assert_eq!(Extent::new(257, 1).level_count(256), 2);
        assert_eq!(e.tile_grid(256), (32, 22));
        assert_eq!(Extent::new(3, 3).at_level(40), Extent::new(1, 1));
    }

    #[test]
    fn coord_parent_children() {
        let c = TileCoord::new(2, 5, 3);
        assert_eq!(c.parent(), TileCoord::new(3, 2, 1));
        let kids = c.children().unwrap();
        assert!(kids.iter().all(|k| k.parent() == c));
        assert!(TileCoord::new(0, 0, 0).children().is_none());
        assert!(TileCoord::new(1, 9, 0) < TileCoord::new(1, 0, 1));
        assert!(TileCoord::new(0, 9, 9) < TileCoord::new(1, 0, 0));
    }

    #[test]
    fn copy_on_write() {
        let mut a = Tile::zeroed(
            TileCoord::new(0, 0, 0),
            TileFormat::F32Planar,
            layout(4, 4, 1, 3),
        )
        .unwrap();
        assert_eq!(a.samples::<f32>().unwrap().len(), 6 * 6 * 3);
        let b = a.clone();
        assert!(a.shares_buffer_with(&b));
        assert!(!a.is_unique());
        a.plane_mut::<f32>(1).unwrap()[0] = 1.0;
        assert!(!a.shares_buffer_with(&b));
        assert!(a.is_unique());
        assert_eq!(a.plane::<f32>(1).unwrap()[0], 1.0);
        assert_eq!(b.plane::<f32>(1).unwrap()[0], 0.0);
    }

    #[test]
    fn format_checks() {
        let t = Tile::zeroed(TileCoord::new(0, 0, 0), TileFormat::U8, layout(2, 2, 0, 1)).unwrap();
        assert!(t.samples::<u8>().is_ok());
        assert!(t.samples::<f32>().is_err());
        assert!(t.plane::<u8>(1).is_err());
        let h = Tile::from_samples(
            TileCoord::new(0, 0, 0),
            layout(1, 1, 0, 2),
            vec![f16::ONE, f16::ZERO],
        )
        .unwrap();
        assert_eq!(h.format(), TileFormat::F16Planar);
        assert_eq!(h.byte_len(), 4);
        assert!(
            Tile::from_samples(TileCoord::new(0, 0, 0), layout(1, 1, 0, 2), vec![0u16]).is_err()
        );
        assert!(Tile::zeroed(
            TileCoord::new(0, 0, 0),
            TileFormat::U8,
            layout(300, 1, 0, 1)
        )
        .is_err());
        assert!(
            Tile::zeroed(TileCoord::new(0, 0, 0), TileFormat::U8, layout(1, 1, 64, 1)).is_err()
        );
    }

    #[test]
    fn halo_indexing() {
        let l = layout(4, 4, 2, 2);
        assert_eq!(l.stride(), 8);
        assert_eq!(l.index(0, -2, -2), Some(0));
        assert_eq!(l.index(0, 0, 0), Some(2 * 8 + 2));
        assert_eq!(l.index(1, 0, 0), Some(64 + 18));
        assert_eq!(l.index(0, 5, 5), Some(63));
        assert_eq!(l.index(0, 6, 0), None);
        assert_eq!(l.index(0, -3, 0), None);
        assert_eq!(l.index(2, 0, 0), None);
    }

    struct Flat(Extent);
    impl Pyramid for Flat {
        fn extent(&self) -> Extent {
            self.0
        }
        fn format(&self) -> TileFormat {
            TileFormat::U8
        }
        fn channels(&self) -> u8 {
            1
        }
        fn halo(&self) -> u16 {
            0
        }
        fn tile(&self, coord: TileCoord) -> EngineResult<Tile> {
            if !self.contains(coord) {
                return Err(EngineError::invalid("coord", coord.to_string()));
            }
            let layout = TileLayout {
                extent: self.tile_extent(coord),
                halo: 0,
                channels: 1,
            };
            Tile::zeroed(coord, TileFormat::U8, layout)
        }
    }

    #[test]
    fn pyramid_defaults() {
        let p: Box<dyn Pyramid> = Box::new(Flat(Extent::new(600, 300)));
        assert_eq!(p.level_count(), 3);
        assert!(p.contains(TileCoord::new(0, 2, 1)));
        assert!(!p.contains(TileCoord::new(0, 3, 0)));
        assert!(!p.contains(TileCoord::new(3, 0, 0)));
        assert_eq!(p.tile_extent(TileCoord::new(0, 2, 1)), Extent::new(88, 44));
        assert_eq!(
            p.tile(TileCoord::new(2, 0, 0)).unwrap().layout().extent,
            Extent::new(150, 75)
        );
        assert!(p.tile(TileCoord::new(0, 9, 9)).is_err());
    }
}
