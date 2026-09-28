//! Closed captured-CFA decoder interface; behavior awaits observed RED.
use super::{CapturedAssetIdentity, CapturedRaw};
use crate::{CfaU16, RawMetadata};
use engine_api::{EngineError, EngineResult, jobs::CancellationToken, pinned_raw::PinnedRawDecoderRoute};

/// Owned packed sensor integers and metadata, usable after capture-stage removal.
/// No normalization, demosaic, recipe, rendering or native-memory bound is implied.
pub struct DecodedCapturedCfa {
    pub image: CfaU16,
    pub metadata: RawMetadata,
    pub identity: CapturedAssetIdentity,
    pub route: PinnedRawDecoderRoute,
}

impl CapturedRaw {
    /// Decode the captured CFA route, retaining its stage through native reads.
    /// Cancellation can be observed between native calls, not during an open/unpack.
    pub fn decode_cfa(self, _cancel: &CancellationToken) -> EngineResult<DecodedCapturedCfa> {
        Err(EngineError::Unsupported { what: "captured CFA decoder not implemented".into() })
    }

    // Closed output type even in the private protocol injection seam. Tests of
    // this path prove ownership protocol, not actual LibRaw pixel fidelity.
    #[cfg(all(test, unix))]
    pub(super) fn decode_cfa_with_for_test(
        self, _cancel: &CancellationToken,
        _decoder: impl FnOnce(&std::path::Path, &CancellationToken) -> EngineResult<(CfaU16, RawMetadata)>,
    ) -> EngineResult<DecodedCapturedCfa> {
        Err(EngineError::Unsupported { what: "captured CFA decoder protocol not implemented".into() })
    }
}


#[cfg(all(test, unix))]
impl CapturedRaw {
    pub(super) fn decode_cfa_with_phase_for_test(
        self, _cancel: &CancellationToken,
        _phase: impl FnMut(super::tests::decode::DecodePhase),
    ) -> EngineResult<DecodedCapturedCfa> {
        Err(EngineError::Unsupported { what: "native decoder phase seam not implemented".into() })
    }
}
