//! Call-site helpers for the instrumented resident lookups and requests
//! (rev7 3.1a). They never receive `Resolved`; a `None` token touches nothing.
//! B0: signatures only; the token is ignored.
use super::{Bucket, Token};
use crate::{
    RenderOutput,
    resident::{ResidentBatch, ResidentTile},
};
use engine_api::{EngineResult, stage::MemoKey};

/// Observed resident lookup. Always calls `batch.cached(key)` exactly once and
/// returns its result unchanged.
pub fn lookup(
    diag: Option<Token>,
    batch: &mut dyn ResidentBatch,
    key: &MemoKey,
    bucket: Bucket,
) -> EngineResult<Option<ResidentTile>> {
    let _ = (diag, bucket);
    batch.cached(key)
}

/// Records a cache request for a live token.
pub fn request(t: Token, key: &MemoKey, bucket: Bucket) {
    let _ = (t, key, bucket);
}

/// Records Begin for a live token from the resolved WB matrix bits.
pub fn begin(t: Token, wb_bits: [u64; 9], output: RenderOutput, level: u8, operator: u64) {
    let _ = (t, wb_bits, output, level, operator);
}
