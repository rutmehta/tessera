//! Closed decoding of a retained capture into owned sensor samples and metadata.
use super::{CapturedAssetIdentity, CapturedRaw};
use crate::{CfaU16, RawMetadata, RawSource};
use engine_api::{
    EngineError, EngineResult, jobs::CancellationToken, pinned_raw::PinnedRawDecoderRoute,
};
use std::path::Path;

/// Owned packed sensor integers and metadata, usable after capture-stage removal.
/// No normalization, demosaic, recipe, rendering or native-memory bound is implied.
pub struct DecodedCapturedCfa {
    pub image: CfaU16,
    pub metadata: RawMetadata,
    pub identity: CapturedAssetIdentity,
    pub route: PinnedRawDecoderRoute,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum DecodePhase {
    Classified,
    Opened,
    Decoded,
}

impl CapturedRaw {
    /// Decode the captured CFA route, retaining its stage through native reads.
    /// Cancellation is checked between calls, not during native open/unpack.
    /// A completed phase error takes precedence over concurrent cancellation.
    pub fn decode_cfa(self, cancel: &CancellationToken) -> EngineResult<DecodedCapturedCfa> {
        self.decode_closed(cancel, decode_native, |_| {})
    }

    // Private and concrete output: this is not a public callback/reader escape.
    fn decode_closed(
        mut self,
        cancel: &CancellationToken,
        decoder: impl FnOnce(
            &Path,
            &CancellationToken,
            &mut dyn FnMut(DecodePhase),
        ) -> EngineResult<(CfaU16, RawMetadata)>,
        mut phase: impl FnMut(DecodePhase),
    ) -> EngineResult<DecodedCapturedCfa> {
        let result = (|| {
            cancel.check()?;
            match self.route {
                PinnedRawDecoderRoute::LibRawCfaV1 => {}
            }
            let linear =
                crate::linear_dng::is_linear_dng(self.stage.readonly()?).map_err(|error| {
                    EngineError::Decode {
                        format: "LinearRaw DNG classifier".into(),
                        message: error.to_string(),
                    }
                })?;
            phase(DecodePhase::Classified);
            cancel.check()?;
            if linear {
                return Err(EngineError::Unsupported {
                    what: "LinearRaw DNG is not the captured CFA decoder route".into(),
                });
            }
            let (image, metadata) = decoder(self.stage.path()?, cancel, &mut phase)?;
            cancel.check()?;
            if image.width != metadata.width
                || image.height != metadata.height
                || image.cfa_layout != metadata.cfa_layout
            {
                return Err(EngineError::Decode {
                    format: "raw".into(),
                    message: "decoded CFA dimensions/layout differ from metadata".into(),
                });
            }
            Ok(DecodedCapturedCfa {
                image,
                metadata,
                identity: self.identity,
                route: self.route,
            })
        })();
        // Decoder's inner scope has ended, including RawSource/RawFile destruction.
        // All failures use this common cleanup path, including pre-cancellation.
        let pool = self.stage.reservation.pool.clone();
        let cleanup = self.close();
        match result {
            Ok(output) => {
                cleanup?;
                Ok(output)
            }
            Err(primary) => {
                if let Err(secondary) = cleanup {
                    pool.operations.report(&secondary);
                }
                Err(primary)
            }
        }
    }

    #[cfg(all(test, unix))]
    pub(super) fn decode_cfa_with_for_test(
        self,
        cancel: &CancellationToken,
        decoder: impl FnOnce(&Path, &CancellationToken) -> EngineResult<(CfaU16, RawMetadata)>,
    ) -> EngineResult<DecodedCapturedCfa> {
        self.decode_closed(cancel, |path, token, _| decoder(path, token), |_| {})
    }

    #[cfg(all(test, unix))]
    pub(super) fn decode_cfa_with_phase_for_test(
        self,
        cancel: &CancellationToken,
        phase: impl FnMut(DecodePhase),
    ) -> EngineResult<DecodedCapturedCfa> {
        self.decode_closed(cancel, decode_native, phase)
    }
}

fn decode_native(
    path: &Path,
    cancel: &CancellationToken,
    phase: &mut dyn FnMut(DecodePhase),
) -> EngineResult<(CfaU16, RawMetadata)> {
    let mut source = RawSource::open(path)?;
    phase(DecodePhase::Opened);
    cancel.check()?;
    // Native unpack plus owned sample extraction is one existing synchronous API.
    // Do not claim cancellation inside either operation.
    let image = source.decode_cfa_u16()?;
    phase(DecodePhase::Decoded);
    cancel.check()?;
    let metadata = source.metadata();
    cancel.check()?;
    // Explicitly close the native stream before returning only owned Rust values.
    drop(source);
    Ok((image, metadata))
}
