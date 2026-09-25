use image::{Rgb, RgbImage};
use ml_runtime::{ModelRegistry, SessionOptions};
use ml_segment::{MaskStore, Segmenter};
use std::path::PathBuf;

#[test]
fn cached_models_segment_synthetic_and_fixture() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let cache = std::env::var_os("TESSERA_SEGMENT_MODELS")
        .map(PathBuf::from)
        .unwrap_or_else(|| root.join("tools/orchestrate/wp/M3-04/.cache/segment-registry"));
    if !cache.is_dir() {
        eprintln!("SKIP offline: fetch segmentation weights then populate SHA cache (README)");
        return;
    }
    let registry = ModelRegistry::open(root.join("crates/ml-runtime/models.toml"), &cache).unwrap();
    for spec in registry
        .models()
        .iter()
        .filter(|m| m.id.starts_with("segment/"))
    {
        assert!(
            cache.join(format!("{}.onnx", spec.sha256)).is_file(),
            "partial model cache must not download in tests"
        );
    }
    let dir = tempfile::tempdir().unwrap();
    let mut model = Segmenter::load(
        &registry,
        SessionOptions::default(),
        MaskStore::new(dir.path(), 32 * 1024 * 1024).unwrap(),
    )
    .unwrap();
    let image = RgbImage::from_fn(128, 96, |x, y| {
        Rgb(
            [if (x as i32 - 64).pow(2) + (y as i32 - 48).pow(2) < 28 * 28 {
                240
            } else {
                10
            }; 3],
        )
    });
    let subject = model.subject(&image, 0).unwrap();
    let (mut intersection, mut union) = (0, 0);
    for (p, &v) in image.pixels().zip(subject.data()) {
        let truth = p[0] > 100;
        let mask = v > 0.5;
        intersection += usize::from(truth && mask);
        union += usize::from(truth || mask);
    }
    let iou = intersection as f32 / union as f32;
    println!("subject disc IoU {iou}");
    assert!(iou > 0.8);
    let background = model.background(&image, 0).unwrap();
    assert!(
        subject
            .data()
            .iter()
            .zip(background.data())
            .all(|(a, b)| (a + b - 1.).abs() < 1e-6)
    );
    let sky_image = RgbImage::from_fn(128, 96, |_, y| {
        if y < 48 {
            Rgb([60 + y as u8, 120 + y as u8, 230])
        } else {
            Rgb([60, 70, 30])
        }
    });
    let sky = model.sky(&sky_image, 0).unwrap();
    let top = sky.data()[..128 * 40].iter().sum::<f32>() / (128. * 40.);
    let bottom = sky.data()[128 * 56..].iter().sum::<f32>() / (128. * 40.);
    println!("sky top={top} bottom={bottom}");
    assert!(top > 0.5 && bottom < 0.1);
    let prompts = ml_segment::Prompts {
        clicks: vec![ml_segment::Click {
            point: [0.5, 0.5],
            positive: true,
        }],
        boxes: vec![],
    };
    let mask = model.promptable(&image, &prompts, 1).unwrap();
    assert_eq!((mask.width(), mask.height()), (64, 48));
    assert!(
        mask.data()[24 * 64 + 32] > 0.5,
        "positive click must select disc"
    );
    assert!(mask.data()[0] < 0.5, "corner must remain background");
    assert_eq!(mask, model.promptable(&image, &prompts, 1).unwrap());
    let mut negative_prompt = prompts.clone();
    negative_prompt.clicks.push(ml_segment::Click {
        point: [0.05, 0.05],
        positive: false,
    });
    let negative = model.promptable(&image, &negative_prompt, 0).unwrap();
    assert!(negative.data()[48 * 128 + 64] > 0.5);
    assert!(negative.data()[6 * 128 + 6] < 0.5);
    let box_prompt = ml_segment::Prompts {
        clicks: vec![],
        boxes: vec![[0.25, 0.15, 0.75, 0.85]],
    };
    let boxed = model.promptable(&image, &box_prompt, 0).unwrap();
    assert!(boxed.data()[48 * 128 + 64] > 0.5 && boxed.data()[0] < 0.5);
    let duplicate_boxes = ml_segment::Prompts {
        clicks: vec![],
        boxes: vec![box_prompt.boxes[0]; 2],
    };
    let union = model.promptable(&image, &duplicate_boxes, 0).unwrap();
    assert!(
        union
            .data()
            .iter()
            .zip(boxed.data())
            .all(|(a, b)| (a - b).abs() < 1e-5)
    );
    model.person(&image, [50., 30., 28., 28.], 0).unwrap();
    let fixture = root.join("fixtures/raw/sony-arw.ARW");
    let previews =
        previews::PreviewStore::new(dir.path().join("previews"), 32 * 1024 * 1024).unwrap();
    let (key, _) = previews.from_raw(&fixture, 384).unwrap();
    let bytes = previews.get(&key, previews::Level::Full).unwrap();
    let fixture_image = image::load_from_memory(&bytes).unwrap().to_rgb8();
    let fixture_mask = model.subject(&fixture_image, 3).unwrap();
    assert_eq!(fixture_mask.width(), (fixture_image.width() >> 3).max(1));
    assert!(fixture_mask.data().iter().all(|v| v.is_finite()));
    println!(
        "fixture sony-arw: {}x{} mask",
        fixture_mask.width(),
        fixture_mask.height()
    );
    let reports = model.partition_reports().unwrap();
    for (name, report) in &reports {
        let n = report
            .nodes
            .iter()
            .filter(|n| n.provider == "CoreMLExecutionProvider")
            .count();
        println!(
            "{name}: {n} CoreML nodes / {} executed nodes",
            report.nodes.len()
        );
    }
    if cfg!(target_os = "macos") {
        assert!(reports.iter().any(|(_, r)| {
            r.nodes
                .iter()
                .any(|n| n.provider == "CoreMLExecutionProvider")
        }));
    }
}
