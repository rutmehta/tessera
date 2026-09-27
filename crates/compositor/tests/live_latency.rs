//! Manual CPU frame benchmark: cargo test -p compositor --release --test live_latency -- --ignored --nocapture
mod common;
use common::*;
use compositor::*;
use engine_api::tile::Extent;
use std::time::Instant;

fn fonts() -> typography::TextRenderer {
    let mut f = typography::TextRenderer::new();
    f.fonts_mut().load_font_data(
        include_bytes!("../../typography/tests/fonts/NotoSans-Regular.ttf").to_vec(),
    );
    f
}
fn report(label: &str, level: u8, mut times: Vec<f64>, limits: (f64, f64)) -> bool {
    times.sort_by(f64::total_cmp);
    let median = times[times.len() / 2];
    let p95 = times[(times.len() as f64 * 0.95).ceil() as usize - 1];
    eprintln!(
        "{label} L{level}, {} edits: median {median:.3} ms, p95 {p95:.3} ms; samples {times:?}",
        times.len()
    );
    // Round 2 explicitly accepts Fit/L2 medians and records contended tails.
    // Keep the original tail requirement runnable, not hidden or discarded.
    let strict = std::env::var_os("M5_34_STRICT_P95").is_some();
    if p95 >= limits.1 {
        eprintln!(
            "{label} L{level}: p95 target {} ms MISSED (strict={strict})",
            limits.1
        );
    }
    level != 2 || (median < limits.0 && (!strict || p95 < limits.1))
}

#[test]
#[ignore = "20MP CPU frame latency benchmark; run in release mode on an idle machine"]
fn photo_typing_and_shape_handle_cpu_latency() {
    eprintln!(
        "Acceptance: Fit L2 median <16ms typing/fill, <33ms inside-dashed; set M5_34_STRICT_P95=1 to also gate p95 <25/50ms. L3 is diagnostic."
    );
    // Decoded, packed sRGB photo bytes, prepared outside the timed interval.
    // See tools/orchestrate/wp/M5-34/RESULTS.md for fixture preparation.
    let extent = Extent::new(5472, 3648);
    let path = std::env::var("M5_34_PHOTO_RGB")
        .expect("set M5_34_PHOTO_RGB to a 5472x3648 packed RGB8 photo");
    let rgb = std::fs::read(path).unwrap();
    assert_eq!(
        rgb.len(),
        extent.width as usize * extent.height as usize * 3
    );
    let photo = layer_fn("20MP raster photo", extent, Depth::U8, |x, y| {
        let i = (y as usize * extent.width as usize + x as usize) * 3;
        [
            rgb[i] as f32 / 255.,
            rgb[i + 1] as f32 / 255.,
            rgb[i + 2] as f32 / 255.,
            1.,
        ]
    });
    let mut targets_met = true;
    for level in [2, 3] {
        for case in ["typing 274px", "fill-only handle", "inside-dashed handle"] {
            let shape = case != "typing 274px";
            let mut d = doc(extent, Depth::U8);
            add(&mut d, None, photo.clone());
            let mut model = typography::TextModel::point("", "Noto Sans", 274.);
            model.text_box = typography::TextBox::Paragraph {
                width: 5072.,
                height: 2400.,
            };
            let transform = Affine::scale_translate(1., 1., 200., 600.);
            let make_shape = |i: usize| vector::ShapeModel {
                path: vector::Path::polyline(
                    &[
                        vector::Point::new(200., 400.),
                        vector::Point::new(700. + i as f64 * 7., 450. + i as f64 * 2.),
                        vector::Point::new(600., 900.),
                        vector::Point::new(180., 850.),
                    ],
                    true,
                ),
                fill: Some(vector::Fill::Solid([0.8, 0.3, 0.1, 1.])),
                stroke: (case == "inside-dashed handle").then_some((
                    vector::Stroke {
                        width: 24.,
                        alignment: vector::Alignment::Inside,
                        dashes: vec![32., 16.],
                        ..Default::default()
                    },
                    vector::Fill::Solid([0., 0., 0., 1.]),
                )),
                ..Default::default()
            };
            let kind = if shape {
                LayerKind::Shape {
                    model: make_shape(0),
                    transform,
                }
            } else {
                LayerKind::Text {
                    model: model.clone(),
                    transform,
                }
            };
            let id = add(&mut d, None, Layer::new("editable", kind));
            let comp = Compositor::new(512 << 20);
            comp.set_text_renderer(fonts());
            comp.render_level_rgba(&d, level).unwrap();
            let mut times = Vec::new();
            let mut stages = [Vec::new(), Vec::new()];
            let sentence = "The quick brown fox jumps over the lazy dog";
            assert_eq!(sentence.chars().count(), 43);
            for (i, ch) in sentence.chars().enumerate() {
                let start = Instant::now();
                if shape {
                    d.apply(DocOp::EditShape {
                        id,
                        model: make_shape(i + 1),
                        transform,
                    })
                    .unwrap();
                } else {
                    model.runs[0].text.push(ch);
                    d.apply(DocOp::EditText {
                        id,
                        model: model.clone(),
                        transform,
                    })
                    .unwrap();
                }
                let edited = Instant::now();
                let frame = comp.render_level_rgba(&d, level).unwrap();
                std::hint::black_box(frame);
                times.push(start.elapsed().as_secs_f64() * 1000.);
                stages[0].push(edited.duration_since(start).as_secs_f64() * 1000.);
                stages[1].push(edited.elapsed().as_secs_f64() * 1000.);
            }
            eprintln!("final counters: {:?}", comp.stats());
            targets_met &= report(
                case,
                level,
                times,
                if case == "inside-dashed handle" {
                    (33., 50.)
                } else {
                    (16., 25.)
                },
            );
            for (label, samples) in ["edit", "completed RGBA frame"].into_iter().zip(stages) {
                report(label, level, samples, (f64::INFINITY, f64::INFINITY));
            }
        }
    }
    assert!(
        targets_met,
        "one or more latency targets missed; see measurements above"
    );
}
