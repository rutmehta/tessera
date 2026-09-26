//! Selection tool contracts (WP M5-05): each tool on a synthetic image
//! selects the expected region with IoU > 0.95.

use compositor::{Affine, Depth};
use engine_api::document::{CanvasRect, SelectionMode, SelectionShape};
use engine_api::tile::Extent;
use engine_api::{EngineError, EngineResult};
use selection::focus::{FocusOptions, focus_area};
use selection::lasso::{self, MagneticLasso};
use selection::ml::{ObjectPrompt, SegmentModel, select_object, select_sky, select_subject};
use selection::quick::{QuickOptions, quick_select};
use selection::range::{Tonal, color_range, tonal_range};
use selection::refine::{RefineParams, decontaminate, refine_edge};
use selection::wand::{WandOptions, magic_wand};
use selection::{Combine, Image, Mask, channels, contour, marquee, ops};

fn noise(x: u32, y: u32, seed: u32) -> f32 {
    let mut h =
        x.wrapping_mul(0x9E37_79B1) ^ y.wrapping_mul(0x85EB_CA77) ^ seed.wrapping_mul(0xC2B2_AE3D);
    h ^= h >> 15;
    h = h.wrapping_mul(0x2C1B_3C6D);
    h ^= h >> 12;
    h = h.wrapping_mul(0x297A_2D39);
    h ^= h >> 15;
    (h >> 8) as f32 / (1u32 << 24) as f32
}

fn disc(w: u32, h: u32, cx: f32, cy: f32, r: f32) -> Mask {
    Mask::from_fn(w, h, |x, y| {
        f32::from(u8::from(
            (x as f32 + 0.5 - cx).hypot(y as f32 + 0.5 - cy) <= r,
        ))
    })
}

fn square(w: u32, h: u32, x0: u32, y0: u32, x1: u32, y1: u32) -> Mask {
    Mask::from_fn(w, h, |x, y| {
        f32::from(u8::from(x >= x0 && x < x1 && y >= y0 && y < y1))
    })
}

fn assert_iou(got: &Mask, want: &Mask, what: &str) {
    let iou = got.iou(want);
    assert!(iou > 0.95, "{what}: IoU {iou}");
}

#[test]
fn marquee_rect_ellipse_rows() {
    let r = marquee::rect(100, 80, [10.25, 20.5, 50.75, 60.0], true);
    assert!((r.area() - 40.5 * 39.5).abs() < 1e-3);
    assert!((r.get(10, 30) - 0.75).abs() < 1e-6 && (r.get(30, 20) - 0.5).abs() < 1e-6);
    assert_iou(
        &marquee::rect(100, 80, [10.0, 20.0, 50.0, 60.0], false),
        &square(100, 80, 10, 20, 50, 60),
        "rect",
    );
    let e = marquee::ellipse(120, 100, [10.0, 20.0, 110.0, 80.0], true);
    let want = std::f64::consts::PI * 50.0 * 30.0;
    assert!(
        (e.area() - want).abs() / want < 0.005,
        "ellipse area {}",
        e.area()
    );
    let expect = Mask::from_fn(120, 100, |x, y| {
        let (u, v) = (
            (x as f32 + 0.5 - 60.0) / 50.0,
            (y as f32 + 0.5 - 50.0) / 30.0,
        );
        f32::from(u8::from(u * u + v * v <= 1.0))
    });
    assert_iou(&e, &expect, "ellipse");
    assert_eq!(marquee::single_row(10, 10, 3).area(), 10.0);
    assert_eq!(marquee::single_column(10, 10, 3).get(3, 9), 1.0);
}

#[test]
fn lasso_and_polygonal() {
    let tri = [[10.0, 10.0], [90.0, 20.0], [30.0, 70.0]];
    let m = lasso::polygonal(100, 80, &tri, true);
    let area = 0.5 * ((90.0 - 10.0) * (70.0 - 10.0) - (30.0 - 10.0) * (20.0 - 10.0));
    assert!(
        (m.area() - area).abs() / area < 0.005,
        "area {} vs {area}",
        m.area()
    );
    let hard = lasso::polygonal(100, 80, &tri, false);
    assert_iou(&m, &hard, "aa vs hard");
    // Freehand: a dense circle path selects the disc; even-odd leaves the
    // hole of a self-overlapping double loop.
    let path: Vec<[f32; 2]> = (0..200)
        .map(|i| {
            let t = i as f32 / 200.0 * std::f32::consts::TAU;
            [60.0 + 30.0 * t.cos(), 40.0 + 30.0 * t.sin()]
        })
        .collect();
    assert_iou(
        &lasso::freehand(120, 80, &path, true),
        &disc(120, 80, 60.0, 40.0, 30.0),
        "freehand",
    );
}

