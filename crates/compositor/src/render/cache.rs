//! Byte-budgeted LRU of rendered tiles keyed by (document, node, part,
//! stamp, tile).

use std::collections::{BTreeMap, HashMap};
use std::sync::Mutex;

use engine_api::tile::{Tile, TileCoord};

/// What a cached tile is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum Part {
    /// A layer's own pixels at a pyramid level (document depth, straight).
    Content,
    /// A layer mask at a pyramid level.
    Mask,
    /// An isolated group's composite (f32, premultiplied).
    Group,
    /// The document composite (f32, premultiplied).
    Root,
    /// A smart object resampled into the parent (f32, straight).
    Smart,
}

/// Cache key. `stamp` is the maximum revision over the node's footprint.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct NodeKey {
    pub doc: u64,
    pub node: u64,
    pub part: Part,
    pub stamp: u64,
    pub coord: TileCoord,
}

struct Entry {
    tile: Tile,
    tick: u64,
}

#[derive(Default)]
struct Lru {
    map: HashMap<NodeKey, Entry>,
    order: BTreeMap<u64, NodeKey>,
    tick: u64,
    bytes: usize,
    evictions: u64,
}

pub(crate) struct RenderCache {
    budget: usize,
    lru: Mutex<Lru>,
}

impl RenderCache {
    pub fn new(budget: usize) -> Self {
        Self {
            budget,
            lru: Mutex::new(Lru::default()),
        }
    }

    pub fn get(&self, key: &NodeKey) -> Option<Tile> {
        let mut l = self.lru.lock().unwrap_or_else(|e| e.into_inner());
        l.tick += 1;
        let tick = l.tick;
        let e = l.map.get_mut(key)?;
        let old = std::mem::replace(&mut e.tick, tick);
        let tile = e.tile.clone();
        l.order.remove(&old);
        l.order.insert(tick, *key);
        Some(tile)
    }

    pub fn insert(&self, key: NodeKey, tile: Tile) {
        let size = tile.byte_len();
        if size > self.budget {
            return;
        }
        let mut l = self.lru.lock().unwrap_or_else(|e| e.into_inner());
        l.tick += 1;
        let tick = l.tick;
        if let Some(old) = l.map.insert(key, Entry { tile, tick }) {
            l.order.remove(&old.tick);
            l.bytes -= old.tile.byte_len();
        }
        l.order.insert(tick, key);
        l.bytes += size;
        while l.bytes > self.budget {
            let Some((_, k)) = l.order.pop_first() else {
                break;
            };
            if let Some(e) = l.map.remove(&k) {
                l.bytes -= e.tile.byte_len();
                l.evictions += 1;
            }
        }
    }

    pub fn bytes(&self) -> usize {
        self.lru.lock().unwrap_or_else(|e| e.into_inner()).bytes
    }

    pub fn len(&self) -> usize {
        self.lru.lock().unwrap_or_else(|e| e.into_inner()).map.len()
    }

    pub fn evictions(&self) -> u64 {
        self.lru.lock().unwrap_or_else(|e| e.into_inner()).evictions
    }

    /// Drops entries matching `pred`.
    pub fn retain(&self, pred: impl Fn(&NodeKey) -> bool) {
        let mut l = self.lru.lock().unwrap_or_else(|e| e.into_inner());
        let dead: Vec<NodeKey> = l.map.keys().filter(|k| !pred(k)).copied().collect();
        for k in dead {
            if let Some(e) = l.map.remove(&k) {
                l.order.remove(&e.tick);
                l.bytes -= e.tile.byte_len();
            }
        }
    }
}
