//! ENG-7b: built-in corrections, the CA default, and lens notices.
//!
//! Lightroom always applies a camera's built-in lens correction (here: DNG
//! opcode lists carried by the raw), whatever the profile setting. Tessera
//! applies the embedded opcodes in `None` and for a named profile that is not
//! available, not only in `Auto`/`Embedded`. Explicitly chosen available
//! profiles and the `AutoCalibrated` opt-in keep their behaviour.
//! "Remove Chromatic Aberration" defaults to off, like Lightroom's Adobe
//! Default for most cameras.
use engine_api::recipe::{
    DevelopSettings,
    settings::{LensProfileRef, LensProfileSource, LensSettings},
};
use lens::{BrownConrady, CalibrationSample, Profile, ProfileDatabase};
use pipeline_cpu::{
    CorrectionSource, Image, LensContext, LensNotice, RenderSource, lens_notice,
    render_linear_scaled_with_lens, resolve_lens,
};
use raw_decode::{CfaImage, CfaLayout, RawMetadata};

const W: u32 = 64;
const H: u32 = 48;

/// Synthetic DNG metadata. `opcodes` is the OpcodeList3 payload the DNG
/// reader (`raw_decode::dng::extract_dng_opcode_lists`) hands the engine.
fn metadata(opcodes: Option<Vec<u8>>) -> RawMetadata {
    RawMetadata {
        make: "test".into(),
        model: "test".into(),
        lens: Some("Test Zoom".into()),
        iso: 100.,
        shutter_s: 0.01,
        aperture: 4.,
        focal_mm: 50.,
        capture_time: 0,
        orientation: 1,
        catalog_orientation: None,
        baseline_exposure: 0.,
        width: W,
        height: H,
        cfa_layout: CfaLayout::Bayer([[0, 1], [3, 2]]),
        black_levels: [0.; 4],
        white_level: 65535,
        as_shot_wb: [1.; 4],
        camera_to_xyz: engine_api::color::ColorMatrix3::IDENTITY,
        cam_xyz: [[1., 0., 0.], [0., 1., 0.], [0., 0., 1.], [0., 0., 0.]],
        rgb_cam: [[1., 0., 0., 0.], [0., 1., 0., 0.], [0., 0., 1., 0.]],
        default_crop: [0, 0, W, H],
        has_gain_map: false,
        has_opcode_list: opcodes.is_some(),
        opcode_lists: [None, None, opcodes],
        maker_lens: None,
    }
}

/// One DNG FixVignetteRadial opcode: a strong, easily measured built-in
/// correction that brightens the corners.
fn vignette_opcode() -> Vec<u8> {
    let mut b = Vec::new();
    for x in [1_u32, 3, 0x01030000, 0, 56] {
        b.extend(x.to_be_bytes());
    }
    for x in [0.5_f64, 0., 0., 0., 0., 0.5, 0.5] {
        b.extend(x.to_be_bytes());
    }
    b
}

fn cfa() -> CfaImage {
    CfaImage::from_linear(W, H, vec![0.25; (W * H) as usize]).unwrap()
}

fn analysis() -> Image {
    Image::new(W, H, vec![vec![0.25; (W * H) as usize]; 3]).unwrap()
}

fn settings(profile: LensProfileSource) -> DevelopSettings {
    let mut s = DevelopSettings::default();
    s.lens.profile = profile;
    s.detail.sharpening.amount = 0.;
    s.detail.noise_reduction.color = 0.;
    s
}

fn named(name: &str) -> LensProfileSource {
    LensProfileSource::Database {
        profile: LensProfileRef::named(name),
    }
}

fn render(s: &DevelopSettings, m: &RawMetadata, context: &LensContext<'_>) -> Image {
    let cfa = cfa();
    render_linear_scaled_with_lens(
        s,
        &RenderSource::Cfa {
            image: &cfa,
            metadata: m,
        },
        1,
        context,
    )
    .unwrap()
}

fn profile(model: &str) -> Profile {
    Profile {
        camera: None,
        maker: "test".into(),
        model: model.into(),
        samples: vec![CalibrationSample {
            distortion: BrownConrady {
                k1: -0.05,
                ..Default::default()
            },
            ..Default::default()
        }],
    }
}

