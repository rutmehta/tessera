//! Save/Load Selection as named alpha channels.

use std::collections::BTreeMap;

use compositor::{Depth, Raster};
use engine_api::tile::Extent;
use engine_api::{EngineError, EngineResult};

use crate::mask::Mask;

/// Save a lossless F32 selection in document history, returning its identity.
pub fn save(
    doc: &mut compositor::Document,
    name: &str,
    mask: &Mask,
) -> EngineResult<compositor::channels::ChannelId> {
    let raster = mask.to_raster(Depth::F32)?;
    doc.apply(compositor::DocOp::AddChannel {
        channel: compositor::channels::DocumentChannel {
            id: compositor::channels::ChannelId(0),
            name: name.into(),
            kind: compositor::channels::ChannelKind::Alpha,
            raster,
        },
    })?;
    Ok(doc.state().channels.last().expect("inserted channel").id)
}

/// Load a document alpha or spot plane as a dense selection.
pub fn load(
    state: &compositor::DocState,
    id: compositor::channels::ChannelId,
) -> EngineResult<Mask> {
    let c = state
        .channels
        .iter()
        .find(|c| c.id == id)
        .ok_or_else(|| EngineError::not_found("channel", id.0))?;
    Mask::from_raster(&c.raster)
}

/// Legacy standalone collection. Use [`save`]/[`load`] for document persistence
/// and undo support. Retained for callers manipulating detached masks.
#[derive(Debug, Clone)]
pub struct AlphaChannels {
    extent: Extent,
    depth: Depth,
    channels: BTreeMap<String, Raster>,
}

impl AlphaChannels {
    /// Channels for a `extent` document stored at `depth`.
    pub fn new(extent: Extent, depth: Depth) -> Self {
        Self {
            extent,
            depth,
            channels: BTreeMap::new(),
        }
    }

    /// Save Selection: stores (or replaces) `name`.
    pub fn save(&mut self, name: &str, m: &Mask) -> EngineResult<()> {
        if m.extent() != self.extent {
            return Err(EngineError::invalid("selection", "extent mismatch"));
        }
        self.channels
            .insert(name.to_string(), m.to_raster(self.depth)?);
        Ok(())
    }

    /// Load Selection.
    pub fn load(&self, name: &str) -> EngineResult<Mask> {
        let r = self
            .channels
            .get(name)
            .ok_or_else(|| EngineError::invalid("channel", format!("no channel {name:?}")))?;
        Mask::from_raster(r)
    }

    /// Adds an existing single-channel raster (e.g. from a PSD).
    pub fn insert_raster(&mut self, name: &str, r: Raster) -> EngineResult<()> {
        if r.channels() != 1 || r.extent() != self.extent {
            return Err(EngineError::invalid(
                "channel",
                "must be single-channel, canvas-sized",
            ));
        }
        self.channels.insert(name.to_string(), r);
        Ok(())
    }

    /// The stored raster.
    pub fn raster(&self, name: &str) -> Option<&Raster> {
        self.channels.get(name)
    }

    /// Deletes a channel.
    pub fn remove(&mut self, name: &str) -> Option<Raster> {
        self.channels.remove(name)
    }

    /// Channel names in order.
    pub fn names(&self) -> impl Iterator<Item = &str> {
        self.channels.keys().map(String::as_str)
    }
}
