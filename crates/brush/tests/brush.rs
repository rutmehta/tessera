//! Brush engine contracts (WP M5-05).

use std::sync::Arc;

use brush::abr::{self, AbrTip};
use brush::gpu::GpuDabRenderer;
use brush::heal::patch;
use brush::{
    AngleControl, Brush, CloneSource, Control, DualBrush, Dynamics, InputPoint, Jitter, PaintMode,
    Planner, SampledTip, Smoothing, Stroke, Symmetry, Tip,
};
use compositor::blend::blend_channel;
use compositor::{BlendMode, Depth, Raster, Rect};
use engine_api::tile::Extent;

fn line(planner: &mut Planner, pts: &[(f32, f32)]) -> Vec<brush::Dab> {
    let mut out = Vec::new();
    for &(x, y) in pts {
        out.extend(planner.push(InputPoint::at(x, y)));
    }
    out.extend(planner.finish());
    out
}

#[test]
fn dab_spacing_is_arc_length_and_sampling_independent() {
    let b = Brush {
        size: 20.0,
        spacing: 0.25,
        ..Brush::default()
    };
    let a = line(&mut Planner::new(&b, 1), &[(0.0, 50.0), (100.0, 50.0)]);
    assert_eq!(a.len(), 21);
    for (i, d) in a.iter().enumerate() {
        assert!((d.x - 5.0 * i as f32).abs() < 1e-3, "dab {i} at {}", d.x);
        assert!((d.y - 50.0).abs() < 1e-6);
    }
    // Irregular sampling of the same path places the same dabs.
    let b2 = line(
        &mut Planner::new(&b, 1),
        &[
            (0.0, 50.0),
            (3.0, 50.0),
            (7.2, 50.0),
            (7.3, 50.0),
            (61.0, 50.0),
            (100.0, 50.0),
        ],
    );
    assert_eq!(b2.len(), a.len());
    for (p, q) in a.iter().zip(&b2) {
        assert!((p.x - q.x).abs() < 1e-3);
    }
    // Around a corner the arc length (not the chord) sets the spacing.
    let c = line(
        &mut Planner::new(&b, 1),
        &[(0.0, 0.0), (7.0, 0.0), (7.0, 13.0)],
    );
    let xs: Vec<(f32, f32)> = c.iter().map(|d| (d.x, d.y)).collect();
    assert_eq!(xs.len(), 5, "{xs:?}");
    assert!(
        (xs[1].0 - 5.0).abs() < 1e-4 && (xs[2].1 - 3.0).abs() < 1e-4,
        "{xs:?}"
    );
    // Pressure-controlled size shrinks the step: half pressure, half step.
    let mut bp = b.clone();
    bp.dynamics.size.control = Control::Pressure;
    let mut p = Planner::new(&bp, 1);
    let mut n = p.push(InputPoint::at(0.0, 0.0).pressure(0.5)).len();
    n += p.push(InputPoint::at(100.0, 0.0).pressure(0.5)).len();
    assert_eq!(n, 41);
    // Smoothing (pulled string) lags, then catch-up reaches the end.
    let mut bs = b.clone();
    bs.smoothing = Smoothing {
        string_length: 30.0,
        catch_up: true,
    };
    let mut p = Planner::new(&bs, 1);
    let mut d = p.push(InputPoint::at(0.0, 0.0));
    d.extend(p.push(InputPoint::at(100.0, 0.0)));
    assert!((d.last().unwrap().x - 70.0).abs() < 1e-3);
    d.extend(p.finish());
    assert!((d.last().unwrap().x - 100.0).abs() < 1e-3);
    assert_eq!(d.len(), 21);
}