fn square_image(noise_amp: f32) -> Image {
    Image::from_fn(160, 160, |x, y| {
        let inside = (40..100).contains(&x) && (40..100).contains(&y);
        let v = if inside { 0.8 } else { 0.2 } + noise_amp * (noise(x, y, 1) - 0.5);
        [v, v * 0.9, v * 0.7, 1.0]
    })
}

#[test]
fn magnetic_lasso_snaps_to_edges() {
    let img = square_image(0.04);
    // Sloppy clicks up to ~3 px off the square's corners and sides.
    let anchors = [
        [42.0, 38.0],
        [70.0, 42.5],
        [98.0, 41.0],
        [101.0, 70.0],
        [99.0, 102.0],
        [69.0, 97.5],
        [38.5, 99.0],
        [41.0, 71.0],
    ];
    let m = lasso::magnetic(&img, &anchors, 6.0, 0.05, true);
    assert_iou(&m, &square(160, 160, 40, 40, 100, 100), "magnetic");
    // The traced outline lies on the pixel-corner lattice of the edge.
    let l = MagneticLasso::new(&img, 0.05);
    let path = l.trace(&anchors, 6.0);
    let on_edge = path
        .iter()
        .filter(|p| p[0] == 40.0 || p[0] == 100.0 || p[1] == 40.0 || p[1] == 100.0)
        .count();
    assert!(
        on_edge as f32 / path.len() as f32 > 0.9,
        "{on_edge}/{}",
        path.len()
    );
}

#[test]
fn magic_wand_contiguous_and_global() {
    let d1 = disc(200, 120, 50.0, 60.0, 30.0);
    let d2 = disc(200, 120, 150.0, 60.0, 25.0);
    let img = Image::from_fn(200, 120, |x, y| {
        let n = (noise(x, y, 2) - 0.5) * 8.0 / 255.0;
        if d1.get(x as i64, y as i64) > 0.0 || d2.get(x as i64, y as i64) > 0.0 {
            [0.1 + n, 0.2 + n, 0.8 + n, 1.0]
        } else {
            let g = x as f32 / 200.0;
            [g, 0.6, 0.3, 1.0]
        }
    });
    let o = WandOptions {
        tolerance: 20.0,
        ..WandOptions::default()
    };
    let m = magic_wand(&img, (50, 60), &o);
    assert_iou(&m, &d1, "wand contiguous");
    assert!(m.get(150, 60) < 0.01);
    let g = magic_wand(
        &img,
        (50, 60),
        &WandOptions {
            contiguous: false,
            ..o
        },
    );
    assert_iou(
        &g,
        &ops::combine(&d1, &d2, Combine::Add).unwrap(),
        "wand global",
    );
}

#[test]
fn quick_selection_colour_and_texture() {
    // Colour: red disc on noisy blue.
    let d = disc(160, 120, 70.0, 60.0, 35.0);
    let img = Image::from_fn(160, 120, |x, y| {
        let n = (noise(x, y, 3) - 0.5) * 0.1;
        if d.get(x as i64, y as i64) > 0.0 {
            [0.85 + n, 0.15 + n, 0.1, 1.0]
        } else {
            [0.1, 0.2 + n, 0.75 + n, 1.0]
        }
    });
    let o = QuickOptions {
        radius: 6.0,
        ..QuickOptions::default()
    };
    let m = quick_select(&img, &[[50.0, 50.0], [85.0, 70.0]], &o, None, false);
    assert_iou(&m, &d, "quick colour");
    // Texture: same mean grey, left half textured, right half flat.
    let img = Image::from_fn(200, 100, |x, y| {
        let v = if x < 100 {
            0.5 + 0.5 * (noise(x, y, 4) - 0.5)
        } else {
            0.5 + 0.01 * (noise(x, y, 5) - 0.5)
        };
        [v, v, v, 1.0]
    });
    let left = square(200, 100, 0, 0, 100, 100);
    let t = quick_select(&img, &[[20.0, 20.0], [60.0, 80.0]], &o, None, false);
    assert_iou(&t, &left, "quick texture");
    // Add then subtract with a second stroke.
    let right = quick_select(&img, &[[150.0, 50.0]], &o, Some(&t), false);
    assert!(right.area() > 0.97 * 200.0 * 100.0);
    let back = quick_select(&img, &[[150.0, 50.0]], &o, Some(&right), true);
    assert_iou(&back, &left, "quick subtract");
}

