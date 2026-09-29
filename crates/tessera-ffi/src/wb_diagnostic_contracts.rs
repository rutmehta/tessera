//! Test-only harness contracts. No Engine/render observer is wired yet.
#![allow(dead_code)]
use image_core::wb_diagnostic::{Error, Snapshot};

#[derive(Clone, Copy)]
struct Admission {
    automatic: bool,
    first_metal: bool,
    reopened_metal: bool,
    candidate: bool,
    second_lookup_hits: u64,
    second_measurements: u64,
    second_publications: u64,
    identical_key: bool,
    original_wb: [u8; 32],
    custom_wb: [u8; 32],
}
fn admit(facts: Admission) -> Result<bool, Error> {
    let common = facts.automatic
        && facts.first_metal
        && facts.reopened_metal
        && facts.original_wb != facts.custom_wb;
    let decision = if facts.candidate {
        facts.second_lookup_hits == 1
            && facts.second_measurements == 0
            && facts.second_publications == 0
            && facts.identical_key
    } else {
        facts.second_lookup_hits == 0
            && facts.second_measurements == 1
            && facts.second_publications == 0
    };
    Ok(common && decision)
}
fn real_phase_capture(
    _candidate: bool,
    _edr: bool,
    _fixture: &std::path::Path,
) -> Result<Snapshot, Error> {
    Err(Error::Unsupported)
}
fn valid() -> Admission {
    Admission {
        automatic: true,
        first_metal: true,
        reopened_metal: true,
        candidate: true,
        second_lookup_hits: 1,
        second_measurements: 0,
        second_publications: 0,
        identical_key: true,
        original_wb: [1; 32],
        custom_wb: [2; 32],
    }
}
#[test]
fn actual_auto_metal_and_live_unchanged_hit_are_required() {
    assert!(admit(valid()).unwrap());
    for f in [
        Admission {
            automatic: false,
            ..valid()
        },
        Admission {
            first_metal: false,
            ..valid()
        },
        Admission {
            reopened_metal: false,
            ..valid()
        },
        Admission {
            second_lookup_hits: 0,
            ..valid()
        },
        Admission {
            second_measurements: 1,
            ..valid()
        },
        Admission {
            second_publications: 1,
            ..valid()
        },
        Admission {
            identical_key: false,
            ..valid()
        },
    ] {
        assert!(!admit(f).unwrap());
    }
}
#[test]
fn equal_resolved_custom_matrix_is_inconclusive_without_substitution() {
    assert!(
        !admit(Admission {
            custom_wb: [1; 32],
            ..valid()
        })
        .unwrap()
    );
}
#[test]
fn baseline_requires_auto_metal_but_does_not_fabricate_cache_hits() {
    assert!(
        admit(Admission {
            candidate: false,
            second_lookup_hits: 0,
            second_measurements: 1,
            second_publications: 0,
            ..valid()
        })
        .unwrap()
    );
}
fn actual(candidate: bool, edr: bool) {
    let path = std::path::PathBuf::from(
        std::env::var_os("TESSERA_SMART_PREVIEW_RAW").expect("explicit read-only fixture required"),
    );
    let phases =
        real_phase_capture(candidate, edr, &path).expect("real phase instrumentation absent");
    assert_eq!(phases.overflow, 0);
    assert!(phases.len > 0);
    // Future implementation must assert full recipe/actual route/TTL/key,
    // exact first Daylight/repeat/custom matrix and existing frame/fidelity/
    // source hashes/Weak release contracts before returning copied records.
    // This Unsupported seam is instrumentation RED, never priming evidence.
}
#[test]
#[ignore = "exclusive runtime; actual phase instrumentation not implemented"]
fn actual_baseline_sdr_phase_capture() {
    actual(false, false);
}
#[test]
#[ignore = "exclusive runtime; actual phase instrumentation not implemented"]
fn actual_baseline_edr_phase_capture() {
    actual(false, true);
}
#[test]
#[ignore = "exclusive runtime; actual phase instrumentation not implemented"]
fn actual_candidate_sdr_phase_capture() {
    actual(true, false);
}
#[test]
#[ignore = "exclusive runtime; actual phase instrumentation not implemented"]
fn actual_candidate_edr_phase_capture() {
    actual(true, true);
}

/// C6 (rev7 R4): must-pass non-regression guard. Under an armed epoch the
/// measured/failed calibration controls bypass `measure_at`, so they make no
/// reservation and leave the CPU-iteration counter unchanged. C2' (same C run)
/// is the positive control for reservation detection.
#[test]
#[cfg(target_os = "macos")]
fn calibration_controls_make_no_reservation() {
    use crate::develop::proxy_cache_contracts::{SelectionControl, SelectionState};
    use image_core::wb_diagnostic::{ARENA, State, harness::EpochGuard};
    let g = EpochGuard::open().expect(
        "EpochGuard: requires --test-threads=1 and an Idle ARENA (an earlier test may have leaked a live lease)",
    );
    g.epoch().arm(1).unwrap();
    use crate::backend::common;
    let image = common::synthetic(606, 64, 48, common::RGGB, [0, 0, 64, 48]);
    let device = gpu_core::GpuDevice::new().ok();
    assert!(
        device.is_some(),
        "C6 needs a Metal device to reach the controls"
    );
    for (label, control, metal) in [
        ("MeasuredCpu", SelectionControl::MeasuredCpu, false),
        ("MeasuredMetal", SelectionControl::MeasuredMetal, true),
        (
            "CalibrationFailure",
            SelectionControl::CalibrationFailure,
            false,
        ),
    ] {
        let before = ARENA.counts().expect("arena counts");
        assert_eq!(ARENA.state().unwrap(), State::Open);
        let observer = std::sync::Mutex::new(SelectionState {
            control: Some(control),
            ..Default::default()
        });
        let backend = crate::backend::select_proxy(
            &image,
            &engine_api::recipe::DevelopSettings::default(),
            || device.clone(),
            None,
            Some(&observer),
        );
        assert_eq!(backend.name.starts_with("Metal"), metal, "{label}");
        let after = ARENA.counts().expect("arena counts");
        assert_eq!(ARENA.state().unwrap(), State::Open);
        assert_eq!(
            (after.reserved, after.active, after.completed),
            (0, 0, 0),
            "{label}"
        );
        assert_eq!(
            after.cpu_iterations_unobserved, before.cpu_iterations_unobserved,
            "{label}"
        );
        assert_eq!(after.loss, 0);
    }
}
