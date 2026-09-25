//! Byte-budgeted LRU cache of memoized stage-output tiles.

use std::collections::{BTreeMap, HashMap};
use std::sync::Mutex;

use engine_api::id::ImageId;
use engine_api::stage::MemoKey;
use engine_api::tile::{Tile, TileFormat};
use engine_api::{EngineError, EngineResult};
use half::f16;

/// Counters since creation (or the last [`TileCache::clear`]).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CacheStats {
    /// `get` calls that found a tile.
    pub hits: u64,
    /// `get` calls that found nothing.
    pub misses: u64,
    /// Tiles stored.
    pub inserts: u64,
    /// Tiles evicted to stay within the budget.
    pub evictions: u64,
    /// Tiles refused because they alone exceed the budget.
    pub rejected: u64,
    /// Largest resident byte count ever observed.
    pub peak_bytes: usize,
}

struct Entry {
    tile: Tile,
    tick: u64,
}

#[derive(Default)]
struct Lru {
    map: HashMap<MemoKey, Entry>,
    order: BTreeMap<u64, MemoKey>,
    tick: u64,
    bytes: usize,
    stats: CacheStats,
}

impl Lru {
    fn touch(&mut self, key: &MemoKey) -> Option<Tile> {
        self.tick += 1;
        let tick = self.tick;
        let entry = self.map.get_mut(key)?;
        self.order.remove(&entry.tick);
        entry.tick = tick;
        self.order.insert(tick, *key);
        Some(entry.tile.clone())
    }

    fn remove(&mut self, key: &MemoKey) -> Option<Entry> {
        let entry = self.map.remove(key)?;
        self.order.remove(&entry.tick);
        self.bytes -= entry.tile.byte_len();
        Some(entry)
    }
}

/// Memoized stage outputs keyed by [`MemoKey`], evicted least recently used
/// first so that resident payload bytes never exceed the budget.
///
/// The cache stores whatever tile it is given; the renderer stores
/// `F16Planar` tiles (see [`to_f16`]). Tiles are reference counted, so a
/// `get` is O(1) and never copies pixels. Thread-safe.
pub struct TileCache {
    budget: usize,
    lru: Mutex<Lru>,
}

impl TileCache {
    /// A cache holding at most `budget_bytes` of tile payload.
    pub fn new(budget_bytes: usize) -> Self {
        Self {
            budget: budget_bytes,
            lru: Mutex::new(Lru::default()),
        }
    }

    /// Configured payload budget in bytes.
    pub fn budget(&self) -> usize {
        self.budget
    }

    /// Resident payload bytes.
    pub fn bytes(&self) -> usize {
        self.lru.lock().unwrap().bytes
    }

    /// Resident tile count.
    pub fn len(&self) -> usize {
        self.lru.lock().unwrap().map.len()
    }

    /// True when nothing is cached.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Counters.
    pub fn stats(&self) -> CacheStats {
        self.lru.lock().unwrap().stats
    }

    /// Looks up a tile and marks it most recently used.
    pub fn get(&self, key: &MemoKey) -> Option<Tile> {
        let mut lru = self.lru.lock().unwrap();
        let found = lru.touch(key);
        if found.is_some() {
            lru.stats.hits += 1;
        } else {
            lru.stats.misses += 1;
        }
        found
    }

    /// Presence check that neither changes recency nor counts as a lookup.
    pub fn contains(&self, key: &MemoKey) -> bool {
        self.lru.lock().unwrap().map.contains_key(key)
    }

    /// Stores a tile, replacing any tile under the same key, then evicts the
    /// least recently used tiles until the payload fits the budget. A tile
    /// larger than the whole budget is not stored. Returns whether it was.
    pub fn insert(&self, key: MemoKey, tile: Tile) -> bool {
        let size = tile.byte_len();
        let mut lru = self.lru.lock().unwrap();
        lru.remove(&key);
        if size > self.budget {
            lru.stats.rejected += 1;
            return false;
        }
        while lru.bytes + size > self.budget {
            let (_, oldest) = lru.order.pop_first().expect("bytes > 0 implies entries");
            let entry = lru.map.remove(&oldest).expect("order and map agree");
            lru.bytes -= entry.tile.byte_len();
            lru.stats.evictions += 1;
        }
        lru.tick += 1;
        let tick = lru.tick;
        lru.order.insert(tick, key);
        lru.map.insert(key, Entry { tile, tick });
        lru.bytes += size;
        lru.stats.inserts += 1;
        lru.stats.peak_bytes = lru.stats.peak_bytes.max(lru.bytes);
        true
    }

