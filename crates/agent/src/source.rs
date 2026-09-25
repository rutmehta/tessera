use anyhow::Result;
use engine_api::{recipe::DevelopSettings, tools::FaceScore};
use serde_json::Value;
use std::path::Path;
use style_profile::{Features, features::SceneStats};

pub fn features(path: &Path, faces: &[FaceScore], description: &Value) -> Result<Features> {
    let ext = path
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    let (pixels, w, h) = if matches!(ext.as_str(), "jpg" | "jpeg" | "png" | "tif" | "tiff") {
        let rgb = image::open(path)?
            .resize(512, 512, image::imageops::FilterType::Triangle)
            .to_rgb8();
        (
            rgb.pixels()
                .map(|p| p.0.map(|v| crate::metrics::linear(v) as f32))
                .collect::<Vec<_>>(),
            rgb.width(),
            rgb.height(),
        )
    } else {
        let mut raw = raw_decode::RawSource::open(path)?;
        let cfa = raw.decode_cfa()?;
        let metadata = raw.metadata();
        let linear = pipeline_cpu::render_linear_scaled(
            &DevelopSettings::default(),
            &pipeline_cpu::RenderSource::Cfa {
                image: &cfa,
                metadata: &metadata,
            },
            16,
        )?;
        let p = linear.planes();
        let pixels = (0..p[0].len())
            .map(|i| {
                let [r, g, b] = [p[0][i], p[1][i], p[2][i]];
                [
                    (1.6605 * r - 0.5876 * g - 0.0728 * b).max(0.),
                    (-0.1246 * r + 1.1329 * g - 0.0083 * b).max(0.),
                    (-0.0182 * r - 0.1006 * g + 1.1187 * b).max(0.),
                ]
            })
            .collect();
        (pixels, linear.width(), linear.height())
    };
    let stats = SceneStats::from_linear_rgb(&pixels)?;
    let mut covered = vec![false; pixels.len()];
    for face in faces {
        let r = face.region;
        if !r.is_valid() {
            continue;
        }
        for y in (r.top * h as f32) as u32..((r.bottom * h as f32).ceil() as u32).min(h) {
            for x in (r.left * w as f32) as u32..((r.right * w as f32).ceil() as u32).min(w) {
                covered[(y * w + x) as usize] = true;
            }
        }
    }
    let selected = pixels
        .iter()
        .zip(&covered)
        .filter_map(|(p, c)| c.then_some(*p))
        .collect::<Vec<_>>();
    Ok(Features {
        embedding_model: "unavailable".into(),
        embedding: vec![0.; 64],
        mean_luminance: stats.mean_luminance,
        percentiles: stats.percentiles,
        shadow_clipping: stats.shadow_clipping,
        highlight_clipping: stats.highlight_clipping,
        face_count: faces.len(),
        face_mean_luminance: if selected.is_empty() {
            None
        } else {
            Some(SceneStats::from_linear_rgb(&selected)?.mean_luminance)
        },
        face_fraction: selected.len() as f64 / pixels.len() as f64,
        face_sharpness: if faces.is_empty() {
            0.
        } else {
            faces.iter().map(|f| f64::from(f.focus)).sum::<f64>() / faces.len() as f64
        },
        camera: description["camera"].as_str().unwrap_or("").into(),
        lens: description["lens"].as_str().unwrap_or("").into(),
        ..Default::default()
    })
}