#[test]
fn colour_and_tonal_range() {
    let red = square(180, 80, 10, 10, 60, 70);
    let green = square(180, 80, 65, 10, 115, 70);
    let img = Image::from_fn(180, 80, |x, y| {
        let n = (noise(x, y, 6) - 0.5) * 0.04;
        if red.get(x as i64, y as i64) > 0.0 {
            [0.8 + n, 0.1 + n, 0.1, 1.0]
        } else if green.get(x as i64, y as i64) > 0.0 {
            [0.1, 0.7 + n, 0.2, 1.0]
        } else if x >= 120 && (10..70).contains(&y) {
            [0.9, 0.5, 0.1, 1.0] // orange: near red but outside the fuzziness
        } else {
            [0.5 + n, 0.5 + n, 0.5 + n, 1.0]
        }
    });
    let m = color_range(&img, &[[0.8, 0.1, 0.1]], 24.0);
    assert!(m.get(150, 40) == 0.0, "orange excluded");
    assert_iou(&m, &red, "colour range");
    // Highlights of a horizontal ramp: L* ≥ 65 ⇔ sRGB ≥ ~0.62.
    let ramp = Image::from_fn(256, 8, |x, _| {
        let v = x as f32 / 255.0;
        [v, v, v, 1.0]
    });
    let hl = tonal_range(&ramp, Tonal::Highlights);
    assert!(hl.get(250, 4) == 1.0 && hl.get(100, 4) == 0.0);
    let sh = tonal_range(&ramp, Tonal::Shadows);
    assert!(sh.get(5, 4) == 1.0 && sh.get(200, 4) == 0.0);
    let skin = Image::from_fn(2, 1, |x, _| {
        if x == 0 {
            [0.88, 0.67, 0.55, 1.0]
        } else {
            [0.1, 0.3, 0.9, 1.0]
        }
    });
    let s = tonal_range(&skin, Tonal::SkinTones);
    assert!(s.get(0, 0) > 0.9 && s.get(1, 0) == 0.0);
}

#[test]
fn focus_area_selects_sharp_half() {
    let (w, h) = (256u32, 128u32);
    let raw: Vec<f32> = (0..w * h).map(|i| noise(i % w, i / w, 7)).collect();
    let blurred = selection::filter::gaussian(&raw, w as usize, h as usize, 3.0);
    let img = Image::from_fn(w, h, |x, y| {
        let i = (y * w + x) as usize;
        let v = if x < 128 { raw[i] } else { blurred[i] };
        [v, v, v, 1.0]
    });
    let m = focus_area(&img, &FocusOptions::default());
    assert_iou(&m, &square(w, h, 0, 0, 128, 128), "focus area");
}

/// Mock network: masks at quarter resolution, like a real model's output.
struct Mock;
impl SegmentModel for Mock {
    fn subject(&mut self, _: &Image) -> EngineResult<Mask> {
        Ok(disc(40, 30, 20.0, 18.0, 8.0))
    }
    fn sky(&mut self, _: &Image) -> EngineResult<Mask> {
        Ok(Mask::from_fn(40, 30, |_, y| f32::from(u8::from(y < 10))))
    }
    fn object(&mut self, _: &Image, p: &ObjectPrompt) -> EngineResult<Mask> {
        let ObjectPrompt::Box(b) = p else {
            return Err(EngineError::invalid("prompt", "box only"));
        };
        Ok(Mask::from_fn(40, 30, |x, y| {
            let (px, py) = (x as f32 * 4.0 + 2.0, y as f32 * 4.0 + 2.0);
            let inside = px >= b[0] && px < b[2] && py >= b[1] && py < b[3];
            f32::from(u8::from(inside && (px - 50.0).hypot(py - 60.0) <= 20.0))
        }))
    }
}

