//! Exposure-normalized, clipping-aware scene-linear HDR in reference coordinates.
use crate::{
    LinearImage, Result,
    alignment::{Alignment, align},
};

#[derive(Clone, Copy, Debug)]
pub struct Exposure {
    pub shutter_s: f64,
    pub iso: f64,
    pub aperture: f64,
}
impl Exposure {
    pub fn from_metadata(m: &raw_decode::RawMetadata) -> Self {
        Self {
            shutter_s: m.shutter_s as f64,
            iso: m.iso as f64,
            aperture: m.aperture as f64,
        }
    }
    /// Relative sensor exposure, including ISO gain and aperture area.
    pub fn value(self) -> Result<f64> {
        if [self.shutter_s, self.iso, self.aperture]
            .iter()
            .any(|v| !v.is_finite() || *v <= 0.)
        {
            return Err("positive finite shutter, ISO and aperture required".into());
        }
        let v = self.shutter_s * (self.iso / 100.) / self.aperture.powi(2);
        if !v.is_finite() || v <= 0. {
            return Err("invalid exposure magnitude".into());
        }
        Ok(v)
    }
}
#[derive(Clone, Debug)]
pub struct BracketFrame {
    pub image: LinearImage,
    pub exposure: Exposure,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Deghost {
    None,
    Low,
    Medium,
    High,
}
#[derive(Clone, Debug)]
pub struct HdrOptions {
    pub reference: usize,
    pub auto_align: bool,
    pub deghost: Deghost,
    /// Refine EXIF using the median of overlapping log-ratio histograms.
    pub refine_exposure: bool,
}
impl Default for HdrOptions {
    fn default() -> Self {
        Self {
            reference: 0,
            auto_align: true,
            deghost: Deghost::Medium,
            refine_exposure: true,
        }
    }
}
#[derive(Debug)]
pub struct HdrResult {
    pub image: LinearImage,
    pub recipe: engine_api::recipe::Recipe,
    /// True means use reference only; in reference image coordinates.
    pub deghost_mask: Vec<bool>,
    /// Input/reference exposure ratios, after histogram refinement.
    pub exposure_ratios: Vec<f64>,
    pub alignments: Vec<Alignment>,
}
fn weight(v: f32) -> f64 {
    if !(0.002..0.995).contains(&v) {
        0.
    } else {
        f64::from(v.min(1. - v))
    }
}
fn refine(a: &LinearImage, b: &LinearImage, t: &Alignment, ratio: f64) -> f64 {
    let mut bins = [0usize; 513];
    let mut n = 0;
    for i in (0..a.pixels.len()).step_by((a.pixels.len() / 100000).max(1)) {
        let p = t.map((i % a.width) as f64, (i / a.width) as f64);
        let Some(b) = b.sample(p[0], p[1]) else {
            continue;
        };
        let a = a.pixels[i];
        for c in 0..3 {
            if (0.02..0.95).contains(&a[c]) && (0.02..0.95).contains(&b[c]) {
                let ev = ((b[c] as f64 / a[c] as f64) / ratio).log2();
                if ev.abs() <= 0.5 {
                    bins[((ev + 0.5) * 512.).round() as usize] += 1;
                    n += 1;
                }
            }
        }
    }
    if n < 32 {
        return ratio;
    }
    let mut cumulative = 0;
    for (i, v) in bins.iter().enumerate() {
        cumulative += v;
        if cumulative > n / 2 {
            return ratio * 2_f64.powf(i as f64 / 512. - 0.5);
        }
    }
    ratio
}
/// Returns radiance on the reference-exposure scale, not absolute photometry.
/// All frames must be in the same camera space and have identical dimensions.
pub fn hdr(frames: &[BracketFrame], o: &HdrOptions) -> Result<HdrResult> {
    if !(2..=64).contains(&frames.len()) || o.reference >= frames.len() {
        return Err("HDR needs 2..64 frames and a valid reference".into());
    }
    let reference = &frames[o.reference];
    let base = reference.exposure.value()?;
    let mut ratios = Vec::new();
    let mut alignments = Vec::new();
    for (j, frame) in frames.iter().enumerate() {
        frame.image.validate()?;
        if frame.image.width != reference.image.width
            || frame.image.height != reference.image.height
            || frame.image.color_matrix != reference.image.color_matrix
            || frame.image.as_shot_neutral != reference.image.as_shot_neutral
        {
            return Err("incompatible HDR geometry or camera metadata".into());
        }
        let ratio = frame.exposure.value()? / base;
        if !(1e-6..=1e6).contains(&ratio) {
            return Err("exposure ratio outside supported range".into());
        }
        let t = if o.auto_align && j != o.reference {
            align(&reference.image, &frame.image, ratio)?
        } else {
            Alignment::identity(&reference.image)
        };
        ratios.push(if o.refine_exposure && j != o.reference {
            refine(&reference.image, &frame.image, &t, ratio)
        } else {
            ratio
        });
        alignments.push(t);
    }
    let mut image = reference.image.clone();
    let mut mask = vec![false; image.pixels.len()];
    let threshold = match o.deghost {
        Deghost::None => f64::INFINITY,
        Deghost::Low => 0.35,
        Deghost::Medium => 0.18,
        Deghost::High => 0.08,
    };
    let mut samples = vec![None; frames.len()];
    for (i, p) in image.pixels.iter_mut().enumerate() {
        for (j, (frame, t)) in frames.iter().zip(&alignments).enumerate() {
            let xy = t.map((i % image.width) as f64, (i / image.width) as f64);
            samples[j] = frame.image.sample(xy[0], xy[1]);
        }
        // Clipped values are radiance intervals, not exact measurements.
        // Overlapping intervals are consistent; disjoint bounds can prove motion.
        mask[i] = samples.iter().zip(&ratios).any(|(sample, ratio)| {
            let Some(sample) = sample else {
                return false;
            };
            (0..3).any(|c| {
                let a = reference.image.pixels[i][c] as f64;
                let b = sample[c] as f64;
                let bounds = |v: f64, r: f64| {
                    if v >= 0.99 {
                        [0.99 / r, f64::INFINITY]
                    } else if v <= 0.01 {
                        [f64::NEG_INFINITY, 0.01 / r]
                    } else {
                        [v / r, v / r]
                    }
                };
                let aa = bounds(a, 1.);
                let bb = bounds(b, *ratio);
                let distance = (aa[0] - bb[1]).max(bb[0] - aa[1]).max(0.);
                distance > threshold * a.min(b / ratio).max(0.02) + 0.003 / ratio.min(1.)
            })
        });
        if mask[i] {
            continue;
        }
        for (c, value) in p.iter_mut().enumerate() {
            let mut sum = 0.;
            let mut weights = 0.;
            for (sample, ratio) in samples.iter().zip(&ratios) {
                let Some(sample) = sample else {
                    continue;
                };
                let v = sample[c];
                let w = weight(v);
                sum += v as f64 / ratio * w;
                weights += w;
            }
            *value = if weights > 0. {
                (sum / weights) as f32
            } else {
                // If every sample clips, shortest exposure gives the tightest lower bound.
                let (sample, ratio) = samples
                    .iter()
                    .zip(&ratios)
                    .filter_map(|(p, r)| p.as_ref().map(|p| (p, r)))
                    .min_by(|a, b| a.1.total_cmp(b.1))
                    .expect("reference always covers output");
                (sample[c] as f64 / ratio) as f32
            };
        }
    }
    Ok(HdrResult {
        recipe: crate::auto_recipe(&image)?,
        image,
        deghost_mask: mask,
        exposure_ratios: ratios,
        alignments,
    })
}
