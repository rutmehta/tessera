//! Bounded pure calibration-decision storage. No Engine integration yet.
//! Module remains cfg(test) until separately reviewed validated-source integration.
use engine_api::{
    EngineResult,
    recipe::{DevelopSettings, ProcessVersion},
};
use image_core::RendererConfig;

const CAPACITY: usize = 16;
const TTL_NS: u64 = 30_000_000_000;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Key([u8; 32]);
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Decision {
    Cpu,
    Metal,
}
#[derive(Clone, Copy, Debug)]
struct Samples {
    cpu: [f64; 3],
    gpu: [f64; 3],
}
#[derive(Clone, Copy, Debug)]
enum SelectionOutcome {
    Measured(Samples),
    ExplicitOverride,
    Unavailable,
    CalibrationFailed,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Override {
    Auto,
    Cpu,
    Metal,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Eligibility {
    SelfContained,
    MappedGeometry,
    UnsupportedTail,
    ExternalAssetUnversioned,
    DeviceUnhealthy,
}
#[derive(Clone, Copy, Debug)]
struct Policy {
    preference: Override,
    eligibility: Eligibility,
}
const AUTO: Policy = Policy {
    preference: Override::Auto,
    eligibility: Eligibility::SelfContained,
};

/// Copied only after actual source validation (Task 2), never source authority.
#[derive(Clone, Copy, Debug)]
struct AssetIdentity {
    owner: [u8; 16],
    container_digest: [u8; 32],
    original_digest: [u8; 32],
    original_length: u64,
    incarnation: [u8; 32],
    journal_generation: u64,
    recipe_digest: [u8; 32],
    dimensions: [u32; 2],
    tier: u8,
    format_version: u32,
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
    asset: AssetIdentity,
    settings: &'a DevelopSettings,
    recipe_process: ProcessVersion,
    config: &'a RendererConfig,
    device: DeviceIdentity,
    calibration_level: u8,
    calibration_extent: [u32; 2],
    sink_policy_version: u32,
    selector_policy_version: u32,
}

/// Advisory key error means cache bypass at integration, never a new open error.
fn key(input: &KeyInputs<'_>) -> EngineResult<Key> {
    use serde::Serialize;
    // JSON itself encodes nonfinite floats as null: explicitly reject them
    // recursively before hashing without allocating a Value/tree or byte Vec.
    input.settings.serialize(finite::Check)?;
    let mut h = blake3::Hasher::new();
    h.update(b"tessera proxy calibration decision v1\0");
    let a = input.asset;
    for bytes in [
        &a.owner[..],
        &a.container_digest,
        &a.original_digest,
        &a.original_length.to_le_bytes(),
        &a.incarnation,
        &a.journal_generation.to_le_bytes(),
        &a.recipe_digest,
        &a.dimensions[0].to_le_bytes(),
        &a.dimensions[1].to_le_bytes(),
        &[a.tier],
        &a.format_version.to_le_bytes(),
    ] {
        h.update(bytes);
    }
    for value in [
        input.device.generation,
        input.config.cache_budget_bytes as u64,
        input.config.threads as u64,
    ] {
        h.update(&value.to_le_bytes());
    }
    h.update(&input.device.adapter_fingerprint);
    h.update(&[
        input.device.capability_flags,
        u8::from(input.config.preview_approximations),
        input.calibration_level,
    ]);
    for value in [
        input.calibration_extent[0],
        input.calibration_extent[1],
        input.sink_policy_version,
        input.selector_policy_version,
    ] {
        h.update(&value.to_le_bytes());
    }
    h.update(&graph_fingerprint(input.config.graph.nodes()));
    serde_json::to_writer(
        HashWriter(&mut h),
        &(
            input.recipe_process,
            input.config.process_version,
            input.settings,
        ),
    )?;
    Ok(Key(*h.finalize().as_bytes()))
}
struct HashWriter<'a>(&'a mut blake3::Hasher);
impl std::io::Write for HashWriter<'_> {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.0.update(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}
fn graph_fingerprint(nodes: &[image_core::StageNode]) -> [u8; 32] {
    let mut h = blake3::Hasher::new();
    h.update(b"proxy graph v1\0");
    h.update(&(nodes.len() as u64).to_le_bytes());
    for node in nodes {
        let name = node.stage.name().as_bytes();
        h.update(&(name.len() as u64).to_le_bytes());
        h.update(name);
        h.update(&[
            u8::from(node.cacheable),
            u8::from(node.implemented),
            match node.frame {
                image_core::graph::Frame::Sensor => 0,
                image_core::graph::Frame::Output => 1,
            },
        ]);
    }
    *h.finalize().as_bytes()
}

#[derive(Clone, Copy)]
struct Entry {
    key: Key,
    decision: Decision,
    completed_ns: u64,
    last_used: u64,
    samples: Samples,
}
struct Cache {
    entries: [Option<Entry>; CAPACITY],
    ordinal: u64,
}
impl Cache {
    fn new() -> Self {
        Self {
            entries: [None; CAPACITY],
            ordinal: 0,
        }
    }
    fn allowed(&mut self, policy: Policy) -> bool {
        if policy.eligibility == Eligibility::DeviceUnhealthy {
            self.clear();
            return false;
        }
        policy.preference == Override::Auto && policy.eligibility == Eligibility::SelfContained
    }
    fn prune(&mut self, now_ns: u64) {
        for slot in &mut self.entries {
            if slot.is_some_and(|e| {
                now_ns
                    .checked_sub(e.completed_ns)
                    .is_none_or(|age| age >= TTL_NS)
            }) {
                *slot = None;
            }
        }
    }
    fn touch(&mut self) -> u64 {
        if self.ordinal == u64::MAX {
            let old = self.entries;
            for entry in self.entries.iter_mut().flatten() {
                entry.last_used = old
                    .iter()
                    .flatten()
                    .filter(|e| e.last_used < entry.last_used)
                    .count() as u64
                    + 1;
            }
            self.ordinal = self.len() as u64;
        }
        self.ordinal += 1;
        self.ordinal
    }
    /// now_ns is measurement completion, not start. Cache hits never change it.
    fn insert(&mut self, key: Key, outcome: SelectionOutcome, policy: Policy, now_ns: u64) -> bool {
        if !self.allowed(policy) {
            return false;
        }
        let SelectionOutcome::Measured(samples) = outcome else {
            return false;
        };
        if !samples
            .cpu
            .iter()
            .chain(&samples.gpu)
            .all(|v| v.is_finite() && *v > 0.)
        {
            return false;
        }
        let decision = if super::gpu_is_faster(samples.cpu, samples.gpu) {
            Decision::Metal
        } else {
            Decision::Cpu
        };
        self.prune(now_ns);
        let index = self
            .entries
            .iter()
            .position(|e| e.is_some_and(|e| e.key == key))
            .or_else(|| self.entries.iter().position(Option::is_none))
            .unwrap_or_else(|| {
                self.entries
                    .iter()
                    .enumerate()
                    .min_by_key(|(_, e)| e.unwrap().last_used)
                    .unwrap()
                    .0
            });
        let last_used = self.touch();
        self.entries[index] = Some(Entry {
            key,
            decision,
            completed_ns: now_ns,
            last_used,
            samples,
        });
        true
    }
    fn lookup(&mut self, key: Key, policy: Policy, now_ns: u64) -> Option<Decision> {
        if !self.allowed(policy) {
            return None;
        }
        self.prune(now_ns);
        let index = self
            .entries
            .iter()
            .position(|e| e.is_some_and(|e| e.key == key))?;
        let last_used = self.touch();
        let entry = self.entries[index].as_mut().unwrap();
        entry.last_used = last_used;
        // Measurements are retained fixed-size provenance, never live resources.
        debug_assert!(
            entry
                .samples
                .cpu
                .iter()
                .chain(&entry.samples.gpu)
                .all(|v| v.is_finite() && *v > 0.)
        );
        Some(entry.decision)
    }
    fn len(&self) -> usize {
        self.entries.iter().flatten().count()
    }
    fn clear(&mut self) {
        self.entries.fill(None);
        self.ordinal = 0;
    }
    fn set_ordinal_for_test(&mut self, ordinal: u64) {
        self.ordinal = ordinal;
    }
}
const _: () = assert!(std::mem::size_of::<Cache>() <= 8192);

mod finite;

#[cfg(test)]
mod tests;
