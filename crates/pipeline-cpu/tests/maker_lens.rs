//! ENG-8: camera built-in corrections stored in maker notes (Fujifilm RAF).
//!
//! Lightroom applies a Fujifilm X-series camera's built-in distortion,
//! vignetting and lateral CA correction whatever the profile setting, like a
//! DNG opcode correction. Tessera represents it as a calibration sample with
//! source `MakerNote`, resolved in every mode that applies built-in
//! corrections (ENG-7b).
//!
//! Independent references:
//! - the Fujifilm spline model as darktable implements it (`src/iop/lens.cc`
//!   `_init_coeffs_md_v2`, Fuji branch, with its autoscale), transcribed here
//!   on its own, not through Tessera's code;
//! - the camera's own embedded JPEG, which the X-E2S renders with its
//!   built-in distortion correction applied (a geometric sanity check).
use engine_api::recipe::{
    DevelopSettings,
    settings::{LensProfileRef, LensProfileSource},
};
use lens::{BrownConrady, CalibrationSample, Profile};
use pipeline_cpu::{
    CorrectionSource, LensContext, LensNotice, RenderSource, lens_notice, render_linear_scaled,
    render_scaled, resolve_lens, resolve_lens_sensor,
};
use raw_decode::{CfaLayout, FujifilmLens, MakerLens, RawMetadata, RawSource};
use test_fixtures::raw as raw_fixtures;

/// The X-E2S fixture's FujiIFD values (ExifTool), at 31.5 mm.
fn xe2s() -> FujifilmLens {
    FujifilmLens {
        knots: (0..=10).map(|i| i as f64 / 10.).collect(),
        distortion: vec![
            0., 0.102, 0.205, 0.307, 0.408, 0.517, 0.66, 0.879, 1.184, 1.598, 2.158,
        ],
        ca_red: vec![
            0., 0.000103, 0.000188, 0.000238, 0.000235, 0.000183, 0.000102, 7e-06, -0.000102,
            -0.000227, -0.000366,
        ],
        ca_blue: vec![
            0., -3.8e-05, -6.6e-05, -7.1e-05, -4.2e-05, 3e-05, 0.00014, 0.000235, 0.000376,
            0.00054, 0.000793,
        ],
        vignetting: vec![
            100., 99.92, 99.77, 99.46, 98.94, 98.43, 98.11, 97.29, 96.78, 95.88, 94.9,
        ],
        crop_factor: 1.,
    }
}

/// A barrel-correcting lens (negative distortion values) with a 1.25x crop.
fn barrel() -> FujifilmLens {
    FujifilmLens {
        knots: (0..9).map(|i| 0.05 + i as f64 / 9.).collect(),
        distortion: (0..9).map(|i| -0.35 * i as f64).collect(),
        ca_red: (0..9).map(|i| 4e-5 * i as f64).collect(),
        ca_blue: (0..9).map(|i| -6e-5 * i as f64).collect(),
        // A smooth (even) falloff, like real lenses: the sample's illumination
        // polynomial has no odd terms, so a spline linear in r near the
        // centre fits to about 1 % only.
        vignetting: (0..9)
            .map(|i| {
                let r = 1.25 * (0.05 + i as f64 / 9.);
                100. - 20. * r * r
            })
            .collect(),
        crop_factor: 1.25,
    }
}

/// Full-resolution active area of the X-E2S fixture.
const W: u32 = 4896;
const H: u32 = 3264;

fn metadata(lens: Option<FujifilmLens>, opcodes: Option<Vec<u8>>) -> RawMetadata {
    RawMetadata {
        make: "FUJIFILM".into(),
        model: "X-E2S".into(),
        lens: Some("XF18-55mmF2.8-4 R LM OIS".into()),
        iso: 200.,
        shutter_s: 0.01,
        aperture: 9.,
        focal_mm: 31.5,
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
        maker_lens: lens.map(MakerLens::Fujifilm),
    }
}

fn settings(profile: LensProfileSource) -> DevelopSettings {
    let mut s = DevelopSettings::default();
    s.lens.profile = profile;
    s
}

fn named(name: &str) -> LensProfileSource {
    LensProfileSource::Database {
        profile: LensProfileRef::named(name),
    }
}