fn jittery() -> Brush {
    Brush {
        size: 30.0,
        spacing: 0.3,
        dynamics: Dynamics {
            size: Jitter {
                jitter: 0.5,
                control: Control::Off,
                minimum: 0.0,
            },
            angle_jitter: 1.0,
            angle_control: AngleControl::Direction,
            roundness: Jitter {
                jitter: 0.6,
                control: Control::Off,
                minimum: 0.2,
            },
            flow: Jitter {
                jitter: 0.4,
                ..Jitter::default()
            },
            opacity: Jitter {
                jitter: 0.3,
                ..Jitter::default()
            },
            scatter: 1.5,
            scatter_both_axes: true,
            count: 3,
            count_jitter: 0.5,
            flip_x_jitter: true,
            flip_y_jitter: true,
        },
        dual: Some(DualBrush {
            tip: Tip::round(0.5),
            size: 8.0,
            scatter: 0.4,
            count: 3,
        }),
        ..Brush::default()
    }
}

#[test]
fn dynamics_are_deterministic_for_a_seed() {
    let b = jittery();
    let pts = [(10.0, 10.0), (80.0, 40.0), (150.0, 20.0)];
    let a = line(&mut Planner::new(&b, 42), &pts);
    let a2 = line(&mut Planner::new(&b, 42), &pts);
    let c = line(&mut Planner::new(&b, 43), &pts);
    assert_eq!(a, a2);
    assert_ne!(a, c);
    assert!(a.len() > 20);
    for d in &a {
        assert!(
            d.size >= 15.0 - 1e-3 && d.size <= 30.0 + 1e-3,
            "size {}",
            d.size
        );
        assert!(d.roundness >= 0.2 - 1e-6 && d.roundness <= 1.0);
        assert!(d.flow >= 0.6 - 1e-6 && d.flow <= 1.0);
        assert!(d.opacity >= 0.7 - 1e-6 && d.opacity <= 1.0);
        assert_eq!(d.dual.len(), 3);
    }
    // Scatter spreads dabs off the path but within 1.5 × size on each axis.
    let off = a.iter().map(|d| (d.y - 30.0).abs()).fold(0.0f32, f32::max);
    assert!(off > 5.0 && off < 20.0 + 1.5 * 30.0 * 2.0);
    // Rendering with a seed is reproducible too.
    let base = Raster::new(Extent::new(200, 100), 4, Depth::F32, 0.0);
    let render = |seed| {
        let mut s = Stroke::new(b.clone(), &base, seed).unwrap();
        let mut t = base.clone();
        for &(x, y) in &pts {
            if let Some(r) = s.add_point(InputPoint::at(x, y)).unwrap() {
                s.apply(&mut t, r, 1).unwrap();
            }
        }
        (0..100u32)
            .flat_map(|y| (0..200u32).map(move |x| (x, y)))
            .map(|(x, y)| t.pixel(x, y)[3])
            .collect::<Vec<_>>()
    };
    assert_eq!(render(7), render(7));
    assert_ne!(render(7), render(8));
}

fn synthetic_tip(name: &str, w: u32, h: u32) -> SampledTip {
    let data = (0..w * h)
        .map(|i| {
            let (x, y) = ((i % w) as f32, (i / w) as f32);
            let d = ((x - w as f32 / 2.0).powi(2) + (y - h as f32 / 2.0).powi(2)).sqrt();
            let v = (1.0 - d / (w.max(h) as f32 / 2.0)).clamp(0.0, 1.0);
            // Runs of equal values plus a noisy band exercise PackBits.
            if (y as u32).is_multiple_of(5) {
                ((x as u32 * 37) % 256) as f32 / 255.0
            } else {
                (v * 8.0).round() / 8.0
            }
        })
        .collect();
    SampledTip::new(name, w, h, data).unwrap()
}

