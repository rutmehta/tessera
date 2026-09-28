//! Bounded Engine-local calibration decisions. No image/backend ownership.
use engine_api::{
    EngineResult,
    recipe::{DevelopSettings, ProcessVersion},
};
use image_core::RendererConfig;

const CAPACITY: usize = 16;
const TTL_NS: u64 = 30_000_000_000;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Key([u8; 32]);
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Decision {
    Cpu,
    Metal,
}
#[derive(Clone, Copy, Debug)]
pub(crate) struct Samples {
    pub(crate) cpu: [f64; 3],
    pub(crate) gpu: [f64; 3],
}
#[derive(Clone, Copy, Debug)]
pub(crate) enum SelectionOutcome {
    Measured(Samples),
    #[cfg(test)]
    ExplicitOverride,
    #[cfg(test)]
    Unavailable,
    #[cfg(test)]
    CalibrationFailed,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Override {
    Auto,
    #[cfg(test)]
    Cpu,
    #[cfg(test)]
    Metal,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Eligibility {
    SelfContained,
    #[cfg(test)]
    MappedGeometry,
    #[cfg(test)]
    UnsupportedTail,
    #[cfg(test)]
    ExternalAssetUnversioned,
    #[cfg(test)]
    DeviceUnhealthy,
}
#[derive(Clone, Copy, Debug)]
pub(crate) struct Policy {
    pub(crate) preference: Override,
    pub(crate) eligibility: Eligibility,
}
pub(crate) const AUTO: Policy = Policy {
    preference: Override::Auto,
    eligibility: Eligibility::SelfContained,
};

/// Copied only after actual source validation (Task 2), never source authority.
#[derive(Clone, Copy, Debug)]
pub(crate) struct AssetIdentity {
    pub(crate) owner: [u8; 16],
    pub(crate) container_digest: [u8; 32],
    pub(crate) original_digest: [u8; 32],
    pub(crate) original_length: u64,
    pub(crate) incarnation: [u8; 32],
    pub(crate) journal_generation: u64,
    pub(crate) recipe_digest: [u8; 32],
    pub(crate) dimensions: [u32; 2],
    pub(crate) tier: u8,
    pub(crate) encoding: u8,
    pub(crate) format_version: u32,
}
#[derive(Clone, Copy, Debug)]
pub(crate) struct DeviceIdentity {
    pub(crate) generation: u64,
    /// Versioned complete adapter backend/vendor/device/driver descriptor hash.
    pub(crate) adapter_fingerprint: [u8; 32],
    /// bit0 timestamp queries; bit1 shader_f16; bit2 rgba16float storage; bit3 passthrough.
    pub(crate) capability_flags: u8,
}
pub(crate) struct KeyInputs<'a> {
    pub(crate) asset: AssetIdentity,
    pub(crate) settings: &'a DevelopSettings,
    pub(crate) recipe_process: ProcessVersion,
    pub(crate) config: &'a RendererConfig,
    pub(crate) device: DeviceIdentity,
    pub(crate) calibration_level: u8,
    pub(crate) calibration_extent: [u32; 2],
    pub(crate) sink_policy_version: u32,
    pub(crate) selector_policy_version: u32,
}

/// Advisory key error means cache bypass at integration, never a new open error.
pub(crate) fn key(input: &KeyInputs<'_>) -> EngineResult<Key> {
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
        &[a.tier, a.encoding],
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
pub(crate) struct Cache {
    entries: [Option<Entry>; CAPACITY],
    ordinal: u64,
}
impl Cache {
    pub(crate) fn new() -> Self {
        Self {
            entries: [None; CAPACITY],
            ordinal: 0,
        }
    }
    fn allowed(&mut self, policy: Policy) -> bool {
        #[cfg(test)]
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
    pub(crate) fn insert(
        &mut self,
        key: Key,
        outcome: SelectionOutcome,
        policy: Policy,
        now_ns: u64,
    ) -> bool {
        if !self.allowed(policy) {
            return false;
        }
        let samples = match outcome {
            SelectionOutcome::Measured(samples) => samples,
            #[cfg(test)]
            _ => return false,
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
    pub(crate) fn lookup(&mut self, key: Key, policy: Policy, now_ns: u64) -> Option<Decision> {
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
    pub(crate) fn len(&self) -> usize {
        self.entries.iter().flatten().count()
    }
    pub(crate) fn clear(&mut self) {
        self.entries.fill(None);
        self.ordinal = 0;
    }
    #[cfg(test)]
    fn set_ordinal_for_test(&mut self, ordinal: u64) {
        self.ordinal = ordinal;
    }
}
const _: () = assert!(std::mem::size_of::<Cache>() <= 8192);

mod finite;

#[cfg(test)]
mod tests;

/// Fixed cache plus mutex/clock bookkeeping remains bounded; no heap ownership.
pub(crate) struct Store {
    cache: std::sync::Mutex<Cache>,
    started: std::time::Instant,
}
impl Default for Store {
    fn default() -> Self {
        Self {
            cache: std::sync::Mutex::new(Cache::new()),
            started: std::time::Instant::now(),
        }
    }
}
impl Store {
    fn now(&self) -> u64 {
        self.started.elapsed().as_nanos().min(u128::from(u64::MAX)) as u64
    }
    pub(crate) fn lookup(&self, key: Key) -> Option<Decision> {
        // Poison is advisory failure, never a new image-open error. Clear retained
        // records but leave poison set, so this Engine permanently bypasses reuse.
        match self.cache.lock() {
            Ok(mut cache) => cache.lookup(key, AUTO, self.now()),
            Err(e) => {
                e.into_inner().clear();
                None
            }
        }
    }
    pub(crate) fn publish(&self, key: Key, samples: Samples) -> bool {
        let completed_ns = self.now();
        match self.cache.lock() {
            Ok(mut cache) => {
                cache.insert(key, SelectionOutcome::Measured(samples), AUTO, completed_ns)
            }
            Err(e) => {
                e.into_inner().clear();
                false
            }
        }
    }
    pub(crate) fn clear(&self) {
        self.cache.lock().unwrap_or_else(|e| e.into_inner()).clear();
    }
    #[cfg(all(test, target_os = "macos"))]
    pub(crate) fn len(&self) -> usize {
        self.cache.lock().unwrap_or_else(|e| e.into_inner()).len()
    }
}
const _: () = assert!(std::mem::size_of::<Store>() <= 8192);

/// Names/paths are not content identity. Captured prefix corrections remain
/// container-bound; Auto lens does not resolve a new profile in this route.
pub(crate) fn self_contained(settings: &DevelopSettings, process: ProcessVersion) -> bool {
    process == ProcessVersion::NATIVE_CURRENT
        && settings.camera_profile == Default::default()
        && settings.color.lut.is_none()
        && settings.output.proof_profile.is_none()
        && settings.locals.adjustments.is_empty()
        && settings.locals.retouch.is_empty()
        && settings.effects.lens_blur.is_none()
}
