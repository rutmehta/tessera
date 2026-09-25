//! Backend-owned tiles and a transaction spanning an entire graph render.
use crate::Op;
use engine_api::{
    EngineResult,
    jobs::CancellationToken,
    stage::MemoKey,
    tile::{Extent, Tile, TileCoord, TileLayout},
};
use std::{any::Any, collections::HashMap, sync::Arc};

/// Opaque resident storage. A backend must reject handles from another device.
#[derive(Clone)]
pub struct ResidentTile {
    pub coord: TileCoord,
    pub layout: TileLayout,
    pub storage: Arc<dyn Any + Send + Sync>,
}

/// Display histogram: R, G, B and integer Rec.709 luma, 256 bins each.
pub type DisplayHistogram = [[u32; 256]; 4];

/// Completed render. Surface targets never return pixel tiles.
#[derive(Default)]
pub struct ResidentOutput {
    pub tiles: Vec<Tile>,
    pub histogram: Option<DisplayHistogram>,
}

/// A surface target; only the small histogram may be read back.
#[derive(Clone, Copy)]
pub struct SurfaceTarget {
    pub id: u32,
    pub histogram: bool,
}

/// A render transaction. Dropping it before `finish` must discard pending work
/// and must not publish uninitialized cache entries. Cache hits may outlive LRU eviction.
pub trait ResidentBatch {
    fn cached(&mut self, key: &MemoKey) -> EngineResult<Option<ResidentTile>>;
    fn cache(&mut self, key: MemoKey, tile: &ResidentTile) -> EngineResult<ResidentTile>;
    /// Retain an extra scheduling checkpoint without another precision loss.
    /// Backends without exact memo storage may leave this checkpoint uncached.
    fn cache_exact(&mut self, _key: MemoKey, tile: &ResidentTile) -> EngineResult<ResidentTile> {
        Ok(tile.clone())
    }
    fn upload(&mut self, tile: &Tile) -> EngineResult<ResidentTile>;
    /// Uploads and retains a source in the backend cache. Backends may preserve
    /// Decode samples at full precision while storing computed outputs as f16.
    fn upload_cached(&mut self, key: MemoKey, tile: &Tile) -> EngineResult<ResidentTile> {
        let tile = self.upload(tile)?;
        self.cache(key, &tile)
    }
    /// Detail consumes real RGB neighbours and returns a halo-free interior.
    /// Other operators retain their usual StageOp layout contract.
    fn run(&mut self, op: &Op<'_>, tile: &ResidentTile) -> EngineResult<ResidentTile>;
    /// Adjacent point operators may be fused without a materialized intermediate.
    fn run_chain(&mut self, ops: &[Op<'_>], tile: &ResidentTile) -> EngineResult<ResidentTile> {
        let mut output = tile.clone();
        for op in ops {
            output = self.run(op, &output)?;
        }
        Ok(output)
    }
    /// Gather within `frame` at `coord.level`; RGB uses period 1, sensor CFA
    /// uses its phase period at level 0. Source tiles must be halo-free.
    fn gather(
        &mut self,
        frame: Extent,
        coord: TileCoord,
        halo: u16,
        period: u32,
        tiles: &HashMap<TileCoord, ResidentTile>,
    ) -> EngineResult<ResidentTile>;
    fn resample(
        &mut self,
        crop: [u32; 4],
        coord: TileCoord,
        tiles: &HashMap<TileCoord, ResidentTile>,
        accumulated: Option<ResidentTile>,
    ) -> EngineResult<ResidentTile>;
    /// One final readback for CPU consumers, or direct surface write when a
    /// surface id is provided. Returns no CPU tiles for a surface target.
    fn finish(
        self: Box<Self>,
        tiles: Vec<ResidentTile>,
        display: bool,
        surface: Option<SurfaceTarget>,
        cancel: &CancellationToken,
    ) -> EngineResult<ResidentOutput>;
}
