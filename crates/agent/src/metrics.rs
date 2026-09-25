//! Objective measurements of the engine's display preview, not planner pixels.
use anyhow::{Result, ensure};
use engine_api::tools::FaceScore;
use serde::{Deserialize, Serialize};

/// Photographer-supplied Lab interval. No universal skin colour is assumed.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkinBand {
    pub low: [f64; 3],
    pub high: [f64; 3],
    pub max_delta_e: f64,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Metrics {
    pub highlight_clipping: f64,
    pub shadow_clipping: f64,
    pub mean_luminance: f64,
    pub percentiles: [f64; 3],
    pub contrast: f64,
    pub noise: f64,
    /// CIE76 distance outside the supplied Lab band, per face. None = unmeasured.
    pub skin_delta_e: Option<Vec<f64>>,
}
impl Metrics {
    pub fn acceptable(&self, target: f64, tolerance: f64) -> bool {
        self.highlight_clipping <= 0.05 && (self.mean_luminance - target).abs() <= tolerance
    }
}
pub fn linear(v: u8) -> f64 {
    let x = f64::from(v) / 255.;
    if x <= 0.04045 {
        x / 12.92
    } else {
        ((x + 0.055) / 1.055).powf(2.4)
    }
}
fn lab(rgb: [f64; 3]) -> [f64; 3] {
    let [r, g, b] = rgb;
    let f = |t: f64| {
        if t > 216. / 24389. {
            t.cbrt()
        } else {
            (24389. / 27. * t + 16.) / 116.
        }
    };
    let x = f((0.4124564 * r + 0.3575761 * g + 0.1804375 * b) / 0.95047);
    let y = f(0.2126729 * r + 0.7151522 * g + 0.072175 * b);
    let z = f((0.0193339 * r + 0.119192 * g + 0.9503041 * b) / 1.08883);
    [116. * y - 16., 500. * (x - y), 200. * (y - z)]
}
pub fn measure(
    rgb: &image::RgbImage,
    faces: &[FaceScore],
    band: Option<&SkinBand>,
) -> Result<Metrics> {
    ensure!(rgb.width() > 0 && rgb.height() > 0, "empty preview");
    let pixels = rgb.pixels().map(|p| p.0.map(linear)).collect::<Vec<_>>();
    let stats = style_profile::features::SceneStats::from_linear_rgb(
        &pixels
            .iter()
            .map(|p| p.map(|v| v as f32))
            .collect::<Vec<_>>(),
    )?;
    let count = pixels.len() as f64;
    let mean = stats.mean_luminance;
    let variance = pixels
        .iter()
        .map(|p| (0.2126 * p[0] + 0.7152 * p[1] + 0.0722 * p[2] - mean).powi(2))
        .sum::<f64>()
        / count;
    let skin_delta_e = if let Some(band) = band {
        ensure!(
            band.max_delta_e.is_finite()
                && band.max_delta_e >= 0.
                && (0..3).all(|i| band.low[i].is_finite()
                    && band.high[i].is_finite()
                    && band.low[i] <= band.high[i]),
            "invalid skin target band"
        );
        let mut deltas = Vec::new();
        for face in faces {
            let r = face.region;
            ensure!(r.is_valid(), "invalid face box");
            let mut sum = [0.; 3];
            let mut n = 0.;
            for y in (r.top * rgb.height() as f32).floor() as u32
                ..(r.bottom * rgb.height() as f32).ceil() as u32
            {
                for x in (r.left * rgb.width() as f32).floor() as u32
                    ..(r.right * rgb.width() as f32).ceil() as u32
                {
                    let p = lab(rgb
                        .get_pixel(x.min(rgb.width() - 1), y.min(rgb.height() - 1))
                        .0
                        .map(linear));
                    for c in 0..3 {
                        sum[c] += p[c];
                    }
                    n += 1.;
                }
            }
            if n > 0. {
                deltas.push(
                    (0..3)
                        .map(|c| {
                            let v = sum[c] / n;
                            (v - v.clamp(band.low[c], band.high[c])).powi(2)
                        })
                        .sum::<f64>()
                        .sqrt(),
                );
            }
        }
        (!deltas.is_empty()).then_some(deltas)
    } else {
        None
    };
    Ok(Metrics {
        highlight_clipping: rgb.pixels().filter(|p| p.0.contains(&255)).count() as f64 / count,
        shadow_clipping: rgb.pixels().filter(|p| p.0.contains(&0)).count() as f64 / count,
        mean_luminance: mean,
        percentiles: stats.percentiles,
        contrast: variance.sqrt(),
        noise: ml_quality::analyze(rgb)?.noise,
        skin_delta_e,
    })
}
