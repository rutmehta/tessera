//! Call-site helpers for the instrumented resident lookups and requests
//! (rev7 3.1a). They never receive `Resolved`; a `None` token touches nothing.
use super::{ARENA, BeginContext, Bucket, Identity, Outcome, Record, Token};
use crate::{
    RenderOutput,
    resident::{ResidentBatch, ResidentTile},
};
use engine_api::{EngineResult, id::Digest, stage::MemoKey};

/// S1 digest domain for the resolved WB matrix bits.
const WB_DIGEST_CONTEXT: &str = "tessera wb-diagnostic resolved-wb v1";

/// Observed resident lookup. Always calls `batch.cached(key)` exactly once and
/// returns its result unchanged; the arena stamps identity and outcome.
pub fn lookup(
    diag: Option<Token>,
    batch: &mut dyn ResidentBatch,
    key: &MemoKey,
    bucket: Bucket,
) -> EngineResult<Option<ResidentTile>> {
    match diag {
        None => batch.cached(key),
        Some(t) => ARENA.observe(t, unstamped(*key, bucket), || batch.cached(key)),
    }
}

/// Records a cache request for a live token.
pub fn request(t: Token, key: &MemoKey, bucket: Bucket) {
    ARENA.request(t, *key, bucket);
}

/// Records Begin for a live token from the resolved WB matrix bits. A
/// nonfinite matrix rejects attribution (no Begin). The digest is computed
/// here, before `ARENA.begin`, outside the arena lock (S1).
pub fn begin(t: Token, wb_bits: [u64; 9], output: RenderOutput, level: u8, operator: u64) {
    if !wb_bits.iter().all(|b| f64::from_bits(*b).is_finite()) {
        ARENA.reject_attribution(t);
        return;
    }
    let mut bytes = [0_u8; 72];
    for (chunk, bits) in bytes.as_chunks_mut::<8>().0.iter_mut().zip(wb_bits) {
        *chunk = bits.to_le_bytes();
    }
    let digest = Digest::derive(WB_DIGEST_CONTEXT, &bytes).0;
    // Context.output_tag: 0 scene linear, 1 SDR display, 2 display linear.
    let (output_tag, headroom_bits) = match output {
        RenderOutput::SceneLinear => (0, 0),
        RenderOutput::Display => (1, 0),
        RenderOutput::DisplayLinear(h) => (2, h.get().to_bits()),
    };
    ARENA.begin(
        t,
        BeginContext::new(digest, output_tag, headroom_bits, level, operator).with_wb_bits(wb_bits),
    );
}

/// Lookup record before the arena stamps identity and outcome.
fn unstamped(key: MemoKey, bucket: Bucket) -> Record {
    Record {
        identity: Identity {
            operator: 0,
            phase: 0,
            transaction: 0,
            generation: 0,
        },
        key,
        bucket,
        outcome: Outcome::None,
    }
}
