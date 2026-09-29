//! Opaque post-decode ownership contract. Construction remains Unsupported until RED.
#[cfg(all(test, unix))]
use super::DecodedCapturedCfa;
use super::{CapturedAssetIdentity, CapturedRaw};
use crate::{CfaLayout, CfaU16, RawMetadata};
use engine_api::{
    EngineError, EngineResult, jobs::CancellationToken, pinned_raw::PinnedRawDecoderRoute,
};

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
        unsupported()
    }
}
fn unsupported<T>() -> EngineResult<T> {
    Err(EngineError::Unsupported {
        what: "opaque captured CFA scaffold; behavioral RED not yet observed".into(),
    })
}
impl CapturedRaw {
    /// Consume this capture through the closed CFA decoder; no original locator reopen.
    pub fn decode_owned_cfa(self, _cancel: &CancellationToken) -> EngineResult<OwnedCapturedCfa> {
        unsupported()
    }
    #[cfg(all(test, unix))]
    pub(super) fn decode_owned_cfa_with_for_test(
        self,
        _cancel: &CancellationToken,
        _decoder: impl FnOnce(
            &std::path::Path,
            &CancellationToken,
        ) -> EngineResult<(CfaU16, RawMetadata)>,
    ) -> EngineResult<OwnedCapturedCfa> {
        unsupported()
    }
}
