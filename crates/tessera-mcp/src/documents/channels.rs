//! Host-owned immutable channel rasters and channel edit planning.
use super::{Documents, convert, dense};
use compositor::{Depth, DocOp, DocState, Raster, Rect};
use engine_api::{
    EngineError, EngineResult,
    document::{ChannelKind, ChannelRasterRef, ChannelSummary, SelectionMode},
    id::{ChannelId, Digest},
    tools::DocumentToolCall,
};

fn kind(value: &ChannelKind) -> compositor::channels::ChannelKind {
    match value {
        ChannelKind::Alpha => compositor::channels::ChannelKind::Alpha,
        ChannelKind::Spot {
            display_rgb,
            solidity,
        } => compositor::channels::ChannelKind::Spot {
            color: *display_rgb,
            solidity: *solidity,
        },
    }
}

pub(super) fn summaries(state: &DocState) -> EngineResult<Vec<ChannelSummary>> {
    state
        .channels
        .iter()
        .map(|c| {
            Ok(ChannelSummary {
                id: ChannelId(c.id.0),
                name: c.name.clone(),
                kind: match c.kind {
                    compositor::channels::ChannelKind::Alpha => ChannelKind::Alpha,
                    compositor::channels::ChannelKind::Spot { color, solidity } => {
                        ChannelKind::Spot {
                            display_rgb: color,
                            solidity,
                        }
                    }
                },
                depth: convert("depth", &c.raster.depth())?,
                extent: c.raster.extent(),
            })
        })
        .collect()
}

impl Documents {
    /// Stage an existing document mask without changing history or selection.
    pub fn stage_document_channel(
        &mut self,
        document: engine_api::id::DocumentId,
        channel: Option<ChannelId>,
    ) -> EngineResult<ChannelRasterRef> {
        let state = self.session(document)?.state();
        let raster = match channel {
            Some(id) => state
                .channels
                .iter()
                .find(|c| c.id.0 == id.0)
                .ok_or_else(|| EngineError::not_found("channel", id))?
                .raster
                .clone(),
            None => state
                .selection
                .as_deref()
                .ok_or_else(|| EngineError::invalid("selection", "no active selection to stage"))?
                .clone(),
        };
        self.stage_channel_raster(raster)
    }

    /// Stage an immutable, normalized, single-plane raster for channel calls.
    /// Handles are content addressed and retained for this Documents lifetime,
    /// including across document close/undo and action playback. Not persisted.
    /// Hosts must restage the same content when replaying in another process.
    pub fn stage_channel_raster(&mut self, raster: Raster) -> EngineResult<ChannelRasterRef> {
        let extent = raster.extent();
        // Bound temporary dense materialization and session retained memory.
        if raster.channels() != 1
            || extent.width == 0
            || extent.height == 0
            || u64::from(extent.width) * u64::from(extent.height) > 16_777_216
        {
            return Err(EngineError::invalid(
                "raster",
                "single plane, positive extent, at most 16M pixels required",
            ));
        }
        let pixels = dense::read(&raster, Rect::of_extent(extent))?;
        let depth = convert("depth", &raster.depth())?;
        let mut bytes = serde_json::to_vec(&(extent, depth))?;
        for p in &pixels.px {
            let v = p[0];
            if !v.is_finite() || !(0.0..=1.0).contains(&v) {
                return Err(EngineError::invalid(
                    "raster",
                    "samples must be finite within 0..=1",
                ));
            }
            bytes.extend_from_slice(&(if v == 0.0 { 0.0f32 } else { v }).to_le_bytes());
        }
        let digest = Digest::derive("tessera.mcp.channel-raster.v1", &bytes);
        if !self.staged_channels.contains_key(&digest) {
            let retained: u64 = self
                .staged_channels
                .values()
                .map(|r| u64::from(r.extent().width) * u64::from(r.extent().height))
                .sum();
            if retained + u64::from(extent.width) * u64::from(extent.height) > 67_108_864 {
                return Err(EngineError::invalid(
                    "raster",
                    "session staging budget exhausted",
                ));
            }
            self.staged_channels.insert(digest, raster);
        }
        Ok(ChannelRasterRef {
            digest,
            depth,
            extent,
        })
    }

    fn resolve_channel_raster(
        &self,
        reference: &ChannelRasterRef,
        state: &DocState,
    ) -> EngineResult<Raster> {
        let r = self
            .staged_channels
            .get(&reference.digest)
            .ok_or_else(|| EngineError::not_found("staged channel raster", reference.digest))?;
        if reference.extent != r.extent()
            || reference.extent != state.canvas
            || reference.depth != convert("depth", &r.depth())?
        {
            return Err(EngineError::invalid(
                "raster",
                "staged depth/extent must match reference and document canvas",
            ));
        }
        Ok(r.clone())
    }

    pub(super) fn channel_op(
        &self,
        state: &DocState,
        call: &DocumentToolCall,
    ) -> EngineResult<DocOp> {
        use compositor::channels::{ChannelId as Id, DocumentChannel};
        Ok(match call {
            DocumentToolCall::AddChannel {
                name,
                kind: k,
                raster,
                ..
            } => DocOp::AddChannel {
                channel: DocumentChannel {
                    id: Id(0),
                    name: name.clone(),
                    kind: kind(k),
                    raster: self.resolve_channel_raster(raster, state)?,
                },
            },
            DocumentToolCall::DeleteChannel { channel, .. } => {
                DocOp::DeleteChannel { id: Id(channel.0) }
            }
            DocumentToolCall::RenameChannel { channel, name, .. } => DocOp::RenameChannel {
                id: Id(channel.0),
                name: name.clone(),
            },
            DocumentToolCall::EditChannel {
                channel,
                kind: k,
                raster,
                ..
            } => {
                let mut c = state
                    .channels
                    .iter()
                    .find(|c| c.id.0 == channel.0)
                    .ok_or_else(|| EngineError::not_found("channel", channel))?
                    .clone();
                if let Some(k) = k {
                    c.kind = kind(k);
                }
                if let Some(r) = raster {
                    c.raster = self.resolve_channel_raster(r, state)?;
                }
                DocOp::EditChannel { channel: c }
            }
            DocumentToolCall::LoadChannelAsSelection { channel, mode, .. } => {
                let loaded =
                    selection::channels::load(state, Id(channel.0))?.to_raster(Depth::F32)?;
                use compositor::document::selection::{Combine, combine};
                let empty = Raster::new(state.canvas, 1, Depth::F32, 0.0);
                let current = state.selection.as_deref().unwrap_or(&empty);
                let selection = match mode {
                    SelectionMode::Replace => loaded,
                    SelectionMode::Add => combine(current, &loaded, Combine::Add)?,
                    SelectionMode::Subtract => combine(current, &loaded, Combine::Subtract)?,
                    SelectionMode::Intersect => combine(current, &loaded, Combine::Intersect)?,
                };
                DocOp::SetSelection {
                    selection: Some(selection),
                }
            }
            _ => return Err(EngineError::internal("not a channel call")),
        })
    }
}
