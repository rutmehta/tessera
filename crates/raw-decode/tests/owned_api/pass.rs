use engine_api::{EngineResult, jobs::CancellationToken, pinned_raw::PinnedRawDecoderRoute};
use raw_decode::{
    RawMetadata,
    capture::{CapturedAssetIdentity, CapturedRaw, CfaPlaneFacts, OwnedCapturedCfa},
};
pub fn use_owner(
    capture: CapturedRaw,
    token: &CancellationToken,
) -> EngineResult<OwnedCapturedCfa> {
    let owner = capture.decode_owned_cfa(token)?;
    let _: CapturedAssetIdentity = owner.identity();
    let _: PinnedRawDecoderRoute = owner.route();
    let _: &RawMetadata = owner.metadata();
    let _: CfaPlaneFacts = owner.plane_facts();
    Ok(owner)
}
fn main() {}