#[test]
fn subject_sky_object_via_model() {
    let subject = disc(160, 120, 80.0, 72.0, 32.0);
    let obj = disc(160, 120, 50.0, 60.0, 20.0);
    let img = Image::from_fn(160, 120, |x, y| {
        let (xi, yi) = (x as i64, y as i64);
        if subject.get(xi, yi) > 0.0 {
            [0.9, 0.6, 0.4, 1.0]
        } else if obj.get(xi, yi) > 0.0 {
            [0.2, 0.8, 0.2, 1.0]
        } else if y < 40 {
            [0.4, 0.6, 0.95, 1.0]
        } else {
            [0.3, 0.3, 0.3, 1.0]
        }
    });
    assert_iou(
        &select_subject(&mut Mock, &img, 4).unwrap(),
        &subject,
        "subject",
    );
    assert_iou(
        &select_sky(&mut Mock, &img, 4).unwrap(),
        &square(160, 120, 0, 0, 160, 40),
        "sky",
    );
    let b = select_object(
        &mut Mock,
        &img,
        &ObjectPrompt::Box([20.0, 30.0, 80.0, 90.0]),
        2,
    )
    .unwrap();
    assert_iou(&b, &obj, "object box");
    let lasso = vec![[25.0, 35.0], [78.0, 32.0], [80.0, 88.0], [22.0, 86.0]];
    let l = select_object(&mut Mock, &img, &ObjectPrompt::Lasso(lasso), 2).unwrap();
    assert_iou(&l, &obj, "object lasso");
    assert!(select_object(&mut Mock, &img, &ObjectPrompt::Points(vec![]), 2).is_err());
}

#[test]
fn boolean_ops_and_modify() {
    let a = square(100, 100, 10, 10, 60, 60);
    let b = square(100, 100, 40, 40, 90, 90);
    let area = |m: &Mask| m.area().round() as i64;
    assert_eq!(
        area(&ops::combine(&a, &b, Combine::Add).unwrap()),
        2500 + 2500 - 400
    );
    assert_eq!(
        area(&ops::combine(&a, &b, Combine::Subtract).unwrap()),
        2500 - 400
    );
    assert_eq!(
        area(&ops::combine(&a, &b, Combine::Intersect).unwrap()),
        400
    );
    assert_eq!(
        area(&ops::combine(&a, &b, Combine::Xor).unwrap()),
        2 * (2500 - 400)
    );
    assert_eq!(ops::combine(&a, &b, Combine::Replace).unwrap(), b);
    assert_eq!(area(&ops::invert(&a)), 10_000 - 2500);
    assert!(ops::combine(&a, &Mask::new(5, 5), Combine::Add).is_err());
    // Grow/contract move a disc's radius; border is a ring.
    let d = disc(120, 120, 60.0, 60.0, 20.0);
    let pi = std::f64::consts::PI;
    let g = ops::grow(&d, 5.0);
    assert!(
        (g.area() - pi * 625.0).abs() / (pi * 625.0) < 0.03,
        "grow {}",
        g.area()
    );
    let c = ops::contract(&d, 5.0);
    assert!(
        (c.area() - pi * 225.0).abs() / (pi * 225.0) < 0.05,
        "contract {}",
        c.area()
    );
    let ring = ops::border(&d, 4.0);
    assert!(ring.get(60, 60) == 0.0 && ring.get(80, 60) > 0.5);
    // Smooth rounds a square's corners but keeps its bulk.
    let s = ops::smooth(&a, 6.0);
    assert!(s.get(10, 10) < 0.5 && s.get(35, 35) == 1.0);
    let f = ops::feather(&a, 4.0);
    assert!(f.get(10, 35) > 0.3 && f.get(10, 35) < 0.7);
}

#[test]
fn transform_selection() {
    let a = square(100, 100, 10, 10, 40, 30);
    let t = Affine::scale_translate(1.0, 1.0, 12.0, 5.0);
    let m = ops::transform(&a, &t, 100, 100).unwrap();
    assert_iou(&m, &square(100, 100, 22, 15, 52, 35), "translate");
    let s = ops::transform(&a, &Affine::scale_translate(2.0, 2.0, 0.0, 0.0), 100, 100).unwrap();
    assert_iou(&s, &square(100, 100, 20, 20, 80, 60), "scale");
    assert!(ops::transform(&a, &Affine::scale_translate(0.0, 1.0, 0.0, 0.0), 10, 10).is_err());
}

