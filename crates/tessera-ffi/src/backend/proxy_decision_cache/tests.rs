//! Pure fake-clock contracts. No filesystem, environment mutation, device or renderer execution.
use super::*;
use engine_api::recipe::{ProcessFamily, settings::WhiteBalanceMode};
use engine_api::stage::StageId;
use image_core::PipelineGraph;

fn k(n: u8) -> Key { let mut bytes = [0; 32]; bytes[0] = n; Key(bytes) }
fn metal() -> SelectionOutcome { SelectionOutcome::Measured(Samples { cpu: [10., 10., 10.], gpu: [20., 2., 3.] }) }
fn cpu() -> SelectionOutcome { SelectionOutcome::Measured(Samples { cpu: [10., 10., 10.], gpu: [1., 12., 3.] }) }
fn asset() -> AssetIdentity { AssetIdentity { owner: [1; 16], container_digest: [2; 32], original_digest: [3; 32], original_length: 4096, incarnation: [4; 32], journal_generation: 7, recipe_digest: [5; 32], dimensions: [1640, 1092], tier: 1, format_version: 1 } }
fn input<'a>(settings: &'a DevelopSettings, config: &'a RendererConfig) -> KeyInputs<'a> {
    KeyInputs { asset: asset(), settings, config, recipe_process: ProcessVersion { family: ProcessFamily::Native, revision: 2 }, device: DeviceIdentity { generation: 1, adapter_fingerprint: [6; 32], capability_flags: 15 }, calibration_level: 0, calibration_extent: [1640, 1092], sink_policy_version: 1, selector_policy_version: 1 }
}

#[test]
fn fixed_storage_and_sample_record_are_bounded_without_heap_ownership() {
    assert_eq!(CAPACITY, 16);
    assert!(std::mem::size_of::<Cache>() <= 8192);
    assert!(!std::mem::needs_drop::<Cache>());
    assert_eq!(Cache::new().len(), 0);
}

#[test]
fn successfully_measured_metal_and_cpu_are_both_reusable() {
    let mut cache = Cache::new();
    assert!(cache.insert(k(1), metal(), AUTO, 100));
    assert!(cache.insert(k(2), cpu(), AUTO, 100));
    assert_eq!(cache.lookup(k(1), AUTO, 101), Some(Decision::Metal));
    assert_eq!(cache.lookup(k(2), AUTO, 101), Some(Decision::Cpu));
    assert_eq!(cache.len(), 2);
}

#[test]
fn measured_cpu_requires_six_valid_samples_and_preserves_existing_selection_rule() {
    for samples in [Samples { cpu: [10.; 3], gpu: [1., 10., 1.] }, Samples { cpu: [10.; 3], gpu: [1., 1., 10.] }] {
        let mut cache = Cache::new();
        assert!(cache.insert(k(1), SelectionOutcome::Measured(samples), AUTO, 1));
        assert_eq!(cache.lookup(k(1), AUTO, 2), Some(Decision::Cpu));
    }
}

#[test]
fn expiry_is_exact_and_hits_never_slide_measurement_completion_age() {
    let mut cache = Cache::new();
    assert!(cache.insert(k(1), metal(), AUTO, 123));
    assert_eq!(cache.lookup(k(1), AUTO, 123 + TTL_NS - 1), Some(Decision::Metal));
    assert_eq!(cache.lookup(k(1), AUTO, 123 + TTL_NS), None);
    assert_eq!(cache.len(), 0);
}

#[test]
fn seventeen_keys_evict_least_recently_used_not_oldest_measurement() {
    let mut cache = Cache::new();
    for n in 0..16 { assert!(cache.insert(k(n), metal(), AUTO, u64::from(n))); }
    assert_eq!(cache.lookup(k(0), AUTO, 20), Some(Decision::Metal));
    assert!(cache.insert(k(16), metal(), AUTO, 21));
    assert_eq!(cache.len(), 16);
    assert_eq!(cache.lookup(k(1), AUTO, 22), None);
    for n in [0, 2, 15, 16] { assert_eq!(cache.lookup(k(n), AUTO, 22), Some(Decision::Metal)); }
}