#[test]
fn abr_round_trip_on_synthetic_files() {
    let tips = vec![
        synthetic_tip("$a1b2c3d4-0000-4000-8000-000000000001", 17, 13),
        synthetic_tip("round-ish", 64, 64),
        synthetic_tip("x", 1, 3),
    ];
    for (version, sub, rle) in [(6, 1, false), (6, 2, true), (10, 2, true), (10, 2, false)] {
        let bytes = abr::write_v6(&tips, version, sub, rle).unwrap();
        let f = abr::parse(&bytes).unwrap();
        assert_eq!((f.version, f.subversion), (version, sub));
        assert!(f.warnings.is_empty(), "{:?}", f.warnings);
        assert_eq!(f.brushes.len(), tips.len());
        for (b, t) in f.brushes.iter().zip(&tips) {
            assert_eq!(b.name, t.name);
            let AbrTip::Sampled(s) = &b.tip else {
                panic!("sampled expected")
            };
            assert_eq!((s.width, s.height), (t.width, t.height));
            for (p, q) in s.data.iter().zip(&t.data) {
                assert!((p - q).abs() <= 0.5 / 255.0 + 1e-6);
            }
            let (tip, diameter) = b.to_tip();
            assert_eq!(diameter, t.width.max(t.height) as f32);
            assert!(matches!(tip.shape, brush::TipShape::Sampled(_)));
        }
    }
    // Permissive: a corrupt record is skipped, its neighbours still load, and
    // trailing garbage is ignored.
    let mut bytes = abr::write_v6(&tips, 6, 2, true).unwrap();
    // First record: size(4) + name len(1) + name(37) + 264 skip + bounds.
    let bounds_at = 4 + 8 + 4 + 4 + 1 + tips[0].name.len() + 264;
    bytes[bounds_at + 8..bounds_at + 12].copy_from_slice(&(-5i32).to_be_bytes());
    bytes.extend(b"garbage!");
    let f = abr::parse(&bytes).unwrap();
    assert_eq!(f.brushes.len(), 2);
    assert_eq!(f.warnings.len(), 1, "{:?}", f.warnings);
    assert!(abr::parse(&[0, 7, 0, 0]).is_err());
}

#[test]
fn abr_v2_computed_and_sampled() {
    let mut f = vec![0, 2, 0, 2];
    // Computed: misc, spacing 25, diameter 40, roundness 50, angle -30, hardness 80.
    let mut rec = vec![0u8; 4];
    for v in [25u16, 40, 50, (-30i16) as u16, 80] {
        rec.extend(v.to_be_bytes());
    }
    f.extend(1u16.to_be_bytes());
    f.extend((rec.len() as u32).to_be_bytes());
    f.extend(rec);
    // Sampled 3×2 raw with a UTF-16 name.
    let mut rec = vec![0u8; 4];
    rec.extend(10u16.to_be_bytes());
    let name: Vec<u16> = "Tip\0".encode_utf16().collect();
    rec.extend((name.len() as u32).to_be_bytes());
    for u in name {
        rec.extend(u.to_be_bytes());
    }
    rec.push(1);
    rec.extend([0u8; 8]);
    for v in [0i32, 0, 2, 3] {
        rec.extend(v.to_be_bytes());
    }
    rec.extend(8u16.to_be_bytes());
    rec.push(0);
    rec.extend([0, 51, 102, 153, 204, 255]);
    f.extend(2u16.to_be_bytes());
    f.extend((rec.len() as u32).to_be_bytes());
    f.extend(rec);
    let a = abr::parse(&f).unwrap();
    assert_eq!(a.brushes.len(), 2, "{:?}", a.warnings);
    assert_eq!(a.brushes[0].spacing, Some(0.25));
    let AbrTip::Computed {
        diameter,
        hardness,
        roundness,
        angle,
    } = a.brushes[0].tip
    else {
        panic!()
    };
    assert_eq!(
        (diameter, hardness, roundness, angle),
        (40.0, 0.8, 0.5, -30.0)
    );
    assert_eq!(a.brushes[1].name, "Tip");
    let AbrTip::Sampled(s) = &a.brushes[1].tip else {
        panic!()
    };
    assert_eq!((s.width, s.height), (3, 2));
    assert!((s.data[5] - 1.0).abs() < 1e-6 && (s.data[1] - 0.2).abs() < 1e-6);
}

