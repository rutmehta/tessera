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

/// Options for [`ResidentBatch::local_tone`].
#[derive(Debug, Clone, Copy)]
pub struct LocalToneOptions {
    /// Preview levels (above zero) may use the backend's documented,
    /// error-bounded downsampled-guidance approximation. Level 0 never does.
    pub preview: bool,
    /// Identity of the Dehaze input (image, upstream chain, basic tone,
    /// Texture/Clarity, level extent). Dehaze-only edits reuse exact global
    /// airlight/confidence statistics stored under this key.
    pub statistics_key: MemoKey,
}

/// A render transaction. Dropping it before `finish` must discard pending work
/// and must not publish uninitialized cache entries. Cache hits may outlive LRU eviction.
pub trait ResidentBatch {
    /// A sensor dependency chunk has retired. Export backends may submit
    /// pending work and release scratch without materializing any pixels.
    fn checkpoint(&mut self, cancel: &CancellationToken) -> EngineResult<()> {
        cancel.check()
    }
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
    /// Whether a whole level of `frame`, padded by `halo`, can be processed
    /// as one tile ([`ResidentBatch::gather_level`], [`ResidentBatch::crop`]).
    fn supports_level(&self, _frame: Extent, _halo: u16) -> bool {
        false
    }
    /// Assembles all of `frame` (the level of `coord`) padded by `halo` from
    /// halo-free pyramid tiles into one tile at `coord`; edges replicate.
    fn gather_level(
        &mut self,
        _frame: Extent,
        _coord: TileCoord,
        _halo: u16,
        _tiles: &HashMap<TileCoord, ResidentTile>,
    ) -> EngineResult<ResidentTile> {
        Err(engine_api::EngineError::Unsupported {
            what: "resident whole-level tiles".into(),
        })
    }
    /// Copies `extent` at `origin` of a halo-free tile into a new halo-free
    /// tile addressed `coord` (splits a level tile into pyramid tiles).
    fn crop(
        &mut self,
        _tile: &ResidentTile,
        _coord: TileCoord,
        _origin: (u32, u32),
        _extent: Extent,
    ) -> EngineResult<ResidentTile> {
        Err(engine_api::EngineError::Unsupported {
            what: "resident whole-level tiles".into(),
        })
    }
    /// Whether [`ResidentBatch::local_tone`] can process a level of `frame`.
    fn supports_local_tone(&self, _frame: Extent) -> bool {
        false
    }
    /// Whole-level Texture/Clarity/Dehaze barrier on halo-free post-Tone tiles
    /// covering `frame`; returns halo-free tiles for `outputs`. Curves are not
    /// applied (they remain in the fused point chain).
    fn local_tone(
        &mut self,
        _settings: &engine_api::recipe::settings::ToneSettings,
        _frame: Extent,
        _tiles: &HashMap<TileCoord, ResidentTile>,
        _outputs: &[TileCoord],
        _options: &LocalToneOptions,
    ) -> EngineResult<HashMap<TileCoord, ResidentTile>> {
        Err(engine_api::EngineError::Unsupported {
            what: "resident local tone".into(),
        })
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
    /// Lateral CA on halo-padded demosaiced camera RGB in the sensor frame
    /// (`frame`): channels 0 and 2 are resampled bilinearly at the plan's
    /// radial scale; returns the halo-free interior. The halo must cover the
    /// plan's largest displacement plus the bilinear support.
    fn lateral_ca(
        &mut self,
        _tile: &ResidentTile,
        _frame: Extent,
        _plan: &pipeline_cpu::CaPlan,
    ) -> EngineResult<ResidentTile> {
        Err(engine_api::EngineError::Unsupported {
            what: "resident lateral CA".into(),
        })
    }
    /// Vignetting gains on a halo-free scene tile of an active-area level of
    /// size `frame` (normalized coordinates span that frame).
    fn lens_gain(
        &mut self,
        _tile: &ResidentTile,
        _frame: Extent,
        _plan: &pipeline_cpu::VignettePlan,
    ) -> EngineResult<ResidentTile> {
        Err(engine_api::EngineError::Unsupported {
            what: "resident vignetting".into(),
        })
    }
    /// Composed inverse map with normalized Lanczos-3: output rows
    /// `rows` (of `output`) from the halo-free tiles of `frame` that cover
    /// input rows `source` (`[first, end)`). Returns one halo-free tile of
    /// `output.width` × `rows.len()` addressed `coord`.
    #[allow(clippy::too_many_arguments)]
    fn remap(
        &mut self,
        _frame: Extent,
        _tiles: &HashMap<TileCoord, ResidentTile>,
        _source: (u32, u32),
        _plan: &pipeline_cpu::MapPlan,
        _output: Extent,
        _rows: std::ops::Range<u32>,
        _coord: TileCoord,
    ) -> EngineResult<ResidentTile> {
        Err(engine_api::EngineError::Unsupported {
            what: "resident lens/geometry map".into(),
        })
    }
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
