//! PL-1: slider response of the native basic tone and colour controls on a
//! fixture RAW. `PL1_TABLE=1 cargo test -p pipeline-cpu --test pl1_response
//! -- --nocapture` prints the markdown tables recorded in
//! docs/design/evidence/PL-1/.
use test_fixtures::raw as raw_fixtures;

use engine_api::recipe::DevelopSettings;
use pipeline_cpu::{Image, RenderSource, render_linear_scaled, render_scaled};
use raw_decode::RawSource;

const SCALE: u32 = 8;
const AMOUNTS: [f32; 5] = [-100.0, -50.0, 0.0, 50.0, 100.0];

struct Baseline {
    y: Vec<f32>,
    shadow: Vec<usize>,
    highlight: Vec<usize>,
    chroma: f32,
    chroma_px: Vec<f32>,
    muted: Vec<usize>,
    log_sd: f32,
    display_l: Vec<f32>,
}

fn luminance(rgb: [f32; 3]) -> f32 {
    0.2627 * rgb[0] + 0.6780 * rgb[1] + 0.0593 * rgb[2]
}

fn mul(m: [[f32; 3]; 3], v: [f32; 3]) -> [f32; 3] {
    m.map(|r| r[0] * v[0] + r[1] * v[1] + r[2] * v[2])
}

/// OkLab from linear Rec.2020, the same matrices as the colour operator.
fn oklab(rgb: [f32; 3]) -> [f32; 3] {
    let lms = mul(
        [
            [0.6167558, 0.3601984, 0.0230458],
            [0.265133, 0.6358394, 0.0990276],
            [0.1001026, 0.2039065, 0.6959909],
        ],
        rgb,
    )
    .map(f32::cbrt);
    mul(
        [
            [0.21045426, 0.7936178, -0.004072047],
            [1.9779985, -2.4285922, 0.4505937],
            [0.025904037, 0.78277177, -0.80867577],
        ],
        lms,
    )
}

fn pixels(image: &Image) -> Vec<[f32; 3]> {
    let p = image.planes();
    (0..p[0].len())
        .map(|i| [p[0][i], p[1][i], p[2][i]])
        .collect()
}

fn chroma_px(image: &Image) -> Vec<f32> {
    pixels(image)
        .iter()
        .map(|&rgb| {
            let [_, a, b] = oklab(rgb.map(|v| v.max(0.0)));
            a.hypot(b)
        })
        .collect()
}

fn mean(v: impl Iterator<Item = f32>) -> f32 {
    let (sum, n) = v.fold((0.0, 0usize), |(s, n), x| (s + x, n + 1));
    sum / n as f32
}

fn log_sd(y: &[f32]) -> f32 {
    let logs: Vec<f32> = y.iter().map(|v| v.max(1e-5).log2()).collect();
    let mean = logs.iter().sum::<f32>() / logs.len() as f32;
    (logs.iter().map(|l| (l - mean).powi(2)).sum::<f32>() / logs.len() as f32).sqrt()
}

/// CIE L* of the display render (sRGB 8-bit), per pixel.
fn display_lstar(settings: &DevelopSettings, source: &RenderSource<'_>) -> Vec<f32> {
    let rgb8 = render_scaled(settings, source, SCALE).unwrap();
    rgb8.pixels()
        .map(|p| {
            let lin: [f32; 3] = p.0.map(|c| {
                let v = c as f32 / 255.0;
                if v <= 0.04045 {
                    v / 12.92
                } else {
                    ((v + 0.055) / 1.055).powf(2.4)
                }
            });
            let y: f32 = 0.2126 * lin[0] + 0.7152 * lin[1] + 0.0722 * lin[2];
            if y > 216.0 / 24389.0 {
                116.0 * y.cbrt() - 16.0
            } else {
                24389.0 / 27.0 * y
            }
        })
        .collect()
}

fn baseline(settings: &DevelopSettings, source: &RenderSource<'_>) -> Baseline {
    let image = render_linear_scaled(settings, source, SCALE).unwrap();
    let y: Vec<f32> = pixels(&image).iter().map(|&p| luminance(p)).collect();
    let mut order: Vec<usize> = (0..y.len()).filter(|&i| y[i] > 1e-6).collect();
    order.sort_by(|&a, &b| y[a].total_cmp(&y[b]));
    let q = order.len() / 4;
    let chroma_px = chroma_px(&image);
    let mut by_chroma: Vec<usize> = (0..chroma_px.len()).collect();
    by_chroma.sort_by(|&a, &b| chroma_px[a].total_cmp(&chroma_px[b]));
    let muted = by_chroma[..by_chroma.len() / 2].to_vec();
    Baseline {
        shadow: order[..q].to_vec(),
        highlight: order[order.len() - q..].to_vec(),
        chroma: mean(chroma_px.iter().copied()),
        chroma_px,
        muted,
        log_sd: log_sd(&y),
        display_l: display_lstar(settings, source),
        y,
    }
}

