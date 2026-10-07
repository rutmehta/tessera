//! ENG-7: the default lens mode matches Lightroom.
//!
//! `LensProfileSource::Auto` (the recipe default) applies the camera's embedded
//! correction when the raw carries one, else a supplied or database profile,
//! else nothing. It never applies a distortion or vignette estimated from image
//! content; that is the explicit `LensProfileSource::AutoCalibrated` opt-in.
//! A named profile that is not available applies nothing (it used to fail).
#[path = "common/raw_fixtures.rs"]
mod raw_fixtures;

use engine_api::recipe::{
    DevelopSettings,
    settings::{LensProfileRef, LensProfileSource},
};
use lens::{BrownConrady, CalibrationSample, Profile, ProfileDatabase};
use pipeline_cpu::{
    CorrectionSource, Image, LensContext, RenderSource, ResolvedLens, render_linear_scaled,
    render_linear_scaled_with_lens, resolve_lens, resolve_lens_sensor,
};
use raw_decode::{CfaLayout, RawMetadata};

/// Radial light falloff: auto-calibration estimates a -0.2 vignette from it.
fn falloff() -> Image {
    let (w, h) = (65, 65);
    let p: Vec<f32> = (0..w * h)
        .map(|i| {
            let x = 2. * (i % w) as f32 / (w - 1) as f32 - 1.;
            let y = 2. * (i / w) as f32 / (h - 1) as f32 - 1.;
            0.5 * (1. - 0.2 * (x * x + y * y))
        })
        .collect();
    Image::new(w, h, vec![p; 3]).unwrap()
}

/// Two straight vertical edges imaged through a k1 = 0.12 barrel lens.
fn barrel() -> Image {
    let n = 192_u32;
    let model = BrownConrady {
        k1: 0.12,
        ..Default::default()
    };
    let mut p = Vec::new();
    for y in 0..n {
        for x in 0..n {
            let q = model
                .undistort([
                    2. * f64::from(x) / f64::from(n - 1) - 1.,
                    2. * f64::from(y) / f64::from(n - 1) - 1.,
                ])
                .unwrap();
            p.push((0.5 + 0.5 * ((q[0].abs() - 0.53) * 150.).tanh()) as f32);
        }
    }
    Image::new(n, n, vec![p; 3]).unwrap()
}

/// Detail is irrelevant here and only slows the comparison renders.
fn settings(profile: LensProfileSource) -> DevelopSettings {
    let mut s = DevelopSettings::default();
    s.lens.profile = profile;
    s.detail.sharpening.amount = 0.;
    s.detail.noise_reduction.color = 0.;
    s
}

fn render(s: &DevelopSettings, image: &Image, context: &LensContext<'_>) -> Image {
    render_linear_scaled_with_lens(s, &RenderSource::Rgb(image), 1, context).unwrap()
}

fn estimated_geometry(r: &ResolvedLens) -> bool {
    r.sample().is_some_and(|s| {
        s.distortion != BrownConrady::default() || s.vignette != [0.; 3]
    })
}

#[test]
fn recipe_default_is_auto() {
    assert_eq!(
        DevelopSettings::default().lens.profile,
        LensProfileSource::Auto
    );
}

#[test]
fn default_auto_never_applies_an_estimate_from_image_content() {
    for (name, image) in [("falloff", falloff()), ("barrel", barrel())] {
        let auto = settings(LensProfileSource::Auto);
        let r = resolve_lens(&image, &auto.lens, None, &LensContext::default()).unwrap();
        assert!(
            !estimated_geometry(&r),
            "{name}: Auto applied an image estimate: {:?} {:?}",
            r.source(),
            r.sample()
        );
        assert_eq!(r.source(), CorrectionSource::Manual, "{name}");
        // Pixel level: with no embedded data and no profile, Auto is lens-off.
        let none = settings(LensProfileSource::None);
        assert_eq!(
            render(&auto, &image, &LensContext::default()).planes(),
            render(&none, &image, &LensContext::default()).planes(),
            "{name}: Auto render differs from profile None"
        );
    }
}

#[test]
fn explicit_auto_calibrated_still_estimates() {
    let s = settings(LensProfileSource::AutoCalibrated);
    let image = falloff();
    let r = resolve_lens(&image, &s.lens, None, &LensContext::default()).unwrap();
    assert_eq!(r.source(), CorrectionSource::Image);
    assert!((r.sample().unwrap().vignette[0] + 0.2).abs() < 0.02);
    let none = settings(LensProfileSource::None);
    assert_ne!(
        render(&s, &image, &LensContext::default()).planes(),
        render(&none, &image, &LensContext::default()).planes()
    );
    let image = barrel();
    let r = resolve_lens(&image, &s.lens, None, &LensContext::default()).unwrap();
    assert_eq!(r.source(), CorrectionSource::Image);
    let k1 = r.sample().unwrap().distortion.k1;
    assert!(k1.abs() > 0.01, "estimated k1 {k1}");
}

fn metadata(opcodes: Option<Vec<u8>>, lens: Option<&str>) -> RawMetadata {
    RawMetadata {
        make: "test".into(),
        model: "test".into(),
        lens: lens.map(Into::into),
        iso: 100.,
        shutter_s: 0.01,
        aperture: 4.,
        focal_mm: 50.,
        capture_time: 0,
        orientation: 1,
        catalog_orientation: None,
        baseline_exposure: 0.,
        width: 65,
        height: 65,
        cfa_layout: CfaLayout::Bayer([[0, 1], [3, 2]]),
        black_levels: [0.; 4],
        white_level: 65535,
        as_shot_wb: [1.; 4],
        camera_to_xyz: engine_api::color::ColorMatrix3::IDENTITY,
        cam_xyz: [[1., 0., 0.], [0., 1., 0.], [0., 0., 1.], [0., 0., 0.]],
        rgb_cam: [[1., 0., 0., 0.], [0., 1., 0., 0.], [0., 0., 1., 0.]],
        default_crop: [0, 0, 65, 65],
        has_gain_map: false,
        has_opcode_list: opcodes.is_some(),
        opcode_lists: [None, None, opcodes],
    }
}

