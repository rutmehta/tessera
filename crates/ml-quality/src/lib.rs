//! Deterministic, preview-scale image-quality measurements.

#[derive(Debug, Clone, Default)]
pub struct QualityScores {
    pub sharpness: f64,
    pub noise: f64,
    pub motion_blur: f64,
    pub exposure: [f64; 3],
    pub shadow_clipping: [f64; 3],
    pub highlight_clipping: [f64; 3],
}

pub fn analyze(image: &image::RgbImage) -> engine_api::EngineResult<QualityScores> {
    if image.width() == 0 || image.height() == 0 {
        return Err(engine_api::EngineError::invalid(
            "image",
            "preview must be nonempty",
        ));
    }
    let resized;
    let image = if image.width().max(image.height()) > 1024 {
        let scale = 1024.0 / f64::from(image.width().max(image.height()));
        let w = (f64::from(image.width()) * scale).round().max(1.0) as u32;
        let h = (f64::from(image.height()) * scale).round().max(1.0) as u32;
        resized = image::imageops::resize(image, w, h, image::imageops::FilterType::Triangle);
        &resized
    } else {
        image
    };
    let (w, h) = image.dimensions();
    let gray: Vec<f64> = image
        .pixels()
        .map(|p| {
            (0.2126 * f64::from(p[0]) + 0.7152 * f64::from(p[1]) + 0.0722 * f64::from(p[2])) / 255.0
        })
        .collect();
    let at = |x: u32, y: u32| gray[(y * w + x) as usize];
    let mut sum = 0.0;
    let mut squares = 0.0;
    let mut n = 0.0;
    let mut residuals = Vec::new();
    let (mut xx, mut yy, mut xy) = (0.0_f64, 0.0_f64, 0.0_f64);
    for y in 1..h.saturating_sub(1) {
        for x in 1..w.saturating_sub(1) {
            let lap = at(x - 1, y) + at(x + 1, y) + at(x, y - 1) + at(x, y + 1) - 4.0 * at(x, y);
            sum += lap;
            let (mut lo, mut hi) = (1.0_f64, 0.0_f64);
            for py in y - 1..=y + 1 {
                for px in x - 1..=x + 1 {
                    lo = lo.min(at(px, py));
                    hi = hi.max(at(px, py));
                }
            }
            if hi - lo <= 0.1 {
                residuals.push(lap / 20.0_f64.sqrt());
            }
            let gx = (at(x + 1, y) - at(x - 1, y)) / 2.0;
            let gy = (at(x, y + 1) - at(x, y - 1)) / 2.0;
            xx += gx * gx;
            yy += gy * gy;
            xy += gx * gy;
            squares += lap * lap;
            n += 1.0;
        }
    }
    let variance = if n > 0.0 {
        (squares / n - (sum / n).powi(2)).max(0.0)
    } else {
        0.0
    };
    let (mut exposure, mut shadow_clipping, mut highlight_clipping) =
        ([0.0; 3], [0.0; 3], [0.0; 3]);
    let count = f64::from(w) * f64::from(h);
    for p in image.pixels() {
        for c in 0..3 {
            exposure[c] += f64::from(p[c]);
            shadow_clipping[c] += if p[c] == 0 { 1.0 } else { 0.0 };
            highlight_clipping[c] += if p[c] == 255 { 1.0 } else { 0.0 };
        }
    }
    let center = median(&mut residuals);
    for c in 0..3 {
        exposure[c] /= 255.0 * count;
        shadow_clipping[c] /= count;
        highlight_clipping[c] /= count;
    }
    for value in &mut residuals {
        *value = (*value - center).abs();
    }
    let sigma = 1.4826 * median(&mut residuals);
    Ok(QualityScores {
        sharpness: variance / (variance + 0.01),
        noise: (sigma / 0.1).clamp(0.0, 1.0),
        exposure,
        shadow_clipping,
        highlight_clipping,
        motion_blur: if xx + yy > 0.0 {
            ((xx - yy).hypot(2.0 * xy) / (xx + yy)).clamp(0.0, 1.0)
        } else {
            0.0
        },
    })
}

pub fn analyze_and_store(
    index: &index::Index,
    id: engine_api::id::ImageId,
    preview: &image::RgbImage,
) -> engine_api::EngineResult<QualityScores> {
    index.image_info(id)?;
    let scores = analyze(preview)?;
    let clipping = scores
        .shadow_clipping
        .iter()
        .chain(scores.highlight_clipping.iter())
        .sum::<f64>()
        / 3.0;
    let quality = scores.sharpness
        * (1.0 - 0.25 * scores.motion_blur)
        * (1.0 - 0.5 * scores.noise)
        * (1.0 - clipping);
    let mut rows = vec![
        ("sharpness".to_owned(), scores.sharpness),
        ("motion_blur".to_owned(), scores.motion_blur),
        ("noise".to_owned(), scores.noise),
    ];
    for (channel, name) in ["r", "g", "b"].iter().enumerate() {
        rows.push((format!("exposure_{name}"), scores.exposure[channel]));
        rows.push((
            format!("shadow_clipping_{name}"),
            scores.shadow_clipping[channel],
        ));
        rows.push((
            format!("highlight_clipping_{name}"),
            scores.highlight_clipping[channel],
        ));
    }
    // Aggregate last: readers never observe a new aggregate before its components.
    rows.push(("quality".to_owned(), quality.clamp(0.0, 1.0)));
    for (signal, value) in rows {
        index.set_score(
            id,
            &index::Score {
                signal,
                value,
                model: "classical-quality-v1".into(),
            },
        )?;
    }
    Ok(scores)
}

