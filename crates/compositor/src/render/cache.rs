//! Byte-budgeted LRU of rendered tiles keyed by (document, node, part,
//! stamp, tile): engine-api's `NodeMemoKey`.

use std::collections::{BTreeMap, HashMap};
use std::sync::Mutex;

use engine_api::id::{DocumentId, LayerId};
use engine_api::stage::NodeMemoKey;
use engine_api::tile::{Tile, TileCoord};

/// What a cached tile is: engine-api's `NodePart`.
///
/// - `Content`: a layer's own pixels at a pyramid level (document depth, straight).
/// - `Mask`: a layer mask at a pyramid level.
/// - `Group`: an isolated group's composite (f32, premultiplied).
/// - `Root`: the document composite (f32, premultiplied).
/// - `Smart`: a smart object resampled into the parent (f32, straight).
pub(crate) use engine_api::stage::NodePart as Part;

/// Cache key as the renderer builds it. `stamp` is the maximum revision over
/// the node's footprint. `doc` is the runtime document key and `node` the
/// layer id (0 for the root); both map one-to-one onto the typed
/// [`NodeMemoKey`] the cache stores.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct NodeKey {
    pub doc: u64,
    pub node: u64,
    pub part: Part,
    pub stamp: u64,
    pub coord: TileCoord,
}

impl From<NodeKey> for NodeMemoKey {
    fn from(k: NodeKey) -> Self {
        NodeMemoKey {
            doc: DocumentId(k.doc),
            node: LayerId(k.node),
            part: k.part,
            revision: k.stamp,
            tile: k.coord,
        }
    }
}

impl From<NodeMemoKey> for NodeKey {
    fn from(k: NodeMemoKey) -> Self {
        NodeKey {
            doc: k.doc.0,
            node: k.node.0,
            part: k.part,
            stamp: k.revision,
            coord: k.tile,
        }
    }
}

struct Entry {
    tile: Tile,
    tick: u64,
}

#[derive(Default)]
struct Lru {
    map: HashMap<NodeMemoKey, Entry>,
    order: BTreeMap<u64, NodeMemoKey>,
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
        let key = NodeMemoKey::from(*key);
        let mut l = self.lru.lock().unwrap_or_else(|e| e.into_inner());
        l.tick += 1;
        let tick = l.tick;
        let e = l.map.get_mut(&key)?;
        let old = std::mem::replace(&mut e.tick, tick);
        let tile = e.tile.clone();
        l.order.remove(&old);
        l.order.insert(tick, key);
        Some(tile)
    }

    pub fn insert(&self, key: NodeKey, tile: Tile) {
        let key = NodeMemoKey::from(key);
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
        let dead: Vec<NodeMemoKey> = l
            .map
            .keys()
            .filter(|k| !pred(&NodeKey::from(**k)))
            .copied()
            .collect();
        for k in dead {
            if let Some(e) = l.map.remove(&k) {
                l.order.remove(&e.tick);
                l.bytes -= e.tile.byte_len();
            }
        }
    }
}
