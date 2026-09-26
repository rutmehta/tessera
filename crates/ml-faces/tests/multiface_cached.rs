//! Generated artwork + explicit landmarks exercises SFace, not detector recall.
use anyhow::Result;
use image::{Rgb, RgbImage};
use ml_faces::{Face, FaceModels, QualityGate, cluster_with_medoids};
use ml_runtime::{ModelRegistry, SessionOptions};
use std::path::PathBuf;

#[test]
fn generated_multiface_cached_models() -> Result<()> {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let cache = std::env::var_os("TESSERA_FACE_MODEL_CACHE")
        .map(PathBuf::from)
        .unwrap_or_else(|| root.join(".model-cache"));
    let registry = ModelRegistry::open(root.join("../ml-runtime/models.toml"), &cache)?;
    for id in ["opencv/yunet", "opencv/sface"] {
        let spec = registry.models().iter().find(|s| s.id == id).unwrap();
        if !cache.join(format!("{}.onnx", spec.sha256)).is_file() {
            eprintln!(
                "SKIP generated multi-face integration: {id} not cached at {} (no download)",
                cache.display()
            );
            assert!(
                std::env::var_os("TESSERA_REQUIRE_MODELS").is_none(),
                "cached models required"
            );
            return Ok(());
        }
    }
    // Loading re-verifies cache hashes; corruption/runtime failures must fail.
    let mut models = FaceModels::load(&registry, SessionOptions::cpu())?;
    let image = RgbImage::from_fn(224, 112, |x, y| {
        let (x, y) = ((x % 112) as i32, y as i32);
        let eye = [(38, 52), (74, 52)]
            .iter()
            .any(|&(ex, ey)| (x - ex).pow(2) + (y - ey).pow(2) < 16);
        if eye || ((40..73).contains(&x) && (90..94).contains(&y)) {
            Rgb([30; 3])
        } else if (x - 56).pow(2) * 2 + (y - 65).pow(2) < 3500 {
            Rgb([210, 170, 140])
        } else {
            Rgb([60, 100, 140])
        }
    });
    let face = Face {
        bbox: [8., 5., 96., 106.],
        landmarks5: [
            [38.2946, 51.6963],
            [73.5318, 51.5014],
            [56.0252, 71.7366],
            [41.5493, 92.3655],
            [70.7299, 92.2041],
        ],
        score: 1.,
    };
    let mut second = face.clone();
    second.bbox[0] += 112.;
    for point in &mut second.landmarks5 {
        point[0] += 112.;
    }
    let mut poor = face.clone();
    poor.score = 0.1;
    let embeddings = models.embed_eligible_faces(
        &image,
        &[face, second, poor],
        QualityGate {
            min_sharpness: 0.,
            ..QualityGate::default()
        },
    )?;
    assert_eq!(embeddings.len(), 3);
    assert!(embeddings[2].is_none());
    let descriptors = [embeddings[0].unwrap(), embeddings[1].unwrap()];
    for row in descriptors {
        assert!(row.iter().all(|v| v.is_finite()));
        assert!((row.iter().map(|v| v * v).sum::<f32>() - 1.).abs() < 1e-5);
    }
    let result = cluster_with_medoids(&descriptors, 0.95)?;
    assert_eq!(result.clusters.len(), 1);
    assert_eq!(result.clusters[0].members, vec![0, 1]);
    assert!(result.clusters[0].eligible);
    // Run the real detector too, but cartoons are not a face-recall dataset.
    for found in models.detect(&image)? {
        assert!(found.bbox.iter().all(|v| v.is_finite()));
    }
    println!(
        "cached YuNet + SFace: two generated face crops embedded and clustered; low-confidence crop excluded"
    );
    Ok(())
}