#[test]
fn refine_edge_preserves_a_hard_edge() {
    let (w, h) = (128u32, 64u32);
    let img = Image::from_fn(w, h, |x, _| {
        if x >= 64 {
            [0.9, 0.9, 0.9, 1.0]
        } else {
            [0.1, 0.1, 0.1, 1.0]
        }
    });
    let mask = square(w, h, 64, 0, 128, 64);
    for smart in [false, true] {
        let p = RefineParams {
            radius: 8.0,
            smart_radius: smart,
            ..RefineParams::default()
        };
        let r = refine_edge(&mask, &img, &p).unwrap();
        for y in [0, 31, 63] {
            for x in 0..w as i64 {
                let v = r.get(x, y);
                if x <= 62 {
                    assert!(v < 0.02, "smart={smart} ({x},{y}) {v}");
                } else if x >= 65 {
                    assert!(v > 0.98, "smart={smart} ({x},{y}) {v}");
                }
            }
            assert!(r.get(63, y) < 0.5 && r.get(64, y) > 0.5);
        }
        assert_iou(&r, &mask, "refine");
    }
    // Soft image edge: the guided filter makes the alpha follow the ramp.
    let soft = Image::from_fn(w, h, |x, _| {
        let v = ((x as f32 - 56.0) / 16.0).clamp(0.0, 1.0);
        [v, v, v, 1.0]
    });
    let r = refine_edge(
        &mask,
        &soft,
        &RefineParams {
            radius: 12.0,
            smart_radius: true,
            ..RefineParams::default()
        },
    )
    .unwrap();
    let partial = (0..w as i64)
        .filter(|&x| (0.02..0.98).contains(&r.get(x, 10)))
        .count();
    assert!(partial >= 4, "soft edge band {partial} px");
    // Shift edge, feather, contrast.
    let shifted = refine_edge(
        &mask,
        &img,
        &RefineParams {
            shift_edge: 3.0,
            ..RefineParams::default()
        },
    )
    .unwrap();
    assert!(shifted.get(61, 5) > 0.98 && shifted.get(60, 5) < 0.02);
    let feathered = refine_edge(
        &mask,
        &img,
        &RefineParams {
            feather: 3.0,
            ..RefineParams::default()
        },
    )
    .unwrap();
    assert!(feathered.get(62, 5) > 0.2);
    let hard = refine_edge(
        &feathered,
        &img,
        &RefineParams {
            contrast: 1.0,
            ..RefineParams::default()
        },
    )
    .unwrap();
    assert_iou(&hard, &mask, "contrast");
    // Decontaminate pulls fringe colours towards the interior.
    let fringe = Mask::from_fn(w, h, |x, _| ((x as f32 - 60.0) / 8.0).clamp(0.0, 1.0));
    let dc = decontaminate(&img, &fringe, 1.0, 6).unwrap();
    assert!(dc.data[10 * w as usize + 62][0] > img.data[10 * w as usize + 62][0]);
}

#[test]
fn contour_tracing_closes_disc_outlines() {
    let (cx, cy, r) = (64.0f32, 60.0f32, 30.0f32);
    // Anti-aliased disc (exact coverage by supersampling).
    let d = Mask::from_fn(128, 128, |x, y| {
        let mut c = 0.0;
        for sy in 0..8 {
            for sx in 0..8 {
                let (px, py) = (
                    x as f32 + (sx as f32 + 0.5) / 8.0,
                    y as f32 + (sy as f32 + 0.5) / 8.0,
                );
                c += f32::from(u8::from((px - cx).hypot(py - cy) <= r));
            }
        }
        c / 64.0
    });
    let lines = contour::trace(&d, 0.5, 0.0);
    assert_eq!(lines.len(), 1);
    let l = &lines[0];
    assert!(l.closed && l.points.len() > 100);
    let n = l.points.len();
    for i in 0..n {
        let (p, q) = (l.points[i], l.points[(i + 1) % n]);
        assert!((p[0] - q[0]).hypot(p[1] - q[1]) < 1.5, "gap at {i}");
        let e = ((p[0] - cx).hypot(p[1] - cy) - r).abs();
        assert!(e < 0.25, "radius error {e}");
    }
    let want = std::f32::consts::PI * r * r;
    assert!((l.area().abs() - want).abs() / want < 0.01);
    let simple = contour::trace(&d, 0.5, 0.3);
    assert!(simple[0].points.len() < n / 2);
    // An annulus yields an outer outline and a hole of opposite orientation;
    // separate blobs yield separate outlines; binary masks close too.
    let ring = ops::combine(
        &disc(100, 100, 50.0, 50.0, 30.0),
        &disc(100, 100, 50.0, 50.0, 12.0),
        Combine::Subtract,
    )
    .unwrap();
    let rl = contour::trace(&ring, 0.5, 0.0);
    assert_eq!(rl.len(), 2);
    assert!(rl[0].area() * rl[1].area() < 0.0);
    // Outlines have the selection on their left: negative area (y down).
    assert!(l.area() < 0.0);
    let two = ops::combine(
        &disc(100, 100, 25.0, 25.0, 10.0),
        &square(100, 100, 60, 60, 99, 100),
        Combine::Add,
    )
    .unwrap();
    assert_eq!(contour::trace(&two, 0.5, 0.0).len(), 2);
}