/// Independent scalar model of a round-tip stroke (straight polyline,
/// arc-length spacing, smoothstep tip, alpha-darken accumulation, W3C
/// separable blending).
fn reference(
    base: &dyn Fn(u32, u32) -> [f32; 4],
    w: u32,
    h: u32,
    path: &[(f32, f32)],
    b: &Brush,
    hardness: f32,
) -> Vec<[f32; 4]> {
    let step = b.spacing * b.size;
    let mut dabs = vec![path[0]];
    let mut acc = 0.0f32;
    for s in path.windows(2) {
        let (a, c) = (s[0], s[1]);
        let len = (c.0 - a.0).hypot(c.1 - a.1);
        let mut t = step - acc;
        while t <= len + 1e-4 {
            dabs.push((a.0 + (c.0 - a.0) * t / len, a.1 + (c.1 - a.1) * t / len));
            t += step;
        }
        acc = len - (t - step);
    }
    let r = b.size / 2.0;
    let fw = (1.0 - hardness) * r + 1.0;
    let mut out = Vec::new();
    for y in 0..h {
        for x in 0..w {
            let mut m = 0.0f32;
            for &(dx, dy) in &dabs {
                let d = (x as f32 + 0.5 - dx).hypot(y as f32 + 0.5 - dy);
                let t = ((r + 0.5 - d) / fw).clamp(0.0, 1.0);
                let c = t * t * (3.0 - 2.0 * t);
                if c > 0.0 && m < b.opacity {
                    m += (b.opacity - m) * (b.flow * c).min(1.0);
                }
            }
            let p = base(x, y);
            if m <= 0.0 {
                out.push(p);
                continue;
            }
            let ab = p[3];
            let ao = m + ab * (1.0 - m);
            let mut q = [0.0; 4];
            for i in 0..3 {
                let s = b.color[i];
                q[i] = (m * (1.0 - ab) * s
                    + m * ab * blend_channel(b.blend, p[i], s)
                    + (1.0 - m) * ab * p[i])
                    / ao;
            }
            q[3] = ao;
            out.push(q);
        }
    }
    out
}

fn gradient_base(depth: Depth, w: u32, h: u32) -> (Raster, impl Fn(u32, u32) -> [f32; 4]) {
    let f = move |x: u32, y: u32| {
        let a = if x < w / 3 {
            0.0
        } else if y < h / 2 {
            0.6
        } else {
            1.0
        };
        [x as f32 / w as f32, y as f32 / h as f32, 0.5, a]
    };
    let mut r = Raster::new(Extent::new(w, h), 4, depth, 0.0);
    r.edit_region(Rect::new(0, 0, i64::from(w), i64::from(h)), 1, |x, y, p| {
        *p = f(x, y)
    })
    .unwrap();
    (r, f)
}

#[test]
fn stroke_on_raster_matches_scalar_reference() {
    let (w, h) = (300u32, 280u32);
    for (depth, tol) in [(Depth::F32, 2e-5f32), (Depth::U8, 1.01 / 255.0)] {
        let (base, f) = gradient_base(depth, w, h);
        // Reference samples the quantized base like the engine does.
        let fq = |x: u32, y: u32| {
            let _ = &f;
            base.pixel(x, y)
        };
        for blend in [
            BlendMode::Normal,
            BlendMode::Multiply,
            BlendMode::Screen,
            BlendMode::Overlay,
        ] {
            let hardness = 0.6;
            let b = Brush {
                tip: Tip::round(hardness),
                size: 24.0,
                spacing: 0.25,
                flow: 0.4,
                opacity: 0.8,
                color: [0.9, 0.2, 0.1],
                blend,
                ..Brush::default()
            };
            let path = [(20.0, 30.0), (140.0, 60.0), (150.0, 250.0), (285.0, 262.0)];
            let mut s = Stroke::new(b.clone(), &base, 0).unwrap();
            let mut t = base.clone();
            let mut rev = 2;
            for &(x, y) in &path {
                // Incremental: composite only the returned dirty rect.
                if let Some(r) = s.add_point(InputPoint::at(x, y)).unwrap() {
                    s.apply(&mut t, r, rev).unwrap();
                    rev += 1;
                }
            }
            assert!(s.finish().unwrap().is_none());
            let want = reference(&fq, w, h, &path, &b, hardness);
            let mut worst = 0.0f32;
            for y in 0..h {
                for x in 0..w {
                    let g = t.pixel(x, y);
                    let e = want[(y * w + x) as usize];
                    for c in 0..4 {
                        worst = worst.max((g[c] - e[c]).abs());
                    }
                }
            }
            assert!(worst <= tol, "{depth:?} {blend:?}: max error {worst}");
        }
    }
}