#[test]
fn ordinal_rollover_preserves_recency_and_capacity() {
    let mut cache = Cache::new();
    for n in 0..16 { assert!(cache.insert(k(n), metal(), AUTO, 0)); }
    cache.set_ordinal_for_test(u64::MAX);
    assert_eq!(cache.lookup(k(0), AUTO, 1), Some(Decision::Metal));
    assert!(cache.insert(k(16), metal(), AUTO, 2));
    assert_eq!(cache.lookup(k(1), AUTO, 3), None);
    assert_eq!(cache.lookup(k(0), AUTO, 3), Some(Decision::Metal));
    assert_eq!(cache.len(), 16);
}

#[test]
fn duplicate_successful_measurement_replaces_value_and_completion_time() {
    let mut cache = Cache::new();
    assert!(cache.insert(k(1), metal(), AUTO, 0));
    assert!(cache.insert(k(1), cpu(), AUTO, TTL_NS - 1));
    assert_eq!(cache.len(), 1);
    assert_eq!(cache.lookup(k(1), AUTO, TTL_NS), Some(Decision::Cpu));
    assert_eq!(cache.lookup(k(1), AUTO, 2 * TTL_NS - 1), None);
}

#[test]
fn invalid_measurements_and_errors_never_insert_or_replace_success() {
    let mut cache = Cache::new();
    assert!(cache.insert(k(1), metal(), AUTO, 1));
    for outcome in [SelectionOutcome::ExplicitOverride, SelectionOutcome::Unavailable, SelectionOutcome::CalibrationFailed] {
        assert!(!cache.insert(k(1), outcome, AUTO, 2));
        assert!(!cache.insert(k(2), outcome, AUTO, 2));
    }
    for bad in [0., -1., f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        for index in 0..6 {
            let mut samples = Samples { cpu: [10.; 3], gpu: [1.; 3] };
            if index < 3 { samples.cpu[index] = bad; } else { samples.gpu[index-3] = bad; }
            assert!(!cache.insert(k(1), SelectionOutcome::Measured(samples), AUTO, TTL_NS - 1));
            assert!(!cache.insert(k(2), SelectionOutcome::Measured(samples), AUTO, TTL_NS - 1));
        }
    }
    assert_eq!(cache.len(), 1);
    assert_eq!(cache.lookup(k(1), AUTO, 4), Some(Decision::Metal));
    assert_eq!(cache.lookup(k(2), AUTO, 4), None);
    assert_eq!(cache.lookup(k(1), AUTO, TTL_NS + 1), None, "failed replacement extended original expiry");
}

#[test]
fn explicit_overrides_and_unsupported_dependencies_bypass_reads_and_writes() {
    let mut cache = Cache::new();
    assert!(cache.insert(k(1), metal(), AUTO, 0));
    for policy in [Policy { preference: Override::Cpu, ..AUTO }, Policy { preference: Override::Metal, ..AUTO },
        Policy { eligibility: Eligibility::MappedGeometry, ..AUTO }, Policy { eligibility: Eligibility::UnsupportedTail, ..AUTO },
        Policy { eligibility: Eligibility::ExternalAssetUnversioned, ..AUTO }] {
        assert_eq!(cache.lookup(k(1), policy, 1), None);
        assert!(!cache.insert(k(2), metal(), policy, 1));
    }
    assert_eq!(cache.lookup(k(1), AUTO, 2), Some(Decision::Metal));
    assert_eq!(cache.len(), 1);
}

#[test]
fn unhealthy_device_clears_both_cpu_and_metal_decisions() {
    let mut cache = Cache::new();
    assert!(cache.insert(k(1), cpu(), AUTO, 1));
    assert!(cache.insert(k(2), metal(), AUTO, 1));
    let policy = Policy { eligibility: Eligibility::DeviceUnhealthy, ..AUTO };
    assert_eq!(cache.lookup(k(1), policy, 2), None);
    assert!(!cache.insert(k(3), metal(), policy, 2));
    assert_eq!(cache.len(), 0);
    assert_eq!(cache.lookup(k(2), AUTO, 3), None);
}

