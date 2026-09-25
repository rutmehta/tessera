use anyhow::Result;
use image::{Rgb, RgbImage};
use ml_faces::{Face, FaceModels};
use ml_runtime::{ModelRegistry, SessionOptions};
use std::{path::PathBuf, sync::OnceLock};

fn registry() -> Result<Option<ModelRegistry>> {
    // In-tree ignored cache: no weights are checked in. Only registry downloads.
    let cache = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(".model-cache");
    let registry = ModelRegistry::open(
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../ml-runtime/models.toml"),
        &cache,
    )?;
    static READY: OnceLock<bool> = OnceLock::new();
    let ready = *READY.get_or_init(|| {
        for id in ["opencv/yunet", "opencv/sface"] {
            if let Err(error) = registry.resolve(id) {
                // Skip only transport failures; hash corruption and filesystem errors fail.
                if error.downcast_ref::<ureq::Error>().is_some_and(|e| {
                    matches!(
                        e,
                        ureq::Error::HostNotFound
                            | ureq::Error::ConnectionFailed
                            | ureq::Error::Timeout(_)
                            | ureq::Error::Io(_)
                    )
                }) {
                    assert!(
                        std::env::var_os("TESSERA_REQUIRE_MODELS").is_none(),
                        "models required: {error:#}"
                    );
                    eprintln!("SKIP offline model tests: {error:#}");
                    return false;
                }
                panic!("model resolution failed: {error:#}");
            }
        }
        true
    });
    Ok(ready.then_some(registry))
}

fn pattern() -> (RgbImage, Face) {
    let image = RgbImage::from_fn(112, 112, |x, y| {
        let (x, y) = (x as i32, y as i32);
        let eye = [(38, 52), (74, 52)]
            .iter()
            .any(|&(ex, ey)| (x - ex).pow(2) + (y - ey).pow(2) < 16);
        let mouth = (40..73).contains(&x) && (90..94).contains(&y);
        if eye || mouth {
            Rgb([30; 3])
        } else if (x - 56).pow(2) * 2 + (y - 65).pow(2) < 3500 {
            Rgb([210, 170, 140])
        } else {
            Rgb([60, 100, 140])
        }
    });
    (
        image,
        Face {
            bbox: [8., 5., 96., 106.],
            landmarks5: [
                [38.2946, 51.6963],
                [73.5318, 51.5014],
                [56.0252, 71.7366],
                [41.5493, 92.3655],
                [70.7299, 92.2041],
            ],
            score: 1.,
        },
    )
}
#[test]
fn generated_pattern_shapes_and_embeddings() -> Result<()> {
    let Some(registry) = registry()? else {
        return Ok(());
    };
    let mut models = FaceModels::load(&registry, SessionOptions::cpu())?;
    let (image, face) = pattern();
    for found in models.detect(&image)? {
        assert!(found.bbox.iter().all(|v| v.is_finite()));
        assert!((0.0..=1.0).contains(&found.score));
    }
    let embedding = models.embed(&image, &face)?;
    assert_eq!(embedding.len(), 128);
    assert!(embedding.iter().all(|v| v.is_finite()));
    assert!((embedding.iter().map(|v| v * v).sum::<f32>() - 1.).abs() < 1e-5);
    assert!(models.detect(&RgbImage::new(0, 0)).is_err());
    Ok(())
}
#[test]
fn raw_previews_write_scores_without_assuming_people() -> Result<()> {
    use previews::{Codec, Jpeg};
    let Some(registry) = registry()? else {
        return Ok(());
    };
    let mut models = FaceModels::load(&registry, SessionOptions::cpu())?;
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/raw");
    let temp = tempfile::tempdir_in(env!("CARGO_MANIFEST_DIR"))?;
    let mut index = index::Index::open(temp.path().join("index.sqlite"))?;
    index.scan(
        &root,
        &index::NoopSidecarReader,
        &index::NoopMetadataProvider,
    )?;
    let ids = index.search(&index::Query::default())?;
    assert_eq!(ids.len(), 5);
    let mut processed = 0;
    for id in ids {
        let mut source = raw_decode::RawSource::open(index.image_info(id)?.path)?;
        let Some(jpeg) = source.embedded_preview() else {
            continue;
        };
        let image = Jpeg.decode(&jpeg)?;
        let records = models.analyze_and_store(&index, id, &image)?;
        assert_eq!(index.faces(id)?, records);
        assert!(
            index
                .scores(id)?
                .iter()
                .any(|s| s.signal == "faces_analyzed" && s.value == 1.)
        );
        ml_quality::analyze_and_store(&index, id, &image)?;
        assert!(index.scores(id)?.iter().any(|s| s.signal == "sharpness"));
        processed += 1;
    }
    assert!(processed > 0, "no fixture preview was exercised");
    println!("processed {processed} RAW embedded previews");
    Ok(())
}
#[cfg(target_os = "macos")]
#[test]
fn both_models_report_real_coreml_partitions() -> Result<()> {
    let Some(registry) = registry()? else {
        return Ok(());
    };
    for id in ["opencv/yunet", "opencv/sface"] {
        let handle = registry.resolve(id)?;
        let mut session = ml_runtime::Session::load(handle.path(), SessionOptions::default())?;
        assert!(
            session.fallback_reason.is_none(),
            "{id}: {:?}",
            session.fallback_reason
        );
        session.probe(&handle.spec().inputs)?;
        let report = session.partition_report()?;
        assert!(!report.nodes.is_empty());
        assert!(
            report
                .nodes
                .iter()
                .any(|n| n.provider == "CoreMLExecutionProvider"),
            "{id}: no CoreML partition: {report:?}"
        );
        let coreml = report
            .nodes
            .iter()
            .filter(|n| n.provider == "CoreMLExecutionProvider")
            .count();
        println!(
            "{id}: {coreml}/{} executed partitions CoreML, remainder CPU (not claiming full offload)",
            report.nodes.len()
        );
    }
    Ok(())
}