struct Response {
    shadow_stops: f32,
    highlight_stops: f32,
    contrast_ratio: f32,
    chroma_ratio: f32,
    muted_chroma_ratio: f32,
    shadow_dl: f32,
    highlight_dl: f32,
}

fn respond(base: &Baseline, settings: &DevelopSettings, source: &RenderSource<'_>) -> Response {
    let image = render_linear_scaled(settings, source, SCALE).unwrap();
    let y: Vec<f32> = pixels(&image).iter().map(|&p| luminance(p)).collect();
    let stops = |set: &[usize]| {
        set.iter()
            .map(|&i| (y[i].max(1e-6) / base.y[i]).log2())
            .sum::<f32>()
            / set.len() as f32
    };
    let chroma = chroma_px(&image);
    let display = display_lstar(settings, source);
    let dl = |set: &[usize]| {
        set.iter()
            .map(|&i| display[i] - base.display_l[i])
            .sum::<f32>()
            / set.len() as f32
    };
    Response {
        shadow_stops: stops(&base.shadow),
        highlight_stops: stops(&base.highlight),
        contrast_ratio: log_sd(&y) / base.log_sd,
        chroma_ratio: mean(chroma.iter().copied()) / base.chroma,
        muted_chroma_ratio: mean(base.muted.iter().map(|&i| chroma[i]))
            / mean(base.muted.iter().map(|&i| base.chroma_px[i])),
        shadow_dl: dl(&base.shadow),
        highlight_dl: dl(&base.highlight),
    }
}

const CONTROLS: [&str; 8] = [
    "shadows",
    "highlights",
    "contrast",
    "vibrance",
    "saturation",
    "whites",
    "blacks",
    "clarity",
];

fn with_control(base: &DevelopSettings, control: &str, amount: f32) -> DevelopSettings {
    let mut s = base.clone();
    match control {
        "shadows" => s.tone.shadows = amount,
        "highlights" => s.tone.highlights = amount,
        "contrast" => s.tone.contrast = amount,
        "whites" => s.tone.whites = amount,
        "blacks" => s.tone.blacks = amount,
        "clarity" => s.tone.clarity = amount,
        "vibrance" => s.color.vibrance = amount,
        "saturation" => s.color.saturation = amount,
        other => panic!("unknown control {other}"),
    }
    s
}

fn fixture_settings() -> DevelopSettings {
    let mut settings = DevelopSettings::default();
    settings.lens.profile = engine_api::recipe::settings::LensProfileSource::None;
    settings.lens.remove_chromatic_aberration = false;
    settings
}