    /// Drops every tile of one image (for example when it is closed).
    pub fn remove_image(&self, image: ImageId) {
        let mut lru = self.lru.lock().unwrap();
        let keys: Vec<_> = lru
            .map
            .keys()
            .filter(|k| k.image_id == image)
            .copied()
            .collect();
        for key in keys {
            lru.remove(&key);
        }
    }

    /// Drops everything and resets the counters.
    pub fn clear(&self) {
        *self.lru.lock().unwrap() = Lru::default();
    }
}

/// Converts an `F32Planar` tile to `F16Planar` for caching. Values beyond
/// the f16 range saturate to ±65504 rather than becoming infinite.
pub fn to_f16(tile: &Tile) -> EngineResult<Tile> {
    let data = tile
        .samples::<f32>()?
        .iter()
        .map(|&v| f16::from_f32(v.clamp(-65504.0, 65504.0)))
        .collect();
    Tile::from_samples(tile.coord(), tile.layout(), data)
}

/// Converts a cached tile back to in-flight `F32Planar` precision.
pub fn to_f32(tile: &Tile) -> EngineResult<Tile> {
    match tile.format() {
        TileFormat::F32Planar => Ok(tile.clone()),
        TileFormat::F16Planar => {
            use half::slice::HalfFloatSliceExt;
            let src = tile.samples::<f16>()?;
            let mut data = vec![0f32; src.len()];
            // Hardware-converted where available; exact either way.
            src.convert_to_f32_slice(&mut data);
            Tile::from_samples(tile.coord(), tile.layout(), data)
        }
        other => Err(EngineError::invalid(
            "cached tile",
            format!("{other:?} is not a floating-point stage output"),
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use engine_api::stage::{ParamHash, StageId};
    use engine_api::tile::{Extent, TileCoord, TileLayout};

    fn key(x: u32) -> MemoKey {
        MemoKey {
            image_id: ImageId(7),
            stage: StageId::Demosaic,
            params_hash: ParamHash::default(),
            tile: TileCoord::new(0, x, 0),
        }
    }

    fn tile(x: u32) -> Tile {
        let layout = TileLayout {
            extent: Extent::new(4, 4),
            halo: 0,
            channels: 3,
        };
        to_f16(&Tile::from_samples(TileCoord::new(0, x, 0), layout, vec![x as f32; 48]).unwrap())
            .unwrap()
    }

    #[test]
    fn lru_eviction_respects_budget_and_recency() {
        let one = tile(0).byte_len();
        assert_eq!(one, 96);
        let cache = TileCache::new(one * 3);
        for x in 0..3 {
            assert!(cache.insert(key(x), tile(x)));
        }
        assert!(cache.get(&key(0)).is_some()); // 1 is now the oldest
        assert!(cache.insert(key(3), tile(3)));
        assert!(cache.contains(&key(0)) && !cache.contains(&key(1)));
        assert_eq!(cache.bytes(), one * 3);
        assert_eq!(cache.stats().evictions, 1);
        assert_eq!(cache.stats().peak_bytes, one * 3);
        // Re-inserting an existing key does not double count.
        assert!(cache.insert(key(3), tile(3)));
        assert_eq!(cache.len(), 3);
        assert!(!TileCache::new(one - 1).insert(key(0), tile(0)));
        cache.remove_image(ImageId(7));
        assert!(cache.is_empty() && cache.bytes() == 0);
    }

    #[test]
    fn f16_round_trip() {
        let t = tile(5);
        assert_eq!(t.format(), TileFormat::F16Planar);
        let back = to_f32(&t).unwrap();
        assert!(back.samples::<f32>().unwrap().iter().all(|&v| v == 5.0));
        let big = Tile::from_samples(t.coord(), t.layout(), vec![1e9f32; 48]).unwrap();
        let clamped = to_f32(&to_f16(&big).unwrap()).unwrap();
        assert!(
            clamped
                .samples::<f32>()
                .unwrap()
                .iter()
                .all(|&v| v == 65504.0)
        );
    }
}
