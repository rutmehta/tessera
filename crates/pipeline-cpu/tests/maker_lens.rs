//! ENG-8: camera built-in corrections stored in maker notes (Fujifilm RAF).
//!
//! Lightroom applies a Fujifilm X-series camera's built-in distortion,
//! vignetting and lateral CA correction whatever the profile setting, like a
//! DNG opcode correction. Since ENG-8c Tessera applies it exactly like one:
//! in the raw prefix (camera RGB, where DNG opcode stages run), so local
//! adjustments, masks, prompts, crop and Upright all work in the corrected,
//! displayed frame. It resolves as the `Embedded` built-in source in every
//! mode that applies built-in corrections (ENG-7b).
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
        let fitted = pipeline_cpu::maker_note_sample(&m).expect("maker-note model");
        let sample = &fitted;
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
    for profile in [
        LensProfileSource::Auto,
        LensProfileSource::None,
        LensProfileSource::Embedded,
        named("Fujifilm XF18-55mm (not installed)"),
    ] {
        let r = resolve(&settings(profile.clone()), &m);
        // ENG-8c: a raw-prefix built-in correction, like DNG opcodes.
        assert_eq!(r.source(), CorrectionSource::Embedded, "{profile:?}");
        assert!(r.sample().is_none(), "{profile:?}: no post-local sample");
    }
    // The explicit estimate opt-in keeps its own source.
    let r = resolve(&settings(LensProfileSource::AutoCalibrated), &m);
    assert_ne!(r.source(), CorrectionSource::Embedded);
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

/// A small raw (64x48, Bayer) for render comparisons.
fn small(
    lens: Option<FujifilmLens>,
    opcodes: Option<Vec<u8>>,
) -> (raw_decode::CfaImage, RawMetadata) {
    let mut m = metadata(lens, opcodes);
    m.width = 64;
    m.height = 48;
    m.default_crop = [0, 0, 64, 48];
    let cfa = raw_decode::CfaImage::from_linear(
        64,
        48,
        (0..64 * 48)
            .map(|i| 0.05 + 0.4 * (((i % 64) / 6 + (i / 64) / 6) % 2) as f32)
            .collect(),
    )
    .unwrap();
    (cfa, m)
}

#[test]
fn dng_opcodes_take_precedence_and_are_never_combined() {
    let m = metadata(Some(xe2s()), Some(vignette_opcode()));
    let r = resolve(&settings(LensProfileSource::None), &m);
    assert_eq!(r.source(), CorrectionSource::Embedded);
    assert!(r.sample().is_none());
    // The maker-note correction is not applied on top of the opcodes.
    let render = |lens, opcodes| {
        let (cfa, m) = small(lens, opcodes);
        render_linear_scaled(
            &settings(LensProfileSource::None),
            &RenderSource::Cfa {
                image: &cfa,
                metadata: &m,
            },
            1,
        )
        .unwrap()
    };
    let both = render(Some(barrel()), Some(vignette_opcode()));
    let opcodes = render(None, Some(vignette_opcode()));
    let maker = render(Some(barrel()), None);
    assert_eq!(both.planes(), opcodes.planes());
    assert_ne!(maker.planes(), opcodes.planes());
}