fn rgb() -> pipeline_cpu::Image {
    pipeline_cpu::Image::new(64, 48, vec![vec![0.25; 64 * 48]; 3]).unwrap()
}

fn resolve(s: &DevelopSettings, m: &RawMetadata) -> pipeline_cpu::ResolvedLens {
    resolve_lens(&rgb(), &s.lens, Some(m), &LensContext::default()).unwrap()
}

/// Linear spline as darktable's `_interpolate_linear_spline`: clamped ends.
fn spline(x: &[f64], y: &[f64], at: f64) -> f64 {
    if at < x[0] {
        return y[0];
    }
    for i in 1..x.len() {
        if at >= x[i - 1] && at <= x[i] {
            return y[i - 1] + (at - x[i - 1]) * (y[i] - y[i - 1]) / (x[i] - x[i - 1]);
        }
    }
    y[x.len() - 1]
}

/// darktable `_init_coeffs_md_v2` (Fuji) plus its autoscale: the knots (in
/// output half-diagonal units) and per-channel source/output radius ratios.
fn darktable(f: &FujifilmLens, w: f64, h: f64) -> (Vec<f64>, [Vec<f64>; 3]) {
    const MAXKNOTS: usize = 16;
    let (mut knots_in, mut cor_in, mut car_in, mut cab_in) = (vec![], vec![], vec![], vec![]);
    if f.knots[0] > 0. {
        knots_in.push(0.);
        cor_in.push(1.);
        car_in.push(0.);
        cab_in.push(0.);
    }
    for i in 0..f.knots.len() {
        knots_in.push(f.crop_factor * f.knots[i]);
        cor_in.push(f.distortion[i] / 100. + 1.);
        car_in.push(f.ca_red[i]);
        cab_in.push(f.ca_blue[i]);
    }
    let mut knots = vec![0.; MAXKNOTS];
    let mut cor: [Vec<f64>; 3] = std::array::from_fn(|_| vec![0.; MAXKNOTS]);
    for i in 0..MAXKNOTS {
        let rin = i as f64 / (MAXKNOTS - 1) as f64;
        let m = spline(&knots_in, &cor_in, rin);
        knots[i] = rin / m;
        cor[0][i] = m * (spline(&knots_in, &car_in, rin) + 1.);
        cor[1][i] = m;
        cor[2][i] = m * (spline(&knots_in, &cab_in, rin) + 1.);
    }
    let r = (w / 2.).hypot(h / 2.);
    let srr = (w / 2.).min(h / 2.) / r;
    let mut scale = 0f64;
    for i in 0..200 {
        for c in &cor {
            let x = srr + (1. - srr) * i as f64 / 199.;
            scale = scale.max(spline(&knots, c, x));
        }
    }
    for i in 0..MAXKNOTS {
        knots[i] *= scale;
        for c in &mut cor {
            c[i] /= scale;
        }
    }
    (knots, cor)
}

/// Pixel offsets of the fitted sample from darktable's model over the frame:
/// (max, rms) for the green geometry and the max red/blue CA offset.
fn geometry_error(f: &FujifilmLens, sample: &CalibrationSample) -> (f64, f64, f64) {
    let (w, h) = (W as f64, H as f64);
    let (knots, cor) = darktable(f, w, h);
    let hd = (w / 2.).hypot(h / 2.);
    let (mut max, mut sum, mut n, mut ca) = (0f64, 0., 0., 0f64);
    for j in 0..=48 {
        for i in 0..=72 {
            let p = [i as f64 / 36. - 1., j as f64 / 24. - 1.];
            let px = [p[0] * w / 2., p[1] * h / 2.];
            let rho = px[0].hypot(px[1]) / hd;
            let q = sample.distort(p);
            let reference = spline(&knots, &cor[1], rho);
            let e = (q[0] * w / 2. - px[0] * reference).hypot(q[1] * h / 2. - px[1] * reference);
            max = max.max(e);
            sum += e * e;
            n += 1.;
            // CA: channel radius / green radius at the green source radius.
            let green = [q[0] * w / 2., q[1] * h / 2.];
            let rg = green[0].hypot(green[1]) / hd;
            let x = (q[0] - sample.distortion.cx) * sample.coordinate_scale[0];
            let y = (q[1] - sample.distortion.cy) * sample.coordinate_scale[1];
            let r2 = x * x + y * y;
            for (c, k) in [(sample.ca_red, 0), (sample.ca_blue, 2)] {
                let ratio = c[0] + r2 * (c[1] + r2 * c[2]);
                let reference = spline(&knots, &cor[k], rho) / spline(&knots, &cor[1], rho);
                ca = ca.max((ratio - reference).abs() * rg * hd);
            }
        }
    }
    (max, (sum / n).sqrt(), ca)
}