fn stroke_alpha(b: &Brush, gpu: Option<Arc<GpuDabRenderer>>, w: u32, h: u32) -> Vec<f32> {
    let base = Raster::new(Extent::new(w, h), 4, Depth::F32, 0.0);
    let mut s = Stroke::new(b.clone(), &base, 9).unwrap();
    let on_gpu = gpu.is_some();
    if let Some(g) = gpu {
        s = s.with_gpu(g);
    }
    for (x, y) in [(10.0, 20.0), (120.0, 90.0), (60.0, 150.0), (190.0, 170.0)] {
        s.add_point(InputPoint::at(x, y).pressure(0.7)).unwrap();
    }
    s.finish().unwrap();
    // The GPU path really ran (no silent CPU fallback).
    assert_eq!(s.gpu_batches() > 0, on_gpu);
    (0..h as i64)
        .flat_map(|y| (0..w as i64).map(move |x| (x, y)))
        .map(|(x, y)| s.stroke_alpha(x, y))
        .collect()
}

#[test]
fn gpu_dabs_match_cpu() {
    let gpu = match GpuDabRenderer::new() {
        Ok(g) => Arc::new(g),
        Err(e) => {
            assert!(
                std::env::var("TESSERA_REQUIRE_GPU").is_err(),
                "GPU required but unavailable: {e}"
            );
            eprintln!("skipping GPU dab parity: {e}");
            return;
        }
    };
    let mut sampled = synthetic_tip("s", 23, 17);
    sampled.data.iter_mut().for_each(|v| *v = v.powf(0.7));
    let mut brushes = vec![
        Brush {
            tip: Tip::round(0.3),
            size: 28.0,
            flow: 0.35,
            opacity: 0.9,
            ..Brush::default()
        },
        Brush {
            tip: Tip::round(1.0),
            size: 9.0,
            spacing: 0.1,
            wet_edges: true,
            ..Brush::default()
        },
    ];
    let mut sb = jittery();
    sb.dual = None;
    sb.tip = Tip::sampled(sampled);
    sb.tip.roundness = 0.7;
    sb.dynamics.size.control = Control::Pressure;
    brushes.push(sb);
    for b in &brushes {
        assert!(b.gpu_compatible());
        let cpu = stroke_alpha(b, None, 220, 200);
        let g = stroke_alpha(b, Some(gpu.clone()), 220, 200);
        let worst = cpu
            .iter()
            .zip(&g)
            .map(|(a, b)| (a - b).abs())
            .fold(0.0f32, f32::max);
        let painted = cpu.iter().filter(|v| **v > 0.01).count();
        assert!(painted > 1000);
        assert!(worst < 1e-4, "gpu/cpu max diff {worst} ({})", gpu.adapter());
        eprintln!("gpu/cpu max diff {worst:e} on {}", gpu.adapter());
    }
}

fn second_difference(r: &Raster, rect: Rect) -> f32 {
    let mut worst = 0.0f32;
    for y in rect.y0..rect.y1 {
        for x in rect.x0..rect.x1 {
            let v = |dx: i64, dy: i64| r.pixel((x + dx) as u32, (y + dy) as u32)[0];
            let dxx = v(1, 0) - 2.0 * v(0, 0) + v(-1, 0);
            let dyy = v(0, 1) - 2.0 * v(0, 0) + v(0, -1);
            worst = worst.max(dxx.abs()).max(dyy.abs());
        }
    }
    worst
}

