//! Named document masks. Spot inks are preserved, but do not affect the
//! RGB composite yet: display colour and solidity are preview metadata only.
use crate::{DocState, Raster};
use engine_api::{EngineError, EngineResult};
use serde::{Deserialize, Serialize};

/// Stable identifier within a document; zero requests allocation on insertion.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChannelId(pub u64);

/// Purpose and display properties of a saved plane.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ChannelKind {
    /// Saved selection or general purpose mask.
    Alpha,
    /// Saved mask with explicit preview properties. Legacy `Alpha` means red,
    /// 50% opacity, masked areas. This metadata never changes stored samples.
    AlphaDisplay {
        /// Finite normalized display RGB.
        color: [f32; 3],
        /// Finite normalized overlay opacity.
        opacity: f32,
        /// Show selected (true) rather than masked (false) areas.
        selected: bool,
    },
    /// Spot ink, not included in RGB rendering.
    Spot {
        /// Normalized display RGB.
        color: [f32; 3],
        /// Normalized preview solidity.
        solidity: f32,
    },
}

/// Canvas-sized single-plane raster, shared copy-on-write across history.
#[derive(Debug, Clone)]
pub struct DocumentChannel {
    /// Stable identity.
    pub id: ChannelId,
    /// Display name (duplicates allowed, as in PSD).
    pub name: String,
    /// Alpha or spot metadata.
    pub kind: ChannelKind,
    /// Mask samples. Depth may differ from the document to preserve selections.
    pub raster: Raster,
}

impl DocumentChannel {
    /// Check shape and finite normalized spot display metadata.
    pub fn validate(&self, state: &DocState) -> EngineResult<()> {
        if self.raster.extent() != state.canvas || self.raster.channels() != 1 {
            return Err(EngineError::invalid(
                "channel",
                "must be canvas-sized and single-channel",
            ));
        }
        let values = match &self.kind {
            ChannelKind::Alpha => None,
            ChannelKind::AlphaDisplay { color, opacity, .. } => Some((color, opacity)),
            ChannelKind::Spot { color, solidity } => Some((color, solidity)),
        };
        if let Some((color, opacity)) = values
            && color
                .iter()
                .chain(std::iter::once(opacity))
                .any(|v| !v.is_finite() || !(0.0..=1.0).contains(v))
        {
            return Err(EngineError::invalid(
                "channel",
                "display values must be finite within 0..=1",
            ));
        }
        Ok(())
    }
}