#[test]
fn maker_note_sample_matches_the_darktable_spline_model() {
    for (name, f) in [
        ("X-E2S 31.5mm", xe2s()),
        ("synthetic barrel 1.25x", barrel()),
    ] {
        let m = metadata(Some(f.clone()), None);
        let r = resolve(&settings(LensProfileSource::Auto), &m);
        assert_eq!(r.source(), CorrectionSource::MakerNote, "{name}");
        let sample = r.sample().expect("maker-note sample");
        let (max, rms, ca) = geometry_error(&f, sample);
        eprintln!("{name}: geometry max {max:.3} px rms {rms:.3} px, CA max {ca:.3} px");
        assert!(
            max <= 0.5 && rms <= 0.15,
            "{name}: geometry {max} / {rms} px"
        );
        assert!(ca <= 0.1, "{name}: CA {ca} px");
        // Vignetting: relative illumination at the source radius.
        let (w, h) = (W as f64, H as f64);
        let hd = (w / 2.).hypot(h / 2.);
        let knots: Vec<f64> = [0.]
            .into_iter()
            .filter(|_| f.knots[0] > 0.)
            .chain(f.knots.iter().map(|k| k * f.crop_factor))
            .collect();
        let vig: Vec<f64> = [1.]
            .into_iter()
            .filter(|_| f.knots[0] > 0.)
            .chain(f.vignetting.iter().map(|v| v / 100.))
            .collect();
        let mut worst = 0f64;
        for i in 0..=100 {
            let p = [i as f64 / 100., i as f64 / 100.];
            let x = p[0] * sample.coordinate_scale[0];
            let y = p[1] * sample.coordinate_scale[1];
            let r2 = x * x + y * y;
            let v = sample.vignette;
            let fitted = 1. + r2 * (v[0] + r2 * (v[1] + r2 * v[2]));
            let rho = (p[0] * w / 2.).hypot(p[1] * h / 2.) / hd;
            worst = worst.max((fitted / spline(&knots, &vig, rho) - 1.).abs());
        }
        eprintln!("{name}: vignetting max relative error {worst:.5}");
        assert!(worst <= 0.002, "{name}: vignetting {worst}");
    }
}