#[test]
fn alpha_channel_save_load() {
    let m = marquee::ellipse(300, 280, [20.0, 30.0, 290.0, 200.0], true);
    let mut ch = channels::AlphaChannels::new(Extent::new(300, 280), Depth::U8);
    ch.save("Alpha 1", &m).unwrap();
    let back = ch.load("Alpha 1").unwrap();
    let worst = m
        .data()
        .iter()
        .zip(back.data())
        .map(|(a, b)| (a - b).abs())
        .fold(0.0f32, f32::max);
    assert!(worst <= 0.5 / 255.0 + 1e-6);
    let mut f = channels::AlphaChannels::new(Extent::new(300, 280), Depth::F32);
    f.save("exact", &m).unwrap();
    assert_eq!(f.load("exact").unwrap(), m);
    assert_eq!(f.names().collect::<Vec<_>>(), ["exact"]);
    // The raster only stores tiles that contain selection.
    let tiny = marquee::rect(600, 600, [1.0, 1.0, 5.0, 5.0], false);
    let r = tiny.to_raster(Depth::F32).unwrap();
    assert_eq!(r.tile_count(), 1);
    assert!(ch.load("missing").is_err());
    assert!(ch.save("bad", &Mask::new(3, 3)).is_err());
}

#[test]
fn set_pixel_selection_adapter() {
    let ext = Extent::new(120, 100);
    let cr = |x0, y0, x1, y1| CanvasRect { x0, y0, x1, y1 };
    let mut none =
        |_: &SelectionShape| -> EngineResult<Mask> { Err(EngineError::invalid("x", "unused")) };
    let r = selection::api::set_pixel_selection(
        None,
        ext,
        &SelectionShape::Rect {
            rect: cr(10, 10, 60, 60),
        },
        SelectionMode::Replace,
        0.0,
        &mut none,
    )
    .unwrap()
    .unwrap();
    assert_eq!((r.channels(), r.depth()), (1, Depth::F32));
    let s = selection::api::set_pixel_selection(
        Some(&r),
        ext,
        &SelectionShape::Ellipse {
            rect: cr(40, 40, 80, 80),
        },
        SelectionMode::Subtract,
        0.0,
        &mut none,
    )
    .unwrap()
    .unwrap();
    let m = Mask::from_raster(&s).unwrap();
    assert_eq!(m.get(20, 20), 1.0);
    assert_eq!(m.get(58, 58), 0.0);
    let inv = selection::api::set_pixel_selection(
        Some(&s),
        ext,
        &SelectionShape::Inverse,
        SelectionMode::Replace,
        0.0,
        &mut none,
    )
    .unwrap()
    .unwrap();
    assert_eq!(Mask::from_raster(&inv).unwrap().get(20, 20), 0.0);
    assert!(
        selection::api::set_pixel_selection(
            Some(&s),
            ext,
            &SelectionShape::None,
            SelectionMode::Replace,
            0.0,
            &mut none
        )
        .unwrap()
        .is_none()
    );
    let mut saved = |_: &SelectionShape| Ok(square(120, 100, 0, 0, 10, 10));
    let v = selection::api::set_pixel_selection(
        Some(&s),
        ext,
        &SelectionShape::Saved {
            selection: serde_json::from_value(serde_json::json!(1)).unwrap(),
        },
        SelectionMode::Add,
        2.0,
        &mut saved,
    )
    .unwrap()
    .unwrap();
    let vm = Mask::from_raster(&v).unwrap();
    assert!(vm.get(2, 2) > 0.9 && vm.get(20, 20) == 1.0);
    let tri = SelectionShape::Polygon {
        points: vec![[0.0, 0.0], [50.0, 0.0], [0.0, 50.0]],
    };
    assert!(
        selection::api::set_pixel_selection(
            None,
            ext,
            &tri,
            SelectionMode::Intersect,
            0.0,
            &mut none
        )
        .unwrap()
        .is_some()
    );
}