/// One DNG FixVignetteRadial opcode (OpcodeList3).
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

#[test]
fn embedded_correction_is_unchanged_in_default_mode() {
    let m = metadata(Some(vignette_opcode()), None);
    let s = settings(LensProfileSource::Auto);
    let r = resolve_lens(&falloff(), &s.lens, Some(&m), &LensContext::default()).unwrap();
    assert_eq!(r.source(), CorrectionSource::Embedded);
    assert!(r.sample().is_none());
    // Explicit Embedded resolves identically.
    let e = settings(LensProfileSource::Embedded);
    let r2 = resolve_lens(&falloff(), &e.lens, Some(&m), &LensContext::default()).unwrap();
    assert_eq!(format!("{r:?}"), format!("{r2:?}"));
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
fn database_profile_is_unchanged_in_default_mode() {
    let database = ProfileDatabase {
        profiles: vec![profile("Test Prime 50")],
    };
    let context = LensContext {
        database: Some(&database),
        ..Default::default()
    };
    let m = metadata(None, Some("Test Prime 50"));
    let s = settings(LensProfileSource::Auto);
    let r = resolve_lens(&falloff(), &s.lens, Some(&m), &context).unwrap();
    assert_eq!(r.source(), CorrectionSource::Database);
    assert_eq!(r.sample().unwrap().distortion.k1, -0.05);
    // A database without a matching lens applies nothing, never an estimate.
    let m = metadata(None, Some("Unknown Zoom"));
    let r = resolve_lens(&falloff(), &s.lens, Some(&m), &context).unwrap();
    assert_eq!(r.source(), CorrectionSource::Manual);
    assert!(r.sample().is_none());
}

/// Lightroom `LensProfileEnable=1` with a named profile Tessera does not have:
/// nothing is applied (the import records an info note); never an estimate,
/// and never a render failure.
#[test]
fn unavailable_named_profile_applies_nothing() {
    let named = LensProfileSource::Database {
        profile: LensProfileRef::named("Adobe (Synthetic Unavailable Lens)"),
    };
    let s = settings(named.clone());
    for image in [falloff(), barrel()] {
        let r = resolve_lens(&image, &s.lens, None, &LensContext::default()).unwrap();
        assert_eq!(r.source(), CorrectionSource::Manual);
        assert!(r.sample().is_none());
        let none = settings(LensProfileSource::None);
        assert_eq!(
            render(&s, &image, &LensContext::default()).planes(),
            render(&none, &image, &LensContext::default()).planes()
        );
    }
    // The same name, when available, is applied exactly.
    let database = ProfileDatabase {
        profiles: vec![profile("Adobe (Synthetic Unavailable Lens)")],
    };
    let context = LensContext {
        database: Some(&database),
        ..Default::default()
    };
    let r = resolve_lens(&falloff(), &s.lens, None, &context).unwrap();
    assert_eq!(r.source(), CorrectionSource::Database);
    assert_eq!(r.sample().unwrap().distortion.k1, -0.05);
}

/// All five real RAW fixtures: default settings apply no estimated geometry.
/// Before ENG-7 the CR3 (k1 about -0.10) and RAF (about -0.13) were corrected
/// by an image-content estimate. The explicit opt-in still estimates.
#[test]
fn raw_fixtures_default_applies_no_estimated_geometry() {
    const TEST: &str = "raw_fixtures_default_applies_no_estimated_geometry";
    let files = raw_fixtures::all(TEST);
    let mut failures = Vec::new();
    for path in &files {
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        let mut source = raw_decode::RawSource::open(path).unwrap();
        let cfa = source.decode_cfa().unwrap();
        let metadata = source.metadata();
        let plane = cfa.pyramid().pixels();
        let ctx = LensContext::default();
        let auto = settings(LensProfileSource::Auto);
        let none = settings(LensProfileSource::None);
        let calibrated = settings(LensProfileSource::AutoCalibrated);
        let r_auto = resolve_lens_sensor(plane, &metadata, &auto, &ctx).unwrap();
        let r_none = resolve_lens_sensor(plane, &metadata, &none, &ctx).unwrap();
        let r_cal = resolve_lens_sensor(plane, &metadata, &calibrated, &ctx).unwrap();
        raw_fixtures::notice(
            TEST,
            &format!(
                "{name}: Auto {:?}; AutoCalibrated {:?} k1 {:?}",
                r_auto.source(),
                r_cal.source(),
                r_cal.sample().map(|s| s.distortion.k1)
            ),
        );
        if r_auto.source() == CorrectionSource::Embedded {
            // Camera-embedded correction: Auto keeps applying it unchanged.
            continue;
        }
        if estimated_geometry(&r_auto) {
            failures.push(format!(
                "{name}: Auto applied an estimate {:?}",
                r_auto.sample()
            ));
            continue;
        }
        if format!("{:?}", r_auto.sample()) != format!("{:?}", r_none.sample()) {
            failures.push(format!("{name}: Auto resolution differs from None"));
            continue;
        }
        let src = RenderSource::Cfa {
            image: &cfa,
            metadata: &metadata,
        };
        let a = render_linear_scaled(&auto, &src, 8).unwrap();
        let b = render_linear_scaled(&none, &src, 8).unwrap();
        if a.planes() != b.planes() {
            failures.push(format!("{name}: default render differs from lens-off"));
        }
    }
    assert!(failures.is_empty(), "{failures:#?}");
}
