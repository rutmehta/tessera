use image::{Rgb, RgbImage};
use ml_embed::{DIMENSION, Siglip};
use ml_runtime::{ModelRegistry, SessionOptions};
use previews::{Codec, Jpeg, Level, PreviewStore};
use std::path::{Path, PathBuf};

fn cached() -> Option<Siglip> {
    let cache = std::env::var_os("TESSERA_SIGLIP_CACHE")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tools/orchestrate/wp/M3-03/cache")
        });
    if !cache
        .join("f89d41bac7f4d4b87e010a467d93f98689d708916ed22f5a07f96fdfa26f475f.onnx")
        .exists()
        || !cache
            .join("3aa7fdbd20eaa8740cce17bf82913de641fcb632a768fed59f661cdcd0c32553.onnx")
            .exists()
        || !cache.join("tokenizer.json").exists()
    {
        eprintln!(
            "SKIP offline: run tools/fetch_siglip.py --cache <dir>, set TESSERA_SIGLIP_CACHE"
        );
        return None;
    }
    let registry = ModelRegistry::open(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../ml-runtime/models.toml"),
        &cache,
    )
    .unwrap();
    Some(
        Siglip::load(
            &registry,
            &cache.join("tokenizer.json"),
            SessionOptions::default(),
        )
        .unwrap(),
    )
}

fn cosine(a: &[f32], b: &[f32]) -> f32 {
    a.iter().zip(b).map(|(a, b)| a * b).sum()
}

#[test]
fn cached_siglip_synthetic_similarity_and_cube_retrieval() {
    let Some(mut model) = cached() else {
        return;
    };
    let block = RgbImage::from_pixel(224, 224, Rgb([255, 0, 0]));
    let gradient = RgbImage::from_fn(224, 224, |x, y| Rgb([x as u8, y as u8, 128]));
    let vectors = model
        .embed_images(&[block.clone(), gradient.clone()])
        .unwrap();
    assert_eq!(vectors[0].len(), DIMENSION);
    let single = model.embed_image(&block).unwrap();
    assert!(cosine(&single, &vectors[0]) > 0.99);
    assert!(cosine(&vectors[0], &vectors[0]) > cosine(&vectors[0], &vectors[1]) + 0.01);
    assert!(cosine(&vectors[1], &vectors[1]) > cosine(&vectors[0], &vectors[1]) + 0.01);
    let many: Vec<_> = (0..9)
        .map(|i| {
            if i % 2 == 0 {
                block.clone()
            } else {
                gradient.clone()
            }
        })
        .collect();
    let batched = model.embed_images(&many).unwrap();
    assert_eq!(batched.len(), many.len());
    for (i, vector) in batched.iter().enumerate() {
        assert!(cosine(vector, &vectors[i % 2]) > 0.99);
    }
    assert!(model.embed_images(&[]).is_err());
    assert!(model.embed_text(" ").is_err());
    let tokens = model.tokenize("A CUBE").unwrap();
    assert_eq!(tokens, model.tokenize("a cube").unwrap());
    assert_eq!(tokens.len(), 64);
    assert_eq!(*tokens.last().unwrap(), 1);
    assert_eq!(model.tokenize(&"word ".repeat(1000)).unwrap().len(), 64);
    let dir = tempfile::tempdir().unwrap();
    let previews = PreviewStore::new(dir.path(), 100_000_000).unwrap();
    let files = [
        "canon-cr3.CR3",
        "fuji-raf.RAF",
        "nikon-nef.NEF",
        "sample.dng",
        "sony-arw.ARW",
    ];
    let mut images = Vec::new();
    for file in files {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../fixtures/raw")
            .join(file);
        let (key, _) = previews.from_raw(&path, 384).unwrap();
        images.push(
            Jpeg.decode(&previews.get(&key, Level::Full).unwrap())
                .unwrap(),
        );
    }
    let embeddings = model.embed_images(&images).unwrap();
    let text = model.embed_text("a cube").unwrap();
    let mut ranked: Vec<_> = files
        .iter()
        .zip(&embeddings)
        .map(|(file, v)| (*file, cosine(&text, v)))
        .collect();
    ranked.sort_by(|a, b| b.1.total_cmp(&a.1));
    println!("cube ranking: {ranked:?}");
    assert_eq!(ranked[0].0, "sony-arw.ARW");
    let (vision, text) = model.partition_reports().unwrap();
    for (name, report) in [("vision", vision), ("text", text)] {
        let mut counts = std::collections::BTreeMap::new();
        for node in &report.nodes {
            *counts.entry(node.provider.as_str()).or_insert(0usize) += 1;
        }
        println!("{name} executed node counts: {counts:?}");
        // Opt-in hardware audit: ordinary tests still permit genuine CPU fallback.
        if std::env::var_os("TESSERA_REQUIRE_SIGLIP_COREML").is_some() {
            assert!(
                counts.get("CoreMLExecutionProvider").copied().unwrap_or(0) > 0,
                "{name} did not execute any CoreML nodes"
            );
        }
    }
    let mut catalog = index::Index::open(":memory:").unwrap();
    let raw = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/raw");
    catalog
        .scan(
            &raw,
            &index::NoopSidecarReader,
            &index::NoopMetadataProvider,
        )
        .unwrap();
    let ids = catalog.search(&index::Query::default()).unwrap();
    let mut search = ml_embed::SemanticIndex::open(model, dir.path()).unwrap();
    let mut sony = None;
    for id in ids {
        let info = catalog.image_info(id).unwrap();
        if let Some(n) = files
            .iter()
            .position(|file| info.path.file_name().unwrap() == *file)
        {
            search.insert(id, &embeddings[n]).unwrap();
            if files[n] == "sony-arw.ARW" {
                sony = Some(id);
            }
        }
    }
    let sony = sony.unwrap();
    assert_eq!(search.search_text("a cube", 5).unwrap()[0].0, sony);
    catalog.add_keyword("cube", None).unwrap();
    catalog.tag(sony, "cube").unwrap();
    let query = index::Query {
        semantic: Some("a cube".into()),
        text: Some("cube".into()),
        keyword: Some("cube".into()),
        ..Default::default()
    };
    assert_eq!(
        catalog.search_with_semantic(&query, &mut search).unwrap(),
        vec![sony]
    );
}