#[test]
fn maker_note_correction_applies_in_every_built_in_mode() {
    let m = metadata(Some(xe2s()), None);
    let reference = resolve(&settings(LensProfileSource::Auto), &m);
    for profile in [
        LensProfileSource::Auto,
        LensProfileSource::None,
        LensProfileSource::Embedded,
        named("Fujifilm XF18-55mm (not installed)"),
    ] {
        let r = resolve(&settings(profile.clone()), &m);
        assert_eq!(r.source(), CorrectionSource::MakerNote, "{profile:?}");
        assert_eq!(r.sample(), reference.sample(), "{profile:?}");
    }
    // The explicit estimate opt-in keeps its own source.
    let r = resolve(&settings(LensProfileSource::AutoCalibrated), &m);
    assert_ne!(r.source(), CorrectionSource::MakerNote);
    // An available profile is the user's explicit choice.
    let profile = Profile {
        maker: "test".into(),
        model: "Supplied".into(),
        camera: None,
        samples: vec![CalibrationSample {
            distortion: BrownConrady {
                k1: -0.05,
                ..Default::default()
            },
            ..Default::default()
        }],
    };
    let context = LensContext {
        profile: Some(&profile),
        ..Default::default()
    };
    let s = settings(named("Supplied"));
    let r = resolve_lens(&rgb(), &s.lens, Some(&m), &context).unwrap();
    assert_eq!(r.source(), CorrectionSource::Database);
    // Without maker-note data nothing changes.
    let r = resolve(&settings(LensProfileSource::Auto), &metadata(None, None));
    assert_eq!(r.source(), CorrectionSource::Manual);
    assert!(r.sample().is_none());
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
fn dng_opcodes_take_precedence_and_are_never_combined() {
    let m = metadata(Some(xe2s()), Some(vignette_opcode()));
    let r = resolve(&settings(LensProfileSource::None), &m);
    assert_eq!(r.source(), CorrectionSource::Embedded);
    assert!(
        r.sample().is_none(),
        "maker-note sample applied on top of opcodes"
    );
}

#[test]
fn built_in_ca_applies_whatever_the_remove_ca_switch() {
    let m = metadata(Some(xe2s()), None);
    for remove in [false, true] {
        let mut s = settings(LensProfileSource::Auto);
        s.lens.remove_chromatic_aberration = remove;
        let r = resolve(&s, &m);
        let plan = r.plan(&s, &m).unwrap().expect("resident plan");
        assert!(plan.ca.is_some(), "remove CA {remove}: built-in CA dropped");
        assert!(plan.vignette.is_some() && plan.map.is_some());
        // A zero CA amount still disables it, as for opcode warps.
        s.lens.chromatic_aberration_scale = 0.;
        let plan = r.plan(&s, &m).unwrap().unwrap();
        assert!(plan.ca.is_none());
    }
}

#[test]
fn notices_name_the_built_in_correction() {
    let m = metadata(Some(xe2s()), None);
    let ctx = LensContext::default();
    assert_eq!(
        lens_notice(&settings(LensProfileSource::Auto).lens, Some(&m), &ctx),
        None
    );
    assert_eq!(
        lens_notice(&settings(named("Missing")).lens, Some(&m), &ctx),
        Some(LensNotice::ProfileUnavailable {
            name: "Missing".into(),
            built_in: true
        })
    );
}

fn strip(m: &RawMetadata) -> RawMetadata {
    let mut m = m.clone();
    m.maker_lens = None;
    m
}

#[test]
fn raw_fixtures_apply_maker_note_corrections_where_present() {
    const TEST: &str = "raw_fixtures_apply_maker_note_corrections_where_present";
    let files = raw_fixtures::all(TEST);
    let mut corrected = 0;
    for path in &files {
        let name = raw_fixtures::name(path);
        let mut source = RawSource::open(path).unwrap();
        let cfa = source.decode_cfa().unwrap();
        let metadata = source.metadata();
        let plane = cfa.pyramid().pixels();
        let ctx = LensContext::default();
        let auto = settings(LensProfileSource::Auto);
        let none = settings(LensProfileSource::None);
        let r_auto = resolve_lens_sensor(plane, &metadata, &auto, &ctx).unwrap();
        let r_none = resolve_lens_sensor(plane, &metadata, &none, &ctx).unwrap();
        assert_eq!(r_auto.source(), r_none.source(), "{name}");
        if metadata.maker_lens.is_none() {
            assert_ne!(r_auto.source(), CorrectionSource::MakerNote, "{name}");
            continue;
        }
        corrected += 1;
        assert_eq!(r_auto.source(), CorrectionSource::MakerNote, "{name}");
        assert_eq!(r_auto.sample(), r_none.sample(), "{name}");
        let src = RenderSource::Cfa {
            image: &cfa,
            metadata: &metadata,
        };
        let stripped = strip(&metadata);
        let off = RenderSource::Cfa {
            image: &cfa,
            metadata: &stripped,
        };
        let a = render_linear_scaled(&auto, &src, 8).unwrap();
        let b = render_linear_scaled(&none, &src, 8).unwrap();
        let c = render_linear_scaled(&none, &off, 8).unwrap();
        assert_eq!(a.planes(), b.planes(), "{name}: None must apply built-in");
        let diff = a.planes()[1]
            .iter()
            .zip(&c.planes()[1])
            .map(|(x, y)| (x - y).abs())
            .fold(0f32, f32::max);
        raw_fixtures::notice(TEST, &format!("{name}: built-in vs none max diff {diff}"));
        assert!(diff > 0.01, "{name}: built-in correction had no effect");
    }
    if files
        .iter()
        .any(|p| p.extension().is_some_and(|e| e.eq_ignore_ascii_case("raf")))
    {
        assert_eq!(
            corrected, 1,
            "the RAF fixture carries a maker-note correction"
        );
    }
}

/// Grey levels of an RGB8 buffer.
fn luma(rgb: &[u8], w: usize, h: usize) -> Vec<f32> {
    (0..w * h)
        .map(|i| {
            0.2126 * rgb[3 * i] as f32
                + 0.7152 * rgb[3 * i + 1] as f32
                + 0.0722 * rgb[3 * i + 2] as f32
        })
        .collect()
}

/// Bilinear resample of a grey image to `w` × `h` (pixel-centre aligned).
fn resample(src: &[f32], sw: usize, sh: usize, w: usize, h: usize) -> Vec<f32> {
    let mut out = vec![0.; w * h];
    for y in 0..h {
        for x in 0..w {
            let sx = ((x as f64 + 0.5) * sw as f64 / w as f64 - 0.5).clamp(0., (sw - 1) as f64);
            let sy = ((y as f64 + 0.5) * sh as f64 / h as f64 - 0.5).clamp(0., (sh - 1) as f64);
            let (x0, y0) = (sx.floor() as usize, sy.floor() as usize);
            let (x1, y1) = ((x0 + 1).min(sw - 1), (y0 + 1).min(sh - 1));
            let (fx, fy) = ((sx - x0 as f64) as f32, (sy - y0 as f64) as f32);
            let at = |x: usize, y: usize| src[y * sw + x];
            out[y * w + x] = (at(x0, y0) * (1. - fx) + at(x1, y0) * fx) * (1. - fy)
                + (at(x0, y1) * (1. - fx) + at(x1, y1) * fx) * fy;
        }
    }
    out
}

/// Gradient magnitude (central differences).
fn gradient(src: &[f32], w: usize, h: usize) -> Vec<f32> {
    let mut out = vec![0.; w * h];
    for y in 1..h - 1 {
        for x in 1..w - 1 {
            let gx = src[y * w + x + 1] - src[y * w + x - 1];
            let gy = src[(y + 1) * w + x] - src[(y - 1) * w + x];
            out[y * w + x] = gx.hypot(gy);
        }
    }
    out
}

/// Per-tile displacement of `moving` relative to `fixed` (best normalized
/// cross-correlation over integer shifts, parabolic sub-pixel refinement):
/// (tile centre, shift) for textured tiles with a clear peak.
fn tile_shifts(fixed: &[f32], moving: &[f32], w: usize, h: usize) -> Vec<([f64; 2], [f64; 2])> {
    const T: usize = 96;
    const S: i64 = 14;
    let mut out = Vec::new();
    for ty in 0..6 {
        for tx in 0..9 {
            let cx = w as f64 * (0.08 + 0.84 * tx as f64 / 8.);
            let cy = h as f64 * (0.1 + 0.8 * ty as f64 / 5.);
            let (x0, y0) = (cx as usize - T / 2, cy as usize - T / 2);
            let patch = |img: &[f32], dx: i64, dy: i64| -> Vec<f32> {
                (0..T * T)
                    .map(|i| {
                        let x = (x0 + i % T) as i64 + dx;
                        let y = (y0 + i / T) as i64 + dy;
                        img[y as usize * w + x as usize]
                    })
                    .collect()
            };
            let ncc = |a: &[f32], b: &[f32]| {
                let n = a.len() as f64;
                let (ma, mb) = (
                    a.iter().map(|&v| v as f64).sum::<f64>() / n,
                    b.iter().map(|&v| v as f64).sum::<f64>() / n,
                );
                let (mut ab, mut aa, mut bb) = (0., 0., 0.);
                for (x, y) in a.iter().zip(b) {
                    let (x, y) = (*x as f64 - ma, *y as f64 - mb);
                    ab += x * y;
                    aa += x * x;
                    bb += y * y;
                }
                (ab / (aa * bb).sqrt().max(1e-12), aa / n)
            };
            let reference = patch(fixed, 0, 0);
            let texture = ncc(&reference, &reference).1;
            let mut scores = vec![vec![0.; (2 * S + 1) as usize]; (2 * S + 1) as usize];
            let mut best = (f64::MIN, 0, 0);
            for dy in -S..=S {
                for dx in -S..=S {
                    let score = ncc(&reference, &patch(moving, dx, dy)).0;
                    scores[(dy + S) as usize][(dx + S) as usize] = score;
                    if score > best.0 {
                        best = (score, dx, dy);
                    }
                }
            }
            let (score, dx, dy) = best;
            if score < 0.6 || texture < 4. || dx.abs() == S || dy.abs() == S {
                continue;
            }
            let at = |dx: i64, dy: i64| scores[(dy + S) as usize][(dx + S) as usize];
            let refine = |m: f64, c: f64, p: f64| {
                let d = m - 2. * c + p;
                if d.abs() < 1e-12 {
                    0.
                } else {
                    0.5 * (m - p) / d
                }
            };
            let sx = dx as f64 + refine(at(dx - 1, dy), score, at(dx + 1, dy));
            let sy = dy as f64 + refine(at(dx, dy - 1), score, at(dx, dy + 1));
            out.push(([cx - w as f64 / 2., cy - h as f64 / 2.], [sx, sy]));
        }
    }
    out
}

/// Residual RMS (pixels) of tile shifts after the best global scale and
/// translation (the JPEG's framing may differ slightly from the raw's crop).
fn similarity_residual(shifts: &[([f64; 2], [f64; 2])]) -> f64 {
    // shift = a·p + t, solved per axis with a shared a.
    let n = shifts.len() as f64;
    let mean = |f: &dyn Fn(&([f64; 2], [f64; 2])) -> f64| shifts.iter().map(f).sum::<f64>() / n;
    let (px, py) = (mean(&|s| s.0[0]), mean(&|s| s.0[1]));
    let (sx, sy) = (mean(&|s| s.1[0]), mean(&|s| s.1[1]));
    let num: f64 = shifts
        .iter()
        .map(|s| (s.0[0] - px) * (s.1[0] - sx) + (s.0[1] - py) * (s.1[1] - sy))
        .sum();
    let den: f64 = shifts
        .iter()
        .map(|s| (s.0[0] - px).powi(2) + (s.0[1] - py).powi(2))
        .sum();
    let a = num / den;
    let r: f64 = shifts
        .iter()
        .map(|s| {
            let ex = s.1[0] - sx - a * (s.0[0] - px);
            let ey = s.1[1] - sy - a * (s.0[1] - py);
            ex * ex + ey * ey
        })
        .sum();
    (r / n).sqrt()
}

/// Sanity check against the camera: the X-E2S writes its JPEG with its
/// built-in distortion correction applied. After the best global scale and
/// translation, the corrected render must line up with the JPEG markedly
/// better than the uncorrected one.
#[test]
fn raf_correction_matches_the_cameras_embedded_jpeg_geometry() {
    const TEST: &str = "raf_correction_matches_the_cameras_embedded_jpeg_geometry";
    let Some(path) = raw_fixtures::with_extension(TEST, "raf") else {
        return;
    };
    let mut source = RawSource::open(&path).unwrap();
    let jpeg = source.embedded_preview().expect("embedded JPEG");
    let cfa = source.decode_cfa().unwrap();
    let metadata = source.metadata();
    let jpeg = image::load_from_memory(&jpeg).unwrap().to_rgb8();
    let (jw, jh) = (jpeg.width() as usize, jpeg.height() as usize);
    let reference = gradient(&luma(jpeg.as_raw(), jw, jh), jw, jh);
    let stripped = strip(&metadata);
    let mut residuals = Vec::new();
    for m in [&metadata, &stripped] {
        let rendered = render_scaled(
            &settings(LensProfileSource::None),
            &RenderSource::Cfa {
                image: &cfa,
                metadata: m,
            },
            2,
        )
        .unwrap();
        let (rw, rh) = (rendered.width() as usize, rendered.height() as usize);
        let grey = resample(&luma(rendered.as_raw(), rw, rh), rw, rh, jw, jh);
        let shifts = tile_shifts(&reference, &gradient(&grey, jw, jh), jw, jh);
        assert!(shifts.len() >= 15, "only {} matched tiles", shifts.len());
        residuals.push((similarity_residual(&shifts), shifts.len()));
    }
    let [(corrected, n1), (uncorrected, n2)] = residuals[..] else {
        unreachable!()
    };
    raw_fixtures::notice(
        TEST,
        &format!(
            "residual vs JPEG after scale+shift: built-in {corrected:.3} px ({n1} tiles), \
             uncorrected {uncorrected:.3} px ({n2} tiles)"
        ),
    );
    assert!(
        corrected < 0.6 * uncorrected && corrected < 1.,
        "built-in {corrected} px vs uncorrected {uncorrected} px"
    );
}