#[test]
fn heal_on_gradient_leaves_no_seam() {
    let (w, h) = (256u32, 200u32);
    let mut base = Raster::new(Extent::new(w, h), 4, Depth::F32, 0.0);
    base.edit_region(Rect::new(0, 0, 256, 200), 1, |x, y, p| {
        let (fx, fy) = (x as f32, y as f32);
        let v = if x < 110 {
            // Brighter source area with a different slope.
            0.45 + 0.0025 * fx + 0.0004 * fy
        } else {
            0.1 + 0.0015 * fx + 0.0005 * fy
        };
        *p = [v, v * 0.8, v * 0.6, 1.0];
    })
    .unwrap();
    let run = |mode: PaintMode| {
        let b = Brush {
            tip: Tip::round(1.0),
            size: 30.0,
            spacing: 0.25,
            mode,
            ..Brush::default()
        };
        let mut s = Stroke::new(b, &base, 0).unwrap();
        let mut t = base.clone();
        for (x, y) in [(150.0, 100.0), (170.0, 104.0), (190.0, 98.0)] {
            if let Some(r) = s.add_point(InputPoint::at(x, y)).unwrap() {
                s.apply(&mut t, r, 2).unwrap();
            }
        }
        t
    };
    let src = CloneSource {
        offset: [-100.0, 0.0],
        source: None,
    };
    let region = Rect::new(125, 70, 225, 130);
    let healed = run(PaintMode::Heal(src.clone()));
    let cloned = run(PaintMode::Clone(src));
    let seam_heal = second_difference(&healed, region);
    let seam_clone = second_difference(&cloned, region);
    assert!(
        seam_clone > 0.1,
        "clone control should show a seam: {seam_clone}"
    );
    assert!(seam_heal < 0.01, "heal seam {seam_heal}");
    eprintln!("second difference: heal {seam_heal:e}, clone {seam_clone:e}");
    // The heal transferred something: inside differs from the untouched base
    // only smoothly, and outside the footprint nothing changed.
    assert_eq!(healed.pixel(150, 20), base.pixel(150, 20));
}

#[test]
fn patch_blends_seamlessly() {
    let (w, h) = (160u32, 120u32);
    let mut t = Raster::new(Extent::new(w, h), 4, Depth::F32, 0.0);
    t.edit_region(Rect::new(0, 0, 160, 120), 1, |x, y, p| {
        let v = 0.2 + 0.003 * x as f32 + 0.001 * y as f32;
        // A dark blemish to remove.
        let blemish = (x as f32 - 110.0).hypot(y as f32 - 60.0) < 8.0;
        let v = if blemish { v - 0.3 } else { v };
        *p = [v, v, v, 1.0];
    })
    .unwrap();
    let mut mask = Raster::new(Extent::new(w, h), 1, Depth::F32, 0.0);
    mask.edit_region(Rect::new(96, 46, 124, 74), 1, |_, _, p| p[0] = 1.0)
        .unwrap();
    let r = patch(&mut t, &mask, [-60.0, 0.0], 2).unwrap().unwrap();
    assert!(r.width() >= 28);
    assert!(second_difference(&t, Rect::new(90, 40, 130, 80)) < 0.01);
    let want = 0.2 + 0.003 * 110.0 + 0.001 * 60.0;
    assert!((t.pixel(110, 60)[0] - want).abs() < 0.01);
}

#[test]
fn symmetry_mirrors_exactly() {
    let (w, h) = (200u32, 120u32);
    let base = Raster::new(Extent::new(w, h), 4, Depth::F32, 0.0);
    let mut b = jittery();
    b.symmetry = Symmetry::Vertical { x: 100.0 };
    let mut s = Stroke::new(b, &base, 3).unwrap();
    let mut t = base.clone();
    for (x, y) in [(20.0, 30.0), (70.0, 60.0), (40.0, 100.0)] {
        if let Some(r) = s.add_point(InputPoint::at(x, y)).unwrap() {
            s.apply(&mut t, r, 1).unwrap();
        }
    }
    let mut painted = 0;
    for y in 0..h {
        for x in 0..w {
            let (a, m) = (t.pixel(x, y)[3], t.pixel(w - 1 - x, y)[3]);
            assert!((a - m).abs() < 1e-3, "({x},{y}) {a} vs {m}");
            painted += usize::from(a > 0.0);
        }
    }
    assert!(painted > 500);
    let radial = Symmetry::Mandala {
        cx: 0.0,
        cy: 0.0,
        count: 6,
    };
    assert_eq!(radial.transforms().len(), 12);
}

