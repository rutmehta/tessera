use ml_caption::Florence;
use ml_runtime::{ModelRegistry, SessionOptions};
use std::path::{Path, PathBuf};

#[test]
fn cached_caption_ocr_and_partition_report() {
    let cache = std::env::var_os("TESSERA_FLORENCE_CACHE")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tools/orchestrate/wp/M3-13/cache")
        });
    if !Florence::is_cached(&cache) {
        eprintln!("SKIP offline: tools/fetch_florence.py --cache DIR; set TESSERA_FLORENCE_CACHE");
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
    let mut model =
        Florence::load(&registry, &cache.join("florence-tokenizer.json"), options).unwrap();
    let font = fontdue::Font::from_bytes(
        include_bytes!("data/Roboto-Regular.ttf") as &[u8],
        fontdue::FontSettings::default(),
    )
    .unwrap();
    let mut image = image::RgbImage::from_pixel(768, 256, image::Rgb([255; 3]));
    let mut x = 60;
    for ch in "HELLO WORLD".chars() {
        let (m, bitmap) = font.rasterize(ch, 80.);
        for row in 0..m.height {
            for col in 0..m.width {
                let y = 160 - m.ymin - m.height as i32 + row as i32;
                image.put_pixel(
                    (x + m.xmin + col as i32) as u32,
                    y as u32,
                    image::Rgb([255 - bitmap[row * m.width + col]; 3]),
                );
            }
        }
        x += m.advance_width.round() as i32;
    }
    let result = model.ocr(&image).unwrap();
    println!("OCR: {result:?}");
    assert_eq!(
        result
            .iter()
            .map(|r| r.text.as_str())
            .collect::<Vec<_>>()
            .join(" ")
            .trim(),
        "HELLO WORLD"
    );
    assert!(
        result
            .iter()
            .all(|r| r.bbox[0] < r.bbox[2] && r.confidence > 0.)
    );
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/raw/sony-arw.ARW");
    let tmp = tempfile::tempdir().unwrap();
    let store = previews::PreviewStore::new(tmp.path(), 100_000_000).unwrap();
    let (key, _) = store.from_raw(&fixture, 768).unwrap();
    let preview = image::load_from_memory(&store.get(&key, previews::Level::Full).unwrap())
        .unwrap()
        .to_rgb8();
    let caption = model.caption(&preview).unwrap();
    println!("caption: {caption:?}");
    assert!(!caption.caption.is_empty() && !caption.alt_text.is_empty());
    for (name, report) in model.partition_reports().unwrap() {
        let mut counts = std::collections::BTreeMap::new();
        for node in report.nodes {
            *counts.entry(node.provider).or_insert(0usize) += 1;
        }
        println!("{name}: {counts:?}");
        assert!(!counts.is_empty());
    }
}