#[derive(Default)]
pub struct QualityScorer(std::collections::HashMap<engine_api::id::ImageId, f64>);

impl QualityScorer {
    pub fn from_index(
        index: &index::Index,
        ids: &[engine_api::id::ImageId],
    ) -> engine_api::EngineResult<Self> {
        let mut values = std::collections::HashMap::new();
        for &id in ids {
            if let Some(score) = index
                .scores(id)?
                .into_iter()
                .find(|s| s.signal == "quality")
            {
                values.insert(id, score.value);
            }
        }
        Ok(Self(values))
    }
}

impl cull::Scorer for QualityScorer {
    fn score(&self, image: &index::ImageInfo) -> f64 {
        self.0.get(&image.id).copied().unwrap_or(0.0)
    }
}

fn median(values: &mut [f64]) -> f64 {
    if values.is_empty() {
        return 0.0;
    }
    values.sort_unstable_by(f64::total_cmp);
    let middle = values.len() / 2;
    if values.len().is_multiple_of(2) {
        (values[middle - 1] + values[middle]) / 2.0
    } else {
        values[middle]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{Rgb, RgbImage};

    fn checker(size: u32) -> RgbImage {
        RgbImage::from_fn(size, size, |x, y| {
            Rgb([if (x / 4 + y / 4) % 2 == 0 { 32 } else { 224 }; 3])
        })
    }

    #[test]
    fn scorer_uses_persisted_quality_not_file_size() {
        use cull::Scorer;
        let dir = tempfile::tempdir_in(env!("CARGO_MANIFEST_DIR")).unwrap();
        std::fs::write(dir.path().join("a.jpg"), b"synthetic").unwrap();
        std::fs::write(dir.path().join("b.jpg"), b"synthetic bigger").unwrap();
        let mut index = index::Index::open(dir.path().join("db.sqlite")).unwrap();
        index
            .scan(
                dir.path(),
                &index::NoopSidecarReader,
                &index::NoopMetadataProvider,
            )
            .unwrap();
        let ids = index.search(&index::Query::default()).unwrap();
        let image = checker(64);
        analyze_and_store(&index, ids[0], &image).unwrap();
        analyze_and_store(&index, ids[1], &image::imageops::blur(&image, 2.0)).unwrap();
        let scorer = QualityScorer::from_index(&index, &ids).unwrap();
        assert!(
            scorer.score(&index.image_info(ids[0]).unwrap())
                > scorer.score(&index.image_info(ids[1]).unwrap())
        );
        let absent = index::ImageInfo {
            id: engine_api::id::ImageId(999),
            path: "missing".into(),
            size: u64::MAX,
            capture_seconds: None,
        };
        assert_eq!(scorer.score(&absent), 0.0);
    }

    #[test]
    fn catalog_scores_roundtrip_and_replace() {
        let dir = tempfile::tempdir_in(env!("CARGO_MANIFEST_DIR")).unwrap();
        std::fs::write(dir.path().join("a.jpg"), b"synthetic").unwrap();
        let db = dir.path().join("index.sqlite");
        let mut index = index::Index::open(&db).unwrap();
        index
            .scan(
                dir.path(),
                &index::NoopSidecarReader,
                &index::NoopMetadataProvider,
            )
            .unwrap();
        let id = index.search(&index::Query::default()).unwrap()[0];
        let image = checker(64);
        analyze_and_store(&index, id, &image).unwrap();
        let rows = index.scores(id).unwrap();
        assert_eq!(rows.len(), 13);
        assert!(
            rows.iter()
                .all(|s| s.model == "classical-quality-v1" && (0.0..=1.0).contains(&s.value))
        );
        let sharp = rows.iter().find(|s| s.signal == "quality").unwrap().value;
        analyze_and_store(&index, id, &image::imageops::blur(&image, 2.0)).unwrap();
        drop(index);
        let index = index::Index::open(db).unwrap();
        let rows = index.scores(id).unwrap();
        assert_eq!(rows.len(), 13);
        assert!(rows.iter().find(|s| s.signal == "quality").unwrap().value < sharp);
        assert!(analyze_and_store(&index, engine_api::id::ImageId(0), &image).is_err());
    }

    #[test]
    fn rejects_empty_previews() {
        assert!(analyze(&RgbImage::new(0, 4)).is_err());
        assert!(analyze(&RgbImage::new(4, 0)).is_err());
    }

    #[test]
    fn exposure_is_bounded_for_tiny_and_solid_images() {
        for (w, h) in [(1, 1), (2, 2), (7, 7), (1, 4096)] {
            for value in [0, 128, 255] {
                let scores = analyze(&RgbImage::from_pixel(w, h, Rgb([value; 3]))).unwrap();
                for v in scores
                    .exposure
                    .into_iter()
                    .chain(scores.shadow_clipping)
                    .chain(scores.highlight_clipping)
                {
                    assert!((0.0..=1.0).contains(&v), "{w}x{h}: {v}");
                }
                assert_eq!(scores.sharpness, 0.0);
                assert_eq!(scores.noise, 0.0);
                assert_eq!(scores.motion_blur, 0.0);
            }
        }
    }

    #[test]
    fn flat_region_noise_increases_with_amplitude() {
        let mut seed = 17u32;
        let low = RgbImage::from_fn(128, 128, |_, _| {
            seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
            Rgb([(128i16 + ((seed >> 24) % 9) as i16 - 4) as u8; 3])
        });
        let high = RgbImage::from_fn(128, 128, |x, y| {
            Rgb([(128i16 + (i16::from(low.get_pixel(x, y)[0]) - 128) * 3) as u8; 3])
        });
        let a = analyze(&low).unwrap().noise;
        let b = analyze(&high).unwrap().noise;
        assert!(a > 0.0 && b > a && b <= 1.0, "low={a} high={b}");
        assert_eq!(
            analyze(&RgbImage::from_pixel(32, 32, Rgb([128; 3])))
                .unwrap()
                .noise,
            0.0
        );
        let edge = RgbImage::from_fn(64, 64, |x, _| Rgb([if x < 32 { 32 } else { 224 }; 3]));
        assert_eq!(analyze(&edge).unwrap().noise, 0.0);
    }

    #[test]
    fn exposure_counts_each_channel_independently() {
        let img = RgbImage::from_fn(4, 4, |x, _| {
            if x < 2 {
                Rgb([0, 128, 255])
            } else {
                Rgb([128, 255, 128])
            }
        });
        let s = analyze(&img).unwrap();
        assert_eq!(s.shadow_clipping, [0.5, 0.0, 0.0]);
        assert_eq!(s.highlight_clipping, [0.0, 0.5, 0.5]);
        assert!((s.exposure[0] - 64.0 / 255.0).abs() < 1e-12);
        assert!((s.exposure[1] - 191.5 / 255.0).abs() < 1e-12);
        let normal = analyze(&RgbImage::from_pixel(4, 4, Rgb([128; 3]))).unwrap();
        assert_eq!(normal.highlight_clipping, [0.0; 3]);
        assert_eq!(normal.shadow_clipping, [0.0; 3]);
    }

    #[test]
    fn anisotropy_detects_directional_structure() {
        let stripes =
            RgbImage::from_fn(128, 128, |x, _| Rgb([if x % 8 < 4 { 32 } else { 224 }; 3]));
        let diagonal = RgbImage::from_fn(128, 128, |x, y| {
            Rgb([if (x + y) % 8 < 4 { 32 } else { 224 }; 3])
        });
        assert!(analyze(&stripes).unwrap().motion_blur > 0.99);
        assert!(analyze(&diagonal).unwrap().motion_blur > 0.99);
        assert!(analyze(&checker(128)).unwrap().motion_blur < 0.01);
        assert_eq!(
            analyze(&RgbImage::from_pixel(4, 4, Rgb([128; 3])))
                .unwrap()
                .motion_blur,
            0.0
        );
    }

    #[test]
    fn preview_is_capped_at_1024_without_upscaling() {
        let large = RgbImage::from_fn(2048, 1024, |x, y| {
            Rgb([if (x / 4 + y / 4) % 2 == 0 { 32 } else { 224 }; 3])
        });
        let expected =
            image::imageops::resize(&large, 1024, 512, image::imageops::FilterType::Triangle);
        assert_eq!(
            analyze(&large).unwrap().sharpness,
            analyze(&expected).unwrap().sharpness
        );
    }

    #[test]
    fn laplacian_separates_sharp_from_blurred() {
        let sharp = checker(128);
        let blurred = image::imageops::blur(&sharp, 2.0);
        let a = analyze(&sharp).unwrap().sharpness;
        let b = analyze(&blurred).unwrap().sharpness;
        assert!(a > b && b > 0.0, "sharp={a}, blurred={b}");
        assert!(a <= 1.0);
        assert_eq!(
            analyze(&RgbImage::from_pixel(32, 32, Rgb([128; 3])))
                .unwrap()
                .sharpness,
            0.0
        );
    }
}
