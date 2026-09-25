use engine_api::{EngineError, jobs::CancellationToken, recipe::Recipe};
use export::*;
use pipeline_cpu::{Image, RenderSource};
use std::{cell::RefCell, fs};

fn fixtures() -> Vec<Image> {
    (0..5)
        .map(|i| {
            Image::new(
                1536,
                1024,
                vec![vec![0.1 + i as f32 * 0.05; 1536 * 1024]; 3],
            )
            .unwrap()
        })
        .collect()
}
fn items<'a>(images: &'a [Image], recipe: &'a Recipe) -> Vec<ExportItem<'a>> {
    images
        .iter()
        .enumerate()
        .map(|(i, image)| ExportItem {
            image: ExportImage {
                source: RenderSource::Rgb(image),
                name: "fixture",
                sequence: i + 1,
                date: "20260925",
                metadata: None,
            },
            recipe,
        })
        .collect()
}
#[test]
fn five_images_progress_cancel_and_resume() {
    let images = fixtures();
    let recipe = Recipe::default();
    let items = items(&images, &recipe);
    let dir = tempfile::tempdir().unwrap();
    let settings = ExportSettings {
        resize: Resize::LongEdge(1024),
        output_dir: dir.path().into(),
        ..Default::default()
    };
    let events = RefCell::new(Vec::new());
    let report = export_batch(
        &items,
        &settings,
        |p| events.borrow_mut().push(p),
        &CancellationToken::new(),
    )
    .unwrap();
    assert_eq!(events.borrow().len(), 5);
    assert_eq!(
        events
            .borrow()
            .iter()
            .map(|p| p.completed)
            .collect::<Vec<_>>(),
        vec![1, 2, 3, 4, 5]
    );
    assert!(report.remaining().is_empty());
    for path in report.results.iter().map(|r| r.as_ref().unwrap()) {
        assert_eq!(image::open(path).unwrap().width(), 1024);
    }
    let dir = tempfile::tempdir().unwrap();
    let settings = ExportSettings {
        output_dir: dir.path().into(),
        ..settings
    };
    let cancel = CancellationToken::new();
    let report = export_batch(&items, &settings, |_| cancel.cancel(), &cancel).unwrap();
    assert_eq!(report.results.iter().filter(|r| r.is_ok()).count(), 1);
    assert_eq!(report.remaining().len(), 4);
    assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 2);
    for entry in fs::read_dir(dir.path()).unwrap() {
        let path = entry.unwrap().path();
        assert!(matches!(
            path.extension().and_then(|s| s.to_str()),
            Some("jpg" | "xmp")
        ));
    }
    let remaining = report.remaining();
    let retry: Vec<_> = items
        .into_iter()
        .enumerate()
        .filter_map(|(i, item)| remaining.contains(&i).then_some(item))
        .collect();
    let resumed = export_batch(&retry, &settings, |_| {}, &CancellationToken::new()).unwrap();
    assert!(resumed.results.iter().all(Result::is_ok));
    assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 10);
}
#[test]
fn precancel_and_duplicate_names_publish_nothing() {
    let image = Image::new(16, 16, vec![vec![0.2; 256]; 3]).unwrap();
    let recipe = Recipe::default();
    let items = items(std::slice::from_ref(&image), &recipe);
    let dir = tempfile::tempdir().unwrap();
    let settings = ExportSettings {
        output_dir: dir.path().into(),
        ..Default::default()
    };
    let cancel = CancellationToken::new();
    cancel.cancel();
    let report =
        export_batch(&items, &settings, |_| panic!("cancelled progress"), &cancel).unwrap();
    assert!(matches!(report.results[0], Err(EngineError::Cancelled)));
    assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 0);
    let images = vec![image.clone(), image];
    let items = self::items(&images, &recipe);
    let settings = ExportSettings {
        naming: "same".into(),
        ..settings
    };
    assert!(export_batch(&items, &settings, |_| {}, &CancellationToken::new()).is_err());
    assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 0);
}
#[test]
#[ignore = "full-size five-image JPEG q90 benchmark"]
fn bench_five_full_size_jpegs() {
    let images: Vec<_> = (0..5)
        .map(|i| Image::new(3000, 2000, vec![vec![0.1 + i as f32 * 0.05; 6_000_000]; 3]).unwrap())
        .collect();
    let recipe = Recipe::default();
    let items = items(&images, &recipe);
    let dir = tempfile::tempdir().unwrap();
    let settings = ExportSettings {
        output_dir: dir.path().into(),
        ..Default::default()
    };
    let start = std::time::Instant::now();
    let report = export_batch(
        &items,
        &settings,
        |p| {
            println!(
                "image {} completed at {:.1} ms",
                p.index,
                start.elapsed().as_secs_f64() * 1000.0
            )
        },
        &CancellationToken::new(),
    )
    .unwrap();
    assert!(report.results.iter().all(Result::is_ok));
    println!(
        "five 3000x2000 RGB fixtures, full-size JPEG q90: {:.1} ms/image (batch wall time / 5)",
        start.elapsed().as_secs_f64() * 1000.0 / 5.0
    );
}