#[test]
fn native_slider_response_table() {
    let Some(path) = raw_fixtures::file("native_slider_response_table", "sample.dng") else {
        return;
    };
    let mut source = RawSource::open(&path).unwrap();
    let cfa = source.decode_cfa().unwrap();
    let mut metadata = source.metadata();
    metadata.maker_lens = None;
    let source = RenderSource::Cfa {
        image: &cfa,
        metadata: &metadata,
    };
    let settings = fixture_settings();
    let base = baseline(&settings, &source);
    let table = std::env::var_os("PL1_TABLE").is_some();
    if table {
        let stats = |set: &[usize]| {
            (
                mean(set.iter().map(|&i| base.y[i])),
                mean(set.iter().map(|&i| base.display_l[i])),
            )
        };
        let (sy, sl) = stats(&base.shadow);
        let (hy, hl) = stats(&base.highlight);
        println!(
            "baseline: shadow quartile mean Y {sy:.4} (L* {sl:.1}); highlight quartile mean Y {hy:.4} (L* {hl:.1}); log2 Y sd {:.3}; mean OkLab chroma {:.4}; muted-half mean chroma {:.4}",
            base.log_sd,
            base.chroma,
            mean(base.muted.iter().map(|&i| base.chroma_px[i]))
        );
        println!(
            "| control | amount | shadow quartile (stops) | highlight quartile (stops) | log-contrast ratio | mean chroma ratio | muted-half chroma ratio | shadow dL* | highlight dL* |"
        );
        println!("|---|---:|---:|---:|---:|---:|---:|---:|---:|");
    }
    let mut responses = std::collections::BTreeMap::new();
    for control in CONTROLS {
        for amount in AMOUNTS {
            let r = respond(&base, &with_control(&settings, control, amount), &source);
            if table {
                println!(
                    "| {control} | {amount:+.0} | {:+.3} | {:+.3} | {:.3} | {:.3} | {:.3} | {:+.1} | {:+.1} |",
                    r.shadow_stops,
                    r.highlight_stops,
                    r.contrast_ratio,
                    r.chroma_ratio,
                    r.muted_chroma_ratio,
                    r.shadow_dl,
                    r.highlight_dl
                );
            }
            responses.insert((control, amount as i32), r);
        }
    }
    // The after-table of docs/design/evidence/PL-1/response-after.md, with a
    // margin for fixture decoding changes. Before PL-1 every figure here was
    // five to ten times smaller (response-before.md).
    let r = |control, amount| &responses[&(control, amount)];
    for sign in [-1, 1] {
        let f = sign as f32;
        // Shadows ±100 move the shadow quartile 1.2–1.6 stops and the
        // highlight quartile under .15; Blacks ±100 move it .85–1.15.
        assert!((1.2..=1.6).contains(&(f * r("shadows", sign * 100).shadow_stops)));
        assert!(r("shadows", sign * 100).highlight_stops.abs() < 0.15);
        assert!((0.85..=1.15).contains(&(f * r("blacks", sign * 100).shadow_stops)));
        assert!(r("blacks", sign * 100).highlight_stops.abs() < 0.02);
        // Highlights ±100 move the highlight quartile .6–.9 stops (1.3 at
        // Y=1) and leave the shadow quartile alone; Whites ±100 .15–.25.
        assert!((0.6..=0.9).contains(&(f * r("highlights", sign * 100).highlight_stops)));
        assert!(r("highlights", sign * 100).shadow_stops.abs() < 0.01);
        assert!((0.15..=0.25).contains(&(f * r("whites", sign * 100).highlight_stops)));
        assert!(r("whites", sign * 100).shadow_stops.abs() < 0.01);
        // Linear in the amount: ±50 is half of ±100 within 2%.
        for control in ["shadows", "highlights", "whites", "blacks"] {
            let full = r(control, sign * 100);
            let half = r(control, sign * 50);
            let pick = |x: &Response| {
                if control == "shadows" || control == "blacks" {
                    x.shadow_stops
                } else {
                    x.highlight_stops
                }
            };
            assert!(
                (pick(half) - 0.5 * pick(full)).abs() <= 0.02 * pick(full).abs(),
                "{control} {sign}: half {} full {}",
                pick(half),
                pick(full)
            );
        }
    }
    // Contrast ±100: global log-contrast ×1.3–1.5 / ×.75–.88; the pivot
    // slope itself is exactly 2 / .5 (tone_gain_pins_the_stop_responses).
    assert!((1.3..=1.5).contains(&r("contrast", 100).contrast_ratio));
    assert!((0.75..=0.88).contains(&r("contrast", -100).contrast_ratio));
    // Vibrance +100: mean chroma ×1.4–1.6 on this warm-dominated frame (the
    // muted half too); Saturation +100 doubles chroma and −100 removes it.
    assert!((1.4..=1.6).contains(&r("vibrance", 100).chroma_ratio));
    assert!((1.35..=1.6).contains(&r("vibrance", 100).muted_chroma_ratio));
    assert!((1.95..=2.05).contains(&r("saturation", 100).chroma_ratio));
    assert!(r("saturation", -100).chroma_ratio < 0.01);
    // Tone controls leave chroma within ±15% (OkLab chroma scales with
    // luminance at fixed RGB ratios); colour controls leave tone.
    for control in ["shadows", "highlights", "contrast", "whites", "blacks"] {
        for amount in [-100, 100] {
            assert!(
                (r(control, amount).chroma_ratio - 1.0).abs() <= 0.15,
                "{control} {amount}"
            );
        }
    }
    for control in ["vibrance", "saturation"] {
        for amount in [-100, 100] {
            assert!(
                r(control, amount).shadow_stops.abs() < 0.08,
                "{control} {amount}"
            );
            assert!(
                r(control, amount).highlight_stops.abs() < 0.08,
                "{control} {amount}"
            );
        }
    }
}