#[test]
fn eraser_selection_airbrush_and_mask_targets() {
    let ext = Extent::new(100, 100);
    let mut base = Raster::new(ext, 4, Depth::F32, 0.0);
    base.edit_region(Rect::new(0, 0, 100, 100), 1, |_, _, p| {
        *p = [0.2, 0.4, 0.6, 1.0]
    })
    .unwrap();
    // Eraser removes alpha, keeps colour.
    let b = Brush {
        mode: PaintMode::Erase,
        ..Brush::default()
    };
    let mut s = Stroke::new(b, &base, 0).unwrap();
    let mut t = base.clone();
    let r = s.add_point(InputPoint::at(50.0, 50.0)).unwrap().unwrap();
    s.apply(&mut t, r, 2).unwrap();
    assert_eq!(t.pixel(50, 50), [0.2, 0.4, 0.6, 0.0]);
    assert_eq!(t.pixel(5, 5)[3], 1.0);
    // Selection limits paint.
    let mut sel = Raster::new(ext, 1, Depth::F32, 0.0);
    sel.edit_region(Rect::new(0, 0, 50, 100), 1, |_, _, p| p[0] = 1.0)
        .unwrap();
    let b = Brush {
        color: [1.0, 0.0, 0.0],
        ..Brush::default()
    };
    let mut s = Stroke::new(b.clone(), &base, 0)
        .unwrap()
        .with_selection(&sel)
        .unwrap();
    let mut t = base.clone();
    for x in [20.0, 80.0] {
        if let Some(r) = s.add_point(InputPoint::at(x, 50.0)).unwrap() {
            s.apply(&mut t, r, 2).unwrap();
        }
    }
    assert_eq!(t.pixel(30, 50), [1.0, 0.0, 0.0, 1.0]);
    assert_eq!(t.pixel(70, 50), base.pixel(70, 50));
    // Airbrush builds up while the pen is still.
    let air = Brush {
        flow: 0.1,
        airbrush: Some(50.0),
        ..b.clone()
    };
    let mut s = Stroke::new(air, &base, 0).unwrap();
    s.add_point(InputPoint::at(50.0, 50.0)).unwrap();
    let a0 = s.stroke_alpha(50, 50);
    s.tick(0.2).unwrap();
    assert_eq!(s.dabs().len(), 11);
    assert!(s.stroke_alpha(50, 50) > a0 * 5.0);
    // A 1-channel mask target lerps towards the paint luminance.
    let m = Raster::new(ext, 1, Depth::U8, 1.0);
    let black = Brush::default();
    let mut s = Stroke::new(black, &m, 0).unwrap();
    let mut mt = m.clone();
    let r = s.add_point(InputPoint::at(50.0, 50.0)).unwrap().unwrap();
    s.apply(&mut mt, r, 2).unwrap();
    assert_eq!(mt.pixel(50, 50)[0], 0.0);
    assert_eq!(mt.pixel(90, 90)[0], 1.0);
}

#[test]
fn engine_api_paint_stroke_adapter() {
    use engine_api::document::{BrushParams, StrokePoint};
    let base = Raster::new(Extent::new(300, 300), 4, Depth::U8, 0.0);
    let params = BrushParams {
        size: 40.0,
        color: [0.0, 1.0, 0.0],
        pressure_size: true,
        ..BrushParams::default()
    };
    let pts = [
        StrokePoint {
            x: 240.0,
            y: 240.0,
            pressure: 1.0,
        },
        StrokePoint {
            x: 280.0,
            y: 280.0,
            pressure: 0.5,
        },
    ];
    let (r, tiles) = brush::api::paint_stroke(&base, None, &params, &pts, 0)
        .unwrap()
        .unwrap();
    assert!(r.x0 >= 219 && r.x1 <= 300);
    // The stroke straddles the 256 tile boundary.
    assert_eq!(tiles.len(), 4);
    assert!(brush::api::paint_stroke(&base, None, &params, &[], 0).is_err());
}
