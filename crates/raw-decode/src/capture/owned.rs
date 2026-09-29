//! Opaque owned result of the private closed captured-CFA decode path.
use super::{CapturedAssetIdentity, DecodedCapturedCfa};
use crate::{CfaLayout, CfaU16, RawMetadata};
#[cfg(all(test, unix))]
use engine_api::EngineResult;
use engine_api::pinned_raw::PinnedRawDecoderRoute;

/// Copied descriptive facts, not an admission token or memory reservation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CfaPlaneFacts {
    pub width: u32,
    pub height: u32,
    pub layout: CfaLayout,
    pub sample_len: usize,
    pub sample_capacity: usize,
}

/// Owned samples from the closed captured-CFA decoder, after stage cleanup.
/// This establishes neither render eligibility nor a native/application RAM cap.
/// No sample getter, mutable borrow, parts projection or inverse constructor.
pub struct OwnedCapturedCfa {
    image: CfaU16,
    metadata: RawMetadata,
    identity: CapturedAssetIdentity,
    route: PinnedRawDecoderRoute,
}
impl OwnedCapturedCfa {
    // Only the capture module's closed decoder may construct this owner.
    pub(super) fn new(
        image: CfaU16,
        metadata: RawMetadata,
        identity: CapturedAssetIdentity,
        route: PinnedRawDecoderRoute,
    ) -> Self {
        Self {
            image,
            metadata,
            identity,
            route,
        }
    }

    // Compatibility projection moves owned allocations; no inverse/public projection.
    pub(super) fn into_public(self) -> DecodedCapturedCfa {
        DecodedCapturedCfa {
            image: self.image,
            metadata: self.metadata,
            identity: self.identity,
            route: self.route,
        }
    }

    pub fn identity(&self) -> CapturedAssetIdentity {
        self.identity
    }
    pub fn route(&self) -> PinnedRawDecoderRoute {
        self.route
    }
    pub fn metadata(&self) -> &RawMetadata {
        &self.metadata
    }
    pub fn plane_facts(&self) -> CfaPlaneFacts {
        CfaPlaneFacts {
            width: self.image.width,
            height: self.image.height,
            layout: self.image.cfa_layout,
            sample_len: self.image.data.len(),
            sample_capacity: self.image.data.capacity(),
        }
    }
    #[cfg(all(test, unix))]
    pub(super) fn image_for_test(&self) -> &CfaU16 {
        &self.image
    }
    #[cfg(all(test, unix))]
    pub(super) fn into_public_for_test(self) -> EngineResult<DecodedCapturedCfa> {
        Ok(self.into_public())
    }
}