#[test]
fn explicit_clear_and_engine_restart_have_no_reusable_entries() {
    let mut cache = Cache::new();
    assert!(cache.insert(k(1), metal(), AUTO, 0));
    cache.clear();
    assert_eq!(cache.len(), 0);
    assert_eq!(cache.lookup(k(1), AUTO, 1), None);
    assert_eq!(Cache::new().lookup(k(1), AUTO, 1), None);
}

#[test]
fn key_retains_every_asset_journal_dimension_and_device_dependency() {
    let settings = DevelopSettings::default(); let config = RendererConfig::default();
    let baseline = key(&input(&settings, &config)).unwrap();
    for field in 0..18 {
        let mut i = input(&settings, &config);
        match field {
            0 => i.asset.owner[0] ^= 1, 1 => i.asset.container_digest[31] ^= 1,
            2 => i.asset.original_digest[31] ^= 1, 3 => i.asset.original_length += 1,
            4 => i.asset.incarnation[31] ^= 1, 5 => i.asset.journal_generation += 1,
            6 => i.asset.recipe_digest[31] ^= 1, 7 => i.asset.dimensions[0] += 1,
            8 => i.asset.dimensions[1] += 1, 9 => i.asset.tier += 1,
            10 => i.asset.format_version += 1, 11 => i.device.generation += 1,
            12 => i.device.adapter_fingerprint[31] ^= 1, 13 => i.device.capability_flags ^= 1,
            14 => i.calibration_level += 1, 15 => i.calibration_extent[0] += 1,
            16 => i.sink_policy_version += 1, _ => i.selector_policy_version += 1,
        }
        assert_ne!(key(&i).unwrap(), baseline, "dependency {field}");
    }
    let mut i = input(&settings, &config); i.calibration_extent[1] += 1;
    assert_ne!(key(&i).unwrap(), baseline);
    i = input(&settings, &config); i.recipe_process.revision += 1;
    assert_ne!(key(&i).unwrap(), baseline);
    i = input(&settings, &config); i.recipe_process.family = ProcessFamily::Adobe;
    assert_ne!(key(&i).unwrap(), baseline);
    for bit in 0..4 {
        i = input(&settings, &config); i.device.capability_flags ^= 1 << bit;
        assert_ne!(key(&i).unwrap(), baseline);
    }
}

#[test]
fn full_hdr_presentation_and_other_settings_change_key_without_mutation() {
    let config = RendererConfig::default(); let original = DevelopSettings::default();
    let baseline = key(&input(&original, &config)).unwrap();
    for field in 0..4 {
        let mut settings = original.clone();
        match field { 0 => settings.output.hdr = true, 1 => settings.output.hdr_headroom_stops = 2.,
            2 => settings.tone.exposure = 0.25, _ => settings.white_balance.mode = WhiteBalanceMode::Daylight }
        let before = serde_json::to_value(&settings).unwrap();
        assert_ne!(key(&input(&settings, &config)).unwrap(), baseline, "settings {field}");
        assert_eq!(serde_json::to_value(&settings).unwrap(), before);
    }
}

#[test]
fn renderer_config_and_ordered_graph_fields_change_key() {
    let settings = DevelopSettings::default(); let original = RendererConfig::default();
    let baseline = key(&input(&settings, &original)).unwrap();
    for field in 0..7 {
        let mut config = original.clone();
        match field { 0 => config.cache_budget_bytes += 1, 1 => config.threads += 1,
            2 => config.process_version.revision += 1, 3 => config.preview_approximations = !config.preview_approximations,
            4 => config.graph = PipelineGraph::m1(),
            5 => config.process_version.family = ProcessFamily::Adobe,
            _ => config.graph = config.graph.with_cacheable(StageId::Tone, true) }
        assert_ne!(key(&input(&settings, &config)).unwrap(), baseline, "config {field}");
    }
}