#[test]
fn built_in_ca_applies_whatever_the_remove_ca_switch() {
    let m = metadata(Some(xe2s()), None);
    for remove in [false, true] {
        let mut s = settings(LensProfileSource::Auto);
        s.lens.remove_chromatic_aberration = remove;
        let r = resolve(&s, &m);
        // ENG-8c: the whole correction is one raw-prefix resample, which the
        // resident plan carries as its sensor-frame stage.
        let plan = r.plan(&s, &m).unwrap().expect("resident plan");
        let maker = plan.ca.as_ref().and_then(|c| c.maker.as_ref());
        let maker = maker.expect("built-in prefix stage");
        assert_eq!(
            maker.ca_amount, 1.,
            "remove CA {remove}: built-in CA dropped"
        );
        assert_eq!((maker.distortion_amount, maker.vignette_amount), (1., 1.));
        // Nothing of it is left for the post-local stages.
        assert!(plan.vignette.is_none() && plan.map.is_none());
        // A zero CA amount still disables its CA, as for opcode warps.
        s.lens.chromatic_aberration_scale = 0.;
        let plan = r.plan(&s, &m).unwrap().unwrap();
        assert_eq!(plan.ca.unwrap().maker.unwrap().ca_amount, 0.);
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
            continue;
        }
        corrected += 1;
        assert_eq!(r_auto.source(), CorrectionSource::Embedded, "{name}");
        assert!(r_auto.sample().is_none(), "{name}");
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
/// A tile centre (relative to the frame centre) and its measured shift.
type TileShift = ([f64; 2], [f64; 2]);

fn tile_shifts(fixed: &[f32], moving: &[f32], w: usize, h: usize) -> Vec<TileShift> {
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
/// translation (the JPEG's framing may differ slightly from the raw's crop),
/// and that relative scale between the two framings (0.0014 = 0.14 %).
fn similarity_residual(shifts: &[TileShift]) -> (f64, f64) {
    // shift = a·p + t, solved per axis with a shared a.
    let n = shifts.len() as f64;
    let mean = |f: &dyn Fn(&TileShift) -> f64| shifts.iter().map(f).sum::<f64>() / n;
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
    ((r / n).sqrt(), a)
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
        let (residual, scale) = similarity_residual(&shifts);
        residuals.push((residual, scale, shifts.len()));
    }
    let [(corrected, s1, n1), (uncorrected, s2, n2)] = residuals[..] else {
        unreachable!()
    };
    // Units: pixels of the camera's 1920-wide JPEG (x2.55 at full size).
    raw_fixtures::notice(
        TEST,
        &format!(
            "residual vs JPEG after scale+shift (JPEG px): built-in {corrected:.3} ({n1} tiles, \
             scale {:+.2} %), uncorrected {uncorrected:.3} ({n2} tiles, scale {:+.2} %)",
            100. * s1,
            100. * s2
        ),
    );
    assert!(
        corrected < 0.6 * uncorrected && corrected < 1.,
        "built-in {corrected} px vs uncorrected {uncorrected} px"
    );
}

/// ENG-8b: Native AI masks are segmented in the frame local adjustments are
/// applied in. Lens warps that run after the locals (maker-note, profile,
/// estimate) are left out of the segmentation input; a DNG opcode warp runs
/// in the raw prefix, before the locals, and stays.
#[test]
fn mask_segmentation_settings_leave_out_post_local_warps_only() {
    // ENG-8c: the maker-note warp runs in the raw prefix, before the locals.
    let s = pipeline_cpu::mask_segmentation_settings(Some(&metadata(Some(xe2s()), None)));
    assert_eq!(s.lens.distortion_scale, 100.);
    let s = pipeline_cpu::mask_segmentation_settings(Some(&metadata(None, None)));
    assert_eq!(s.lens.distortion_scale, 0.);
    let s =
        pipeline_cpu::mask_segmentation_settings(Some(&metadata(None, Some(vignette_opcode()))));
    assert_eq!(s.lens.distortion_scale, 100.);
    // Everything else is the as-shot default (vignetting and CA do not move
    // content and stay).
    let mut expected = DevelopSettings::default();
    expected.lens.distortion_scale = 0.;
    assert_eq!(pipeline_cpu::mask_segmentation_settings(None), expected);
}

/// Luma of planar linear RGB.
fn plane_luma(image: &pipeline_cpu::Image) -> Vec<f32> {
    let p = image.planes();
    (0..p[0].len())
        .map(|i| 0.2627 * p[0][i] + 0.678 * p[1][i] + 0.0593 * p[2][i])
        .collect()
}

/// Box blur (radius `r`, clamped edges): thresholds of smooth content have
/// smooth boundaries, so resampling the raster does not dominate the IoU.
fn box_blur(v: &[f32], w: usize, h: usize, r: usize) -> Vec<f32> {
    let pass = |src: &[f32], horizontal: bool| -> Vec<f32> {
        let mut out = vec![0.; src.len()];
        for y in 0..h {
            for x in 0..w {
                let mut sum = 0.;
                for d in 0..=2 * r {
                    let (sx, sy) = if horizontal {
                        ((x + d).saturating_sub(r).min(w - 1), y)
                    } else {
                        (x, (y + d).saturating_sub(r).min(h - 1))
                    };
                    sum += src[sy * w + sx];
                }
                out[y * w + x] = sum / (2 * r + 1) as f32;
            }
        }
        out
    };
    pass(&pass(v, true), false)
}

fn median(v: &[f32]) -> f32 {
    let mut s = v.to_vec();
    s.sort_by(f32::total_cmp);
    s[s.len() / 2]
}

/// Intersection over union of two masks, ignoring a 3 % border.
pub fn interior_iou(a: &[bool], b: &[bool], w: usize, h: usize) -> f64 {
    let (mx, my) = (w * 3 / 100, h * 3 / 100);
    let (mut inter, mut union) = (0u64, 0u64);
    for y in my..h - my {
        for x in mx..w - mx {
            let (p, q) = (a[y * w + x], b[y * w + x]);
            inter += u64::from(p && q);
            union += u64::from(p || q);
        }
    }
    inter as f64 / union.max(1) as f64
}

/// IoU between the pixels a -3 EV local adjustment darkens in the final
/// render and the content it was segmented from: a threshold "segmentation"
/// of the `segmentation` render (smoothed luma above its median), applied at
/// the reference pipeline's local-adjustment barrier.
fn mask_alignment(
    cfa: &raw_decode::CfaImage,
    metadata: &RawMetadata,
    segmentation: &DevelopSettings,
) -> f64 {
    let src = RenderSource::Cfa {
        image: cfa,
        metadata,
    };
    let scale = 4;
    let mut edited = DevelopSettings::default();
    edited
        .locals
        .adjustments
        .push(engine_api::recipe::LocalAdjustment {
            components: vec![engine_api::recipe::MaskComponent::new(
                engine_api::recipe::MaskKind::Subject { model: None },
            )],
            params: engine_api::recipe::LocalParams {
                exposure: -3.,
                ..Default::default()
            },
            ..Default::default()
        });
    let plain = render_linear_scaled(&DevelopSettings::default(), &src, scale).unwrap();
    let (w, h) = (plain.width() as usize, plain.height() as usize);
    let target_luma = plane_luma(&plain);
    let input = render_linear_scaled(segmentation, &src, scale).unwrap();
    assert_eq!((input.width() as usize, input.height() as usize), (w, h));
    let luma = box_blur(&plane_luma(&input), w, h, 6);
    let t = median(&luma);
    let alpha: Vec<f32> = luma.iter().map(|&v| f32::from(u8::from(v > t))).collect();
    let locals = |image: &pipeline_cpu::Image,
                  groups: &[engine_api::recipe::LocalAdjustment]|
     -> engine_api::EngineResult<pipeline_cpu::Image> {
        // The barrier runs at full resolution: resample the raster to it
        // (pixel centres, bilinear), as the mask cache does.
        let (iw, ih) = (image.width() as usize, image.height() as usize);
        let full = resample(&alpha, w, h, iw, ih);
        let mut out = image.clone();
        for g in groups {
            let adjusted = pipeline_cpu::adjust_local(&out, &g.params, g.amount)?;
            out = pipeline_cpu::blend_local(&out, &adjusted, &full)?;
        }
        Ok(out)
    };
    let masked = pipeline_cpu::render_linear_scaled_with_local_hook(
        &edited,
        &src,
        scale,
        &LensContext::default(),
        None,
        None,
        &locals,
    )
    .unwrap();
    let darkened: Vec<bool> = plane_luma(&masked)
        .iter()
        .zip(&target_luma)
        .map(|(a, b)| *a < 0.5 * b)
        .collect();
    let target: Vec<bool> = box_blur(&target_luma, w, h, 6)
        .iter()
        .map(|&v| v > t)
        .collect();
    interior_iou(&darkened, &target, w, h)
}

/// The reviewer's alignment check (REV-ENG-8 B2) on the reference pipeline:
/// with the correction on, a mask segmented from `mask_segmentation_settings`
/// lands on its content as well as on the same raw without the correction
/// (main's behaviour, the resampling floor); segmenting the default (warped)
/// render, as before ENG-8b, misaligns it.
#[test]
fn raf_ai_mask_from_segmentation_settings_lands_on_its_content() {
    const TEST: &str = "raf_ai_mask_from_segmentation_settings_lands_on_its_content";
    let Some(path) = raw_fixtures::with_extension(TEST, "raf") else {
        return;
    };
    let mut source = RawSource::open(&path).unwrap();
    let cfa = source.decode_cfa().unwrap();
    let metadata = source.metadata();
    let floor = mask_alignment(&cfa, &strip(&metadata), &DevelopSettings::default());
    let aligned = mask_alignment(
        &cfa,
        &metadata,
        &pipeline_cpu::mask_segmentation_settings(Some(&metadata)),
    );
    let warped = mask_alignment(&cfa, &metadata, &DevelopSettings::default());
    raw_fixtures::notice(
        TEST,
        &format!(
            "IoU: no correction {floor:.4}, corrected + segmentation settings {aligned:.4}, \
             corrected + default render {warped:.4}"
        ),
    );
    // ENG-8c: the built-in warp runs in the raw prefix, so the as-shot
    // default render already is the frame the masks are applied in; the
    // segmentation settings equal it for this raw. (ENG-8b, with the warp
    // after the locals, measured floor 0.9979, aligned 0.9953, default
    // render 0.9671.)
    assert_eq!(
        pipeline_cpu::mask_segmentation_settings(Some(&metadata)),
        DevelopSettings::default()
    );
    assert!(
        aligned >= floor - 0.002,
        "aligned {aligned} vs floor {floor}"
    );
    assert_eq!(aligned, warped);
}

/// Centroid (pixels) of the pixels a local adjustment brightened by more than
/// 25 % of the strongest change.
fn effect_centroid(plain: &pipeline_cpu::Image, edited: &pipeline_cpu::Image) -> [f64; 2] {
    let (w, a, b) = (
        plain.width() as usize,
        plane_luma(plain),
        plane_luma(edited),
    );
    let delta: Vec<f32> = a.iter().zip(&b).map(|(a, b)| b - a).collect();
    let peak = delta.iter().copied().fold(0f32, f32::max);
    let (mut sx, mut sy, mut n) = (0f64, 0f64, 0f64);
    for (i, d) in delta.iter().enumerate() {
        if *d > 0.25 * peak {
            let wgt = f64::from(*d);
            sx += wgt * ((i % w) as f64 + 0.5);
            sy += wgt * ((i / w) as f64 + 0.5);
            n += wgt;
        }
    }
    [sx / n, sy / n]
}

/// REV2-ENG-8 NB1: hand-drawn masks are defined in the frame the locals run
/// in; the app maps clicks through crop and orientation only. With the
/// built-in warp in the raw prefix that frame is the displayed one: a radial
/// mask and a brush dab drawn at (0.85, 0.80) land within 1 px of it at
/// level 2 (1/4 scale). With the ENG-8b post-local warp the radial landed
/// 6.8 px (27 px at full size) away.
#[test]
fn raf_hand_drawn_masks_land_where_drawn() {
    const TEST: &str = "raf_hand_drawn_masks_land_where_drawn";
    let Some(path) = raw_fixtures::with_extension(TEST, "raf") else {
        return;
    };
    let mut source = RawSource::open(&path).unwrap();
    let cfa = source.decode_cfa().unwrap();
    let metadata = source.metadata();
    let src = RenderSource::Cfa {
        image: &cfa,
        metadata: &metadata,
    };
    let scale = 4;
    let plain = render_linear_scaled(&DevelopSettings::default(), &src, scale).unwrap();
    let (w, h) = (f64::from(plain.width()), f64::from(plain.height()));
    let at = [0.85f32, 0.80];
    for (name, kind) in [
        (
            "radial",
            engine_api::recipe::MaskKind::Radial {
                center: at,
                radii: [0.03, 0.045],
                angle: 0.,
                feather: 0.,
            },
        ),
        (
            "brush",
            engine_api::recipe::MaskKind::Brush {
                strokes: vec![engine_api::recipe::mask::BrushStroke {
                    points: vec![[at[0], at[1], 1.]],
                    radius: 0.03,
                    feather: 0.,
                    flow: 100.,
                    erase: false,
                }],
            },
        ),
    ] {
        let mut edited = DevelopSettings::default();
        edited
            .locals
            .adjustments
            .push(engine_api::recipe::LocalAdjustment {
                components: vec![engine_api::recipe::MaskComponent::new(kind)],
                params: engine_api::recipe::LocalParams {
                    exposure: 2.,
                    ..Default::default()
                },
                ..Default::default()
            });
        let masked = render_linear_scaled(&edited, &src, scale).unwrap();
        let c = effect_centroid(&plain, &masked);
        let drawn = [f64::from(at[0]) * w, f64::from(at[1]) * h];
        let off = (c[0] - drawn[0]).hypot(c[1] - drawn[1]);
        raw_fixtures::notice(
            TEST,
            &format!("{name}: centroid {c:?} drawn {drawn:?}, off {off:.2} px"),
        );
        assert!(off <= 1., "{name}: lands {off} px from where it was drawn");
    }
}

/// ENG-8c: with default settings the RAF's geometry stage is the identity:
/// the pre-geometry frame (where locals, masks, prompts and retouch run) is
/// the displayed frame.
#[test]
fn raf_pre_geometry_frame_is_the_displayed_frame() {
    const TEST: &str = "raf_pre_geometry_frame_is_the_displayed_frame";
    let Some(path) = raw_fixtures::with_extension(TEST, "raf") else {
        return;
    };
    let mut source = RawSource::open(&path).unwrap();
    let cfa = source.decode_cfa().unwrap();
    let metadata = source.metadata();
    let src = RenderSource::Cfa {
        image: &cfa,
        metadata: &metadata,
    };
    let s = DevelopSettings::default();
    let r = resolve_lens_sensor(
        cfa.pyramid().pixels(),
        &metadata,
        &s,
        &LensContext::default(),
    )
    .unwrap();
    let plan = r.plan(&s, &metadata).unwrap().expect("resident plan");
    assert!(plan.map.is_none(), "no post-local warp");
    let before = pipeline_cpu::render_linear_before_geometry(&s, &src, None).unwrap();
    let after = render_linear_scaled(&s, &src, 1).unwrap();
    assert_eq!(before.planes(), after.planes());
}

/// REV-ENG-8 (7): a checksum pin of the corrected default RAF render (the
/// golden-scale render of `tests/golden.rs`, lens Auto), so later changes to
/// the maker-note correction show up like a golden change.
#[test]
fn raf_corrected_default_render_checksum() {
    const TEST: &str = "raf_corrected_default_render_checksum";
    // ENG-8d re-pin: the raw-prefix resample is Lanczos-3, not bilinear
    // (REV3-ENG-8 NB2). Earlier: ENG-8c 679f717b…f3cc (prefix, bilinear),
    // ENG-8b f5f82c27…b729 (post-local Lanczos geometry map).
    const PIN: &str = "b66ad609b736d817acd529ae8019722b4f21057252cdc30b66ed8e5cacbc5e8c";
    let Some(path) = raw_fixtures::with_extension(TEST, "raf") else {
        return;
    };
    let mut source = RawSource::open(&path).unwrap();
    let cfa = source.decode_cfa().unwrap();
    let metadata = source.metadata();
    let rendered = render_scaled(
        &DevelopSettings::default(),
        &RenderSource::Cfa {
            image: &cfa,
            metadata: &metadata,
        },
        8,
    )
    .unwrap();
    let digest = blake3::hash(rendered.as_raw()).to_hex().to_string();
    raw_fixtures::notice(
        TEST,
        &format!("{}x{} {digest}", rendered.width(), rendered.height()),
    );
    assert_eq!(digest, PIN, "corrected RAF render changed");
}

/// Mean squared Laplacian and gradient of the square-root-encoded green
/// plane, per radial ring of the half-diagonal (REV3-ENG-8's method).
fn ring_detail(image: &pipeline_cpu::Image, rings: &[f64]) -> Vec<(f64, f64)> {
    let (w, h) = (image.width() as usize, image.height() as usize);
    let g: Vec<f64> = image.planes()[1]
        .iter()
        .map(|&v| f64::from(v.max(0.)).sqrt())
        .collect();
    let hd = (w as f64 / 2.).hypot(h as f64 / 2.);
    let mut acc = vec![(0f64, 0f64, 0f64); rings.len() - 1];
    for y in 1..h - 1 {
        for x in 1..w - 1 {
            let i = y * w + x;
            let lap = g[i - 1] + g[i + 1] + g[i - w] + g[i + w] - 4. * g[i];
            let gx = (g[i + 1] - g[i - 1]) / 2.;
            let gy = (g[i + w] - g[i - w]) / 2.;
            let r = (x as f64 + 0.5 - w as f64 / 2.).hypot(y as f64 + 0.5 - h as f64 / 2.) / hd;
            if let Some(k) = rings.windows(2).position(|b| r >= b[0] && r < b[1]) {
                acc[k].0 += lap * lap;
                acc[k].1 += gx * gx + gy * gy;
                acc[k].2 += 1.;
            }
        }
    }
    acc.iter().map(|(l, g, n)| (l / n, g / n)).collect()
}

/// REV3-ENG-8 NB2: the raw-prefix resample must not soften the picture. Per
/// ring, the corrected render keeps at least 60 % of the fine-detail energy
/// (Laplacian²) of the same render without the resample (distortion and CA
/// amounts 0: the stage samples integer positions). Bilinear kept ~31 %
/// (ENG-8c); the ENG-8b Lanczos geometry map ~67 %.
#[test]
fn raf_prefix_resample_keeps_fine_detail() {
    const TEST: &str = "raf_prefix_resample_keeps_fine_detail";
    let Some(path) = raw_fixtures::with_extension(TEST, "raf") else {
        return;
    };
    let mut source = RawSource::open(&path).unwrap();
    let cfa = source.decode_cfa().unwrap();
    let metadata = source.metadata();
    let src = RenderSource::Cfa {
        image: &cfa,
        metadata: &metadata,
    };
    let corrected = render_linear_scaled(&DevelopSettings::default(), &src, 1).unwrap();
    let mut unresampled = DevelopSettings::default();
    unresampled.lens.distortion_scale = 0.;
    unresampled.lens.chromatic_aberration_scale = 0.;
    let reference = render_linear_scaled(&unresampled, &src, 1).unwrap();
    let rings = [0., 0.15, 0.35, 0.55, 0.75, 1.0];
    let (a, b) = (
        ring_detail(&corrected, &rings),
        ring_detail(&reference, &rings),
    );
    let mut worst = f64::INFINITY;
    for (k, ((la, ga), (lb, gb))) in a.iter().zip(&b).enumerate() {
        let (lr, gr) = (la / lb, ga / gb);
        raw_fixtures::notice(
            TEST,
            &format!(
                "ring {:.2}-{:.2}: Laplacian² ratio {lr:.3}, gradient² ratio {gr:.3}",
                rings[k],
                rings[k + 1]
            ),
        );
        worst = worst.min(lr);
    }
    assert!(
        worst >= 0.6,
        "the prefix resample keeps only {worst:.3} of fine detail"
    );
}
