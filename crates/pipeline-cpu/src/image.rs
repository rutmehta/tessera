use engine_api::{
    EngineError, EngineResult,
    tile::{Extent, MAX_HALO, Pyramid, TILE_SIZE, Tile, TileCoord, TileLayout},
};

/// Small planar image adapter for tests and the synchronous reference renderer.
/// Real operators consume/produce engine-api Tiles, not this storage type.
#[derive(Clone, Debug)]
pub struct Image {
    width: u32,
    height: u32,
    planes: Vec<Vec<f32>>,
}
impl Image {
    pub fn new(width: u32, height: u32, planes: Vec<Vec<f32>>) -> EngineResult<Self> {
        let n = u64::from(width) * u64::from(height);
        if width == 0
            || height == 0
            || !matches!(planes.len(), 1 | 3)
            || planes
                .iter()
                .any(|p| p.len() as u64 != n || p.iter().any(|v| !v.is_finite()))
        {
            return Err(EngineError::invalid(
                "image",
                "nonempty finite one/three-plane image required",
            ));
        }
        Ok(Self {
            width,
            height,
            planes,
        })
    }
    pub fn width(&self) -> u32 {
        self.width
    }
    pub fn height(&self) -> u32 {
        self.height
    }
    pub fn planes(&self) -> &[Vec<f32>] {
        &self.planes
    }
    pub(crate) fn blank(width: u32, height: u32, channels: usize) -> Self {
        Self {
            width,
            height,
            planes: vec![vec![0.0; width as usize * height as usize]; channels],
        }
    }
    pub fn from_pyramid(p: &impl Pyramid) -> EngineResult<Self> {
        let e = p.extent();
        if e.width == 0
            || e.height == 0
            || !matches!(p.channels(), 1 | 3)
            || p.tile_size() != TILE_SIZE
        {
            return Err(EngineError::invalid(
                "pyramid",
                "unsupported extent, channels or tile size",
            ));
        }
        let mut out = Self::blank(e.width, e.height, p.channels() as usize);
        for coord in out.coords() {
            out.put(&p.tile(coord)?)?;
        }
        Ok(out)
    }
    pub fn coords(&self) -> impl Iterator<Item = TileCoord> + use<> {
        let nx = self.width.div_ceil(TILE_SIZE);
        let ny = self.height.div_ceil(TILE_SIZE);
        (0..ny).flat_map(move |y| (0..nx).map(move |x| TileCoord::new(0, x, y)))
    }
    /// Gather real neighbours across tile seams. Outside the image, clamp to
    /// the closest coordinate with the same CFA phase (period 2/6; RGB: 1).
    pub fn tile(&self, coord: TileCoord, halo: u16, period: u32) -> EngineResult<Tile> {
        if coord.level != 0
            || coord.x >= self.width.div_ceil(TILE_SIZE)
            || coord.y >= self.height.div_ceil(TILE_SIZE)
            || halo > MAX_HALO
            || !matches!(period, 1 | 2 | 6)
        {
            return Err(EngineError::invalid(
                "tile",
                "invalid coordinate, halo or CFA period",
            ));
        }
        let (ox, oy) = coord.pixel_origin(TILE_SIZE);
        let layout = TileLayout {
            extent: Extent::new(
                (self.width - ox).min(TILE_SIZE),
                (self.height - oy).min(TILE_SIZE),
            ),
            halo,
            channels: self.planes.len() as u8,
        };
        let clamp = |v: i64, n: u32| -> usize {
            if v >= 0 && v < i64::from(n) {
                return v as usize;
            }
            let phase = v.rem_euclid(i64::from(period)) as u32;
            if phase >= n {
                return v.clamp(0, i64::from(n) - 1) as usize;
            }
            if v < 0 {
                phase as usize
            } else {
                (phase + (n - 1 - phase) / period * period) as usize
            }
        };
        let mut data = Vec::with_capacity(layout.len());
        for plane in &self.planes {
            for y in 0..layout.rows() {
                for x in 0..layout.stride() {
                    let sx = clamp(i64::from(ox) + x as i64 - i64::from(halo), self.width);
                    let sy = clamp(i64::from(oy) + y as i64 - i64::from(halo), self.height);
                    data.push(plane[sy * self.width as usize + sx]);
                }
            }
        }
        Tile::from_samples(coord, layout, data)
    }
    pub fn put(&mut self, tile: &Tile) -> EngineResult<()> {
        let (ox, oy) = tile.coord().pixel_origin(TILE_SIZE);
        let l = tile.layout();
        if tile.coord().level != 0
            || l.channels as usize != self.planes.len()
            || u64::from(ox) + u64::from(l.extent.width) > u64::from(self.width)
            || u64::from(oy) + u64::from(l.extent.height) > u64::from(self.height)
        {
            return Err(EngineError::invalid("tile", "tile does not fit image"));
        }
        let samples = tile.samples::<f32>()?;
        for (c, plane) in self.planes.iter_mut().enumerate() {
            for y in 0..l.extent.height {
                let src = l.index(c as u8, 0, y as i32).unwrap();
                let dst = (oy + y) as usize * self.width as usize + ox as usize;
                plane[dst..dst + l.extent.width as usize]
                    .copy_from_slice(&samples[src..src + l.extent.width as usize]);
            }
        }
        Ok(())
    }
    /// Box-filter scene-linear RGB over an active-area crop before display.
    pub fn downsample_crop(&self, crop: [u32; 4], scale: u32) -> EngineResult<Self> {
        let [left, top, w, h] = crop;
        if scale == 0
            || w == 0
            || h == 0
            || u64::from(left) + u64::from(w) > u64::from(self.width)
            || u64::from(top) + u64::from(h) > u64::from(self.height)
        {
            return Err(EngineError::invalid(
                "crop/scale",
                "invalid crop or zero scale",
            ));
        }
        let mut out = Self::blank(w.div_ceil(scale), h.div_ceil(scale), self.planes.len());
        for (src, dst) in self.planes.iter().zip(&mut out.planes) {
            for y in 0..out.height {
                for x in 0..out.width {
                    let x0 = x * scale;
                    let y0 = y * scale;
                    let x1 = x0.saturating_add(scale).min(w);
                    let y1 = y0.saturating_add(scale).min(h);
                    let mut sum = 0.0;
                    for sy in y0..y1 {
                        for sx in x0..x1 {
                            sum += src
                                [(top + sy) as usize * self.width as usize + (left + sx) as usize];
                        }
                    }
                    dst[y as usize * out.width as usize + x as usize] =
                        sum / ((x1 - x0) * (y1 - y0)) as f32;
                }
            }
        }
        Ok(out)
    }
}