#[test]
fn identical_inputs_are_deterministic_and_key_failure_is_explicit_bypass() {
    let settings = DevelopSettings::default(); let config = RendererConfig::default();
    assert_eq!(key(&input(&settings, &config)).unwrap(), key(&input(&settings, &config)).unwrap());
    // serde_json may emit null for nonfinite floats: successful JSON encoding
    // alone cannot authorize a cache key. All these must explicitly bypass.
    for bad in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
        for field in 0..3 {
            let mut invalid = settings.clone();
            match field { 0 => invalid.tone.exposure = bad,
                1 => invalid.output.hdr_headroom_stops = bad,
                _ => invalid.tone.curves.rgb.0.push(engine_api::recipe::settings::CurvePoint { x: 0.5, y: bad }) }
            assert!(key(&input(&invalid, &config)).is_err(), "nonfinite field {field}");
        }
    }
}

#[test]
fn expired_entries_are_pruned_on_insert_and_timestamp_arithmetic_does_not_overflow() {
    let mut cache = Cache::new();
    for n in 0..16 { assert!(cache.insert(k(n), metal(), AUTO, 0)); }
    assert!(cache.insert(k(20), metal(), AUTO, TTL_NS));
    assert_eq!(cache.len(), 1);
    for n in 0..16 { assert_eq!(cache.lookup(k(n), AUTO, TTL_NS), None); }
    assert!(cache.insert(k(21), metal(), AUTO, u64::MAX - 100));
    assert_eq!(cache.lookup(k(21), AUTO, u64::MAX), Some(Decision::Metal));
}

#[test]
fn concurrent_completed_misses_can_publish_same_key_without_extra_slots() {
    use std::sync::{Arc, Barrier, Mutex};
    let cache = Arc::new(Mutex::new(Cache::new()));
    let ready = Arc::new(Barrier::new(4));
    let workers: Vec<_> = (0..4).map(|_| {
        let cache = cache.clone(); let ready = ready.clone();
        std::thread::spawn(move || {
            // Simulates four independent completed measurements: no singleflight
            // promise, no work done while holding the small cache mutex.
            ready.wait();
            cache.lock().unwrap().insert(k(1), metal(), AUTO, 1)
        })
    }).collect();
    for worker in workers { assert!(worker.join().unwrap()); }
    let mut cache = cache.lock().unwrap();
    assert_eq!(cache.len(), 1);
    assert_eq!(cache.lookup(k(1), AUTO, 2), Some(Decision::Metal));
}

#[test]
fn complete_vector_payload_and_full_container_digest_affect_key() {
    use engine_api::recipe::settings::CurvePoint;
    let config = RendererConfig::default(); let mut settings = DevelopSettings::default();
    settings.tone.curves.rgb.0 = (0..4097).map(|n| CurvePoint { x: n as f32 / 4096., y: n as f32 / 4096. }).collect();
    let a = key(&input(&settings, &config)).unwrap();
    settings.tone.curves.rgb.0.last_mut().unwrap().y = 0.99;
    assert_ne!(key(&input(&settings, &config)).unwrap(), a);
    let mut i = input(&settings, &config);
    let b = key(&i).unwrap();
    // Same first128bits/renderId must not collide after container tail changes.
    i.asset.container_digest[31] ^= 1;
    assert_ne!(key(&i).unwrap(), b);
}

#[test]
fn graph_fingerprint_includes_stage_frame_order_count_and_flags() {
    let original = PipelineGraph::m2().nodes().to_vec();
    let baseline = graph_fingerprint(&original);
    for field in 0..6 {
        let mut nodes = original.clone();
        match field {
            0 => nodes[0].stage = StageId::Output,
            1 => nodes[0].frame = image_core::graph::Frame::Output,
            2 => nodes.swap(0, 1),
            3 => { nodes.pop(); },
            4 => nodes[0].cacheable = !nodes[0].cacheable,
            _ => nodes[0].implemented = !nodes[0].implemented,
        }
        assert_ne!(graph_fingerprint(&nodes), baseline, "graph field {field}");
    }
}
