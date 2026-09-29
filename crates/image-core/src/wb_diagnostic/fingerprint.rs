//! Shared request/Begin context helpers (post-Stage-B erratum E2), so the
//! reservation side (tessera-ffi) and the Begin side (`live::begin`) cannot
//! drift.
use crate::RenderOutput;
use engine_api::{
    id::{Digest, ImageId},
    recipe::{DevelopSettings, ProcessVersion},
    stage::canonical_json,
};

/// Fingerprint of the normalized settings actually rendered.
pub fn settings_fingerprint(settings: &DevelopSettings) -> [u8; 32] {
    Digest::derive(
        "tessera wb-diagnostic settings v1",
        &canonical_json(settings),
    )
    .0
}

/// Fingerprint of the render's recipe identity: source image plus settings.
pub fn recipe_fingerprint(image: ImageId, settings: &DevelopSettings) -> [u8; 32] {
    Digest::derive(
        "tessera wb-diagnostic recipe v1",
        &canonical_json(&(image, settings)),
    )
    .0
}

/// Process identity: the leading 8 bytes (LE) of the process-version digest.
pub fn process_identity(process: ProcessVersion) -> u64 {
    let d = Digest::derive(
        "tessera wb-diagnostic process v1",
        &canonical_json(&process),
    );
    let mut first = [0_u8; 8];
    first.copy_from_slice(&d.0[..8]);
    u64::from_le_bytes(first)
}

/// `Context.output_tag` and headroom bits: 0 scene linear, 1 SDR display,
/// 2 display linear (with its headroom bits); other outputs carry 0 bits.
pub fn output_tag(output: RenderOutput) -> (u8, u32) {
    match output {
        RenderOutput::SceneLinear => (0, 0),
        RenderOutput::Display => (1, 0),
        RenderOutput::DisplayLinear(h) => (2, h.get().to_bits()),
    }
}
