//! Task 1 contracts only. No decision cache implementation or Engine integration.
//! Compiled only by tests until behavioral RED is observed and implementation is authorized.
use engine_api::{EngineError, EngineResult, recipe::{DevelopSettings, ProcessVersion}};
use image_core::RendererConfig;

const CAPACITY: usize = 16;
const TTL_NS: u64 = 30_000_000_000;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Key([u8; 32]);
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Decision { Cpu, Metal }
#[derive(Clone, Copy, Debug)]
struct Samples { cpu: [f64; 3], gpu: [f64; 3] }
#[derive(Clone, Copy, Debug)]
enum SelectionOutcome { Measured(Samples), ExplicitOverride, Unavailable, CalibrationFailed }
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Override { Auto, Cpu, Metal }
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Eligibility { SelfContained, MappedGeometry, UnsupportedTail, ExternalAssetUnversioned, DeviceUnhealthy }
#[derive(Clone, Copy, Debug)]
struct Policy { preference: Override, eligibility: Eligibility }
const AUTO: Policy = Policy { preference: Override::Auto, eligibility: Eligibility::SelfContained };

/// Copied only after actual source validation (Task 2), never source authority.
#[derive(Clone, Copy, Debug)]
struct AssetIdentity {
    owner: [u8; 16], container_digest: [u8; 32], original_digest: [u8; 32], original_length: u64,
    incarnation: [u8; 32], journal_generation: u64, recipe_digest: [u8; 32],
    dimensions: [u32; 2], tier: u8, format_version: u32,
}
#[derive(Clone, Copy, Debug)]
struct DeviceIdentity {
    generation: u64,
    /// Versioned complete adapter backend/vendor/device/driver descriptor hash.
    adapter_fingerprint: [u8; 32],
    /// bit0 timestamp queries; bit1 shader_f16; bit2 rgba16float storage; bit3 passthrough.
    capability_flags: u8,
}
struct KeyInputs<'a> {
    asset: AssetIdentity, settings: &'a DevelopSettings, recipe_process: ProcessVersion,
    config: &'a RendererConfig, device: DeviceIdentity,
    calibration_level: u8, calibration_extent: [u32; 2], sink_policy_version: u32,
    selector_policy_version: u32,
}

/// Stub: later encode full settings/config/ordered graph directly into bounded
/// streaming hash; do not normalize HDR, serialize to an accumulating Vec, or
/// use renderId's truncated container hash. Errors become caller cache bypass.
fn key(_input: &KeyInputs<'_>) -> EngineResult<Key> {
    Err(EngineError::Unsupported { what: "Task 1 key implementation awaits observed RED".into() })
}

#[derive(Clone, Copy)]
struct Entry { key: Key, decision: Decision, completed_ns: u64, last_used: u64, samples: Samples }
struct Cache { entries: [Option<Entry>; CAPACITY], ordinal: u64 }
impl Cache {
    fn new() -> Self { Self { entries: [None; CAPACITY], ordinal: 0 } }
    /// Times are monotonic nanoseconds in one Engine-local clock epoch. Insertion
    /// receives measurement COMPLETION; lookup must not extend the 30-second TTL.
    fn insert(&mut self, _key: Key, _outcome: SelectionOutcome, _policy: Policy, _now_ns: u64) -> bool { false }
    fn lookup(&mut self, _key: Key, _policy: Policy, _now_ns: u64) -> Option<Decision> { None }
    fn len(&self) -> usize { 0 }
    fn clear(&mut self) {}
    fn set_ordinal_for_test(&mut self, _ordinal: u64) {}
}

#[cfg(test)]
mod tests;
