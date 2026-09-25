use anyhow::{Result, ensure};
use engine_api::tile::{Tile, TileCoord, TileFormat, TileLayout};
use half::f16;

/// Owned contiguous input, with explicit dimensions (an empty shape is a scalar).
#[derive(Clone, Debug)]
pub enum TensorInput {
    F32 { shape: Vec<usize>, data: Vec<f32> },
    I64 { shape: Vec<usize>, data: Vec<i64> },
}

/// A named output in model declaration order, converted to contiguous fp32.
#[derive(Clone, Debug)]
pub struct TensorOutput {
    pub name: String,
    pub shape: Vec<usize>,
    pub data: Vec<f32>,
}

/// Owned, contiguous NCHW image tensor (one image, no implicit normalization).
#[derive(Clone, Debug)]
pub struct Tensor {
    shape: [usize; 4],
    data: Vec<f32>,
}

impl Tensor {
    pub fn new(channels: usize, height: usize, width: usize, data: Vec<f32>) -> Result<Self> {
        let len = channels
            .checked_mul(height)
            .and_then(|n| n.checked_mul(width));
        ensure!(
            channels > 0 && height > 0 && width > 0 && len == Some(data.len()),
            "invalid tensor shape/length"
        );
        Ok(Self {
            shape: [1, channels, height, width],
            data,
        })
    }
    pub fn shape(&self) -> [usize; 4] {
        self.shape
    }
    pub fn data(&self) -> &[f32] {
        &self.data
    }
    pub fn to_f16(&self) -> Vec<f16> {
        self.data.iter().copied().map(f16::from_f32).collect()
    }
    pub fn from_f16(channels: usize, height: usize, width: usize, data: &[f16]) -> Result<Self> {
        Self::new(
            channels,
            height,
            width,
            data.iter().map(|v| v.to_f32()).collect(),
        )
    }
    /// Includes the tile's halo in the tensor's spatial dimensions.
    pub fn from_tile(tile: &Tile) -> Result<Self> {
        let l = tile.layout();
        let data = match tile.format() {
            TileFormat::F32Planar => tile.samples::<f32>()?.to_vec(),
            TileFormat::F16Planar => tile.samples::<f16>()?.iter().map(|v| v.to_f32()).collect(),
            _ => anyhow::bail!("ML input requires float planes"),
        };
        Self::new(l.channels as usize, l.rows(), l.stride(), data)
    }
    pub fn to_tile(
        &self,
        coord: TileCoord,
        layout: TileLayout,
        format: TileFormat,
    ) -> Result<Tile> {
        ensure!(
            self.shape == [1, layout.channels as usize, layout.rows(), layout.stride()],
            "tile/tensor shape mismatch"
        );
        Ok(match format {
            TileFormat::F32Planar => Tile::from_samples(coord, layout, self.data.clone())?,
            TileFormat::F16Planar => Tile::from_samples(coord, layout, self.to_f16())?,
            _ => anyhow::bail!("ML output requires float planes"),
        })
    }
}