/// Fixture-free pins of the tone model at single luminances, in stops.
#[test]
fn tone_gain_pins_the_stop_responses() {
    use engine_api::{
        recipe::settings::ToneSettings,
        tile::{Extent, Tile, TileCoord, TileLayout},
    };
    let stops = |y: f32, s: &ToneSettings| {
        let mut tile = Tile::from_samples(
            TileCoord::new(0, 0, 0),
            TileLayout {
                extent: Extent::new(1, 1),
                halo: 0,
                channels: 3,
            },
            vec![y; 3],
        )
        .unwrap();
        pipeline_cpu::tone(&mut tile, s).unwrap();
        (tile.samples::<f32>().unwrap()[0] / y).log2()
    };
    fn with(f: impl FnOnce(&mut ToneSettings)) -> ToneSettings {
        let mut s = ToneSettings::default();
        f(&mut s);
        s
    }
    let near = |a: f32, b: f32| (a - b).abs() <= 0.03;
    // Shadows/Blacks are a constant gain near black and vanish above grey.
    let s = with(|s| s.shadows = 100.0);
    assert!(near(stops(0.0065, &s), 1.39));
    assert!(near(stops(0.18, &s), 0.20));
    assert!(near(stops(1.0, &s), 0.02));
    let s = with(|s| s.shadows = -100.0);
    assert!(near(stops(0.0065, &s), -1.39));
    let s = with(|s| s.shadows = 20.0);
    assert!(near(stops(0.0065, &s), 0.28));
    let s = with(|s| s.blacks = -100.0);
    assert!(near(stops(0.001, &s), -1.24));
    assert!(near(stops(0.05, &s), -0.14));
    assert!(near(stops(0.5, &s), 0.0));
    // Highlights/Whites are the mirror.
    let s = with(|s| s.highlights = -100.0);
    assert!(near(stops(1.0, &s), -1.32));
    assert!(near(stops(0.5, &s), -0.91));
    assert!(near(stops(0.18, &s), -0.27));
    assert!(near(stops(0.02, &s), -0.01));
    let s = with(|s| s.highlights = 20.0);
    assert!(near(stops(1.0, &s), 0.26));
    let s = with(|s| s.whites = 100.0);
    assert!(near(stops(4.0, &s), 0.88));
    assert!(near(stops(1.0, &s), 0.42));
    assert!(near(stops(0.18, &s), 0.04));
    // Contrast: pivot slope 2 / .5 in log luminance, shift bounded by
    // 1.5 / .75 stops, grey fixed.
    for (contrast, slope, reach) in [(100.0, 2.0, 1.5), (-100.0, 0.5, -0.75)] {
        let s = with(|s| s.contrast = contrast);
        assert!(stops(0.18, &s).abs() < 1e-3);
        let h = 0.01f32;
        let a = (0.18 * (-h).exp()).max(0.0);
        let b = 0.18 * h.exp();
        let measured = (stops(b, &s) - stops(a, &s)) * std::f32::consts::LN_2 / (2.0 * h) + 1.0;
        assert!((measured - slope).abs() < 0.02, "slope {measured}");
        assert!(near(stops(0.001, &s), -reach));
        assert!(near(stops(16.0, &s), reach));
    }
    // Exposure is untouched: +1 EV doubles everything exactly.
    let s = with(|s| s.exposure = 1.0);
    assert_eq!(stops(0.3, &s), 1.0);
}

/// Vibrance on single colours: relative saturation `S = chroma / (0.25 L)`
/// decides how much of the boost a colour takes.
#[test]
fn vibrance_probe_table() {
    use engine_api::{
        recipe::settings::ColorSettings,
        tile::{Extent, Tile, TileCoord, TileLayout},
    };
    let table = std::env::var_os("PL1_TABLE").is_some();
    if table {
        println!(
            "| colour (linear Rec.2020) | OkLab L | S | vibrance +100 | vibrance +50 | vibrance -100 | saturation +100 |"
        );
        println!("|---|---:|---:|---:|---:|---:|---:|");
    }
    for rgb in [
        [0.19, 0.18, 0.17],
        [0.21, 0.18, 0.15],
        [0.16, 0.18, 0.22],
        [0.12, 0.18, 0.28],
        [0.3, 0.18, 0.1],
        [0.1, 0.15, 0.4],
        [0.03, 0.02, 0.015],
        [0.7, 0.6, 0.5],
    ] {
        let [l, a, b] = oklab(rgb);
        let chroma = a.hypot(b);
        let relative = chroma / (0.25 * l.abs().max(0.05));
        let run = |vibrance: f32, saturation: f32| {
            let mut tile = Tile::from_samples(
                TileCoord::new(0, 0, 0),
                TileLayout {
                    extent: Extent::new(1, 1),
                    halo: 0,
                    channels: 3,
                },
                rgb.to_vec(),
            )
            .unwrap();
            let s = ColorSettings {
                vibrance,
                saturation,
                ..Default::default()
            };
            pipeline_cpu::color(&mut tile, &s).unwrap();
            let v = tile.samples::<f32>().unwrap();
            let [_, a, b] = oklab([v[0], v[1], v[2]]);
            a.hypot(b) / chroma
        };
        if table {
            println!(
                "| {rgb:?} | {l:.2} | {relative:.2} | {:.2} | {:.2} | {:.2} | {:.2} |",
                run(100.0, 0.0),
                run(50.0, 0.0),
                run(-100.0, 0.0),
                run(0.0, 100.0)
            );
        }
    }
}
