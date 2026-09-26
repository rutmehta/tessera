use ml_caption::{Calibration, KeywordModel};
use ml_runtime::{ModelRegistry, SessionOptions};
use std::path::{Path, PathBuf};
#[test]
fn cached_red_disc_ranks_circle_and_red_in_top_five() {
    let cache = std::env::var_os("TESSERA_SIGLIP_CACHE")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tools/orchestrate/wp/M3-13/cache")
        });
    if ![
        "f89d41bac7f4d4b87e010a467d93f98689d708916ed22f5a07f96fdfa26f475f.onnx",
        "3aa7fdbd20eaa8740cce17bf82913de641fcb632a768fed59f661cdcd0c32553.onnx",
        "tokenizer.json",
    ]
    .iter()
    .all(|p| cache.join(p).is_file())
    {
        eprintln!("SKIP offline: tools/fetch_siglip.py --cache DIR; set TESSERA_SIGLIP_CACHE");
        return;
    }
    let registry = ModelRegistry::open(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../ml-runtime/models.toml"),
        &cache,
    )
    .unwrap();
    let options = if std::env::var_os("TESSERA_CAPTION_COREML").is_some() {
        SessionOptions::default()
    } else {
        SessionOptions::cpu()
    };
    let model = ml_embed::Siglip::load(&registry, &cache.join("tokenizer.json"), options).unwrap();
    let labels = [
        "circle", "red", "blue", "green", "square", "triangle", "dog", "forest", "car", "mountain",
    ]
    .map(str::to_owned)
    .to_vec();
    let mut model = KeywordModel::new(model, labels, Calibration::default()).unwrap();
    let image = image::RgbImage::from_fn(224, 224, |x, y| {
        if (x as i32 - 112).pow(2) + (y as i32 - 112).pow(2) < 80 * 80 {
            image::Rgb([255, 0, 0])
        } else {
            image::Rgb([255; 3])
        }
    });
    let ranked = model.suggest_keywords(&image).unwrap();
    println!("red disc: {ranked:?}");
    let top = ranked
        .iter()
        .take(5)
        .map(|v| v.0.as_str())
        .collect::<Vec<_>>();
    assert!(top.contains(&"red") && top.contains(&"circle"));
    for (name, report) in [
        ("vision", model.partition_reports().unwrap().0),
        ("text", model.partition_reports().unwrap().1),
    ] {
        let mut counts = std::collections::BTreeMap::new();
        for node in report.nodes {
            *counts.entry(node.provider).or_insert(0usize) += 1;
        }
        println!("{name}: {counts:?}");
        assert!(!counts.is_empty());
    }
}
#[test]
fn bundled_vocabulary_is_curated_unique_and_photographic() {
    let labels = ml_caption::vocabulary();
    assert!((1800..=2200).contains(&labels.len()));
    let unique: std::collections::HashSet<_> = labels.iter().collect();
    assert_eq!(labels.len(), unique.len());
    for label in ["red", "circle", "portrait", "sunset"] {
        assert!(labels.iter().any(|v| v == label), "missing {label}");
    }
}