#[test]
fn remove_chromatic_aberration_defaults_off() {
    assert!(!LensSettings::default().remove_chromatic_aberration);
    assert!(!DevelopSettings::default().lens.remove_chromatic_aberration);
    // A saved recipe stores the field explicitly and keeps its value.
    let on: LensSettings =
        serde_json::from_str(r#"{"remove_chromatic_aberration": true}"#).unwrap();
    assert!(on.remove_chromatic_aberration);
    // Only a recipe that never stored the field takes the new default.
    let absent: LensSettings = serde_json::from_str("{}").unwrap();
    assert!(!absent.remove_chromatic_aberration);
}

#[test]
fn built_in_correction_applies_in_none_and_for_an_unavailable_profile() {
    let m = metadata(Some(vignette_opcode()));
    let ctx = LensContext::default();
    let embedded = settings(LensProfileSource::Embedded);
    let reference = render(&embedded, &m, &ctx);
    let without = {
        let off = settings(LensProfileSource::None);
        render(&off, &metadata(None), &ctx)
    };
    assert_ne!(
        reference.planes(),
        without.planes(),
        "the opcode must be visible"
    );
    for (what, mode) in [
        ("none", LensProfileSource::None),
        ("unavailable named", named("Adobe (Synthetic Missing Lens)")),
        ("auto", LensProfileSource::Auto),
    ] {
        let s = settings(mode);
        let r = resolve_lens(&analysis(), &s.lens, Some(&m), &ctx).unwrap();
        assert_eq!(r.source(), CorrectionSource::Embedded, "{what}");
        assert_eq!(
            render(&s, &m, &ctx).planes(),
            reference.planes(),
            "{what}: built-in correction not applied"
        );
    }
}

#[test]
fn available_profile_and_auto_calibrated_keep_their_behaviour_with_built_in_data() {
    let m = metadata(Some(vignette_opcode()));
    let database = ProfileDatabase {
        profiles: vec![profile("Adobe (Synthetic Lens)")],
    };
    let ctx = LensContext {
        database: Some(&database),
        ..Default::default()
    };
    let s = settings(named("Adobe (Synthetic Lens)"));
    let r = resolve_lens(&analysis(), &s.lens, Some(&m), &ctx).unwrap();
    assert_eq!(r.source(), CorrectionSource::Database);
    assert_eq!(r.sample().unwrap().distortion.k1, -0.05);
    let s = settings(LensProfileSource::AutoCalibrated);
    let r = resolve_lens(&analysis(), &s.lens, Some(&m), &LensContext::default()).unwrap();
    assert_ne!(r.source(), CorrectionSource::Embedded);
}

#[test]
fn lens_notices() {
    let raw = metadata(None);
    let built_in = metadata(Some(vignette_opcode()));
    let ctx = LensContext::default();
    let missing = named("Adobe (Synthetic Missing Lens)");
    let notice = |profile: LensProfileSource, m: Option<&RawMetadata>| {
        let s = settings(profile);
        lens_notice(&s.lens, m, &ctx)
    };
    // A named profile that is not available.
    let n = notice(missing.clone(), Some(&raw)).unwrap();
    assert_eq!(
        n,
        LensNotice::ProfileUnavailable {
            name: "Adobe (Synthetic Missing Lens)".into(),
            built_in: false
        }
    );
    assert_eq!(
        n.to_string(),
        "Lens profile 'Adobe (Synthetic Missing Lens)' not available — no profile correction applied"
    );
    let n = notice(missing.clone(), Some(&built_in)).unwrap();
    assert!(
        n.to_string().contains("built-in lens correction applied"),
        "{n}"
    );
    // Also for an RGB source (no raw metadata).
    assert!(matches!(
        notice(missing, None),
        Some(LensNotice::ProfileUnavailable { .. })
    ));
    // Auto on a raw with neither built-in data nor a profile.
    let n = notice(LensProfileSource::Auto, Some(&raw)).unwrap();
    assert_eq!(n, LensNotice::NoProfile);
    assert_eq!(
        n.to_string(),
        "No lens profile available — no profile correction applied"
    );
    // Nothing to say when a correction applies, when correction is off, or for RGB Auto.
    assert_eq!(notice(LensProfileSource::Auto, Some(&built_in)), None);
    assert_eq!(notice(LensProfileSource::None, Some(&raw)), None);
    assert_eq!(notice(LensProfileSource::AutoCalibrated, Some(&raw)), None);
    assert_eq!(notice(LensProfileSource::Auto, None), None);
    let database = ProfileDatabase {
        profiles: vec![profile("Test Zoom")],
    };
    let with_db = LensContext {
        database: Some(&database),
        ..Default::default()
    };
    let s = settings(LensProfileSource::Auto);
    assert_eq!(lens_notice(&s.lens, Some(&raw), &with_db), None);
}
