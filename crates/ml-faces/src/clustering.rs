//! Cosine HDBSCAN with a bounded deterministic training sample for large catalogs.
use crate::normalize;
use anyhow::{Result, ensure};
use hdbscan::{DistanceMetric, Hdbscan, HdbscanHyperParams, NnAlgorithm};
use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq)]
pub struct FaceCluster {
    pub members: Vec<usize>,
    /// Original input ordinal of the actual member minimizing total cosine distance.
    pub medoid_index: usize,
    pub medoid: [f32; 128],
    /// False for density noise/singletons; not a calibrated identity probability.
    pub eligible: bool,
}
#[derive(Debug, Clone, PartialEq)]
pub struct ClusterResult {
    pub clusters: Vec<FaceCluster>,
    /// Per-input quality eligibility, independent of density/noise classification.
    pub eligibility: Vec<bool>,
    /// True when HDBSCAN was fitted to a bounded sample, then extended by medoids.
    pub approximate: bool,
}

/// Inputs from a detector box and `face_signals`; no blink heuristic is used.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FaceQuality {
    pub confidence: f32,
    pub width: f32,
    pub height: f32,
    pub sharpness: f64,
}
/// Configurable conservative defaults, not statistically calibrated probabilities.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct QualityGate {
    pub min_confidence: f32,
    pub min_size: f32,
    pub min_sharpness: f64,
}
impl Default for QualityGate {
    fn default() -> Self {
        Self {
            min_confidence: 0.9,
            min_size: 32.,
            min_sharpness: 0.1,
        }
    }
}
impl QualityGate {
    pub(crate) fn validate(self) -> Result<()> {
        ensure!(
            (0.0..=1.0).contains(&self.min_confidence),
            "invalid confidence gate"
        );
        ensure!(
            self.min_size.is_finite() && self.min_size > 0.,
            "invalid size gate"
        );
        ensure!(
            (0.0..=1.0).contains(&self.min_sharpness),
            "invalid sharpness gate"
        );
        Ok(())
    }
    pub fn eligible(self, face: FaceQuality) -> Result<bool> {
        self.validate()?;
        Ok((self.min_confidence..=1.).contains(&face.confidence)
            && face.width.is_finite()
            && face.width >= self.min_size
            && face.height.is_finite()
            && face.height >= self.min_size
            && (self.min_sharpness..=1.).contains(&face.sharpness))
    }
}
/// Only eligible faces train density clusters or become medoids. Rejected inputs
/// remain singleton entries with `eligible=false`; an unusable rejected descriptor
/// is represented by a zero medoid and MUST NOT be offered to nearest-medoid lookup.
pub fn cluster_eligible(
    embeddings: &[[f32; 128]],
    quality: &[FaceQuality],
    threshold: f32,
    gate: QualityGate,
) -> Result<ClusterResult> {
    validate_threshold(threshold)?;
    gate.validate()?;
    ensure!(embeddings.len() == quality.len(), "quality length mismatch");
    let eligibility = quality
        .iter()
        .map(|&q| gate.eligible(q))
        .collect::<Result<Vec<_>>>()?;
    let ordinals: Vec<_> = (0..embeddings.len()).filter(|&i| eligibility[i]).collect();
    let selected: Vec<_> = ordinals.iter().map(|&i| embeddings[i]).collect();
    let mut result = cluster_with_medoids(&selected, threshold)?;
    for c in &mut result.clusters {
        c.medoid_index = ordinals[c.medoid_index];
        for i in &mut c.members {
            *i = ordinals[*i];
        }
    }
    for (i, &eligible) in eligibility.iter().enumerate() {
        if !eligible {
            result.clusters.push(FaceCluster {
                members: vec![i],
                medoid_index: i,
                medoid: normalize(&embeddings[i]).unwrap_or([0.; 128]),
                eligible: false,
            });
        }
    }
    result.clusters.sort_by_key(|c| c.members[0]);
    result.eligibility = eligibility;
    Ok(result)
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MedoidMatch {
    /// Index in the supplied medoids slice, NOT an input face or persistent person ID.
    pub medoid_index: usize,
    pub similarity: f32,
}
/// Incremental suggestion only; no clustering, ID allocation, or persistence occurs.
/// Caller supplies only eligible/confirmed person medoids and gates incoming quality.
/// None means below threshold/new person. Equal scores choose the lowest index.
pub fn nearest_medoid(
    embedding: &[f32; 128],
    medoids: &[[f32; 128]],
    threshold: f32,
) -> Result<Option<MedoidMatch>> {
    validate_threshold(threshold)?;
    let query = normalize(embedding)?;
    let unit = medoids.iter().map(normalize).collect::<Result<Vec<_>>>()?;
    Ok(nearest_unit(&query, &unit, threshold))
}
fn nearest_unit(query: &[f32; 128], medoids: &[[f32; 128]], threshold: f32) -> Option<MedoidMatch> {
    let mut best: Option<MedoidMatch> = None;
    for (medoid_index, center) in medoids.iter().enumerate() {
        let similarity = cosine(query, center);
        if similarity >= threshold && best.is_none_or(|m| similarity > m.similarity) {
            best = Some(MedoidMatch {
                medoid_index,
                similarity,
            });
        }
    }
    best
}

fn validate_threshold(threshold: f32) -> Result<()> {
    ensure!(
        (-1.0..=1.0).contains(&threshold),
        "invalid cosine threshold"
    );
    Ok(())
}
fn cosine(a: &[f32; 128], b: &[f32; 128]) -> f32 {
    // f32 normalization is not exact. Recompute norms in f64 so inclusive
    // threshold=1 accepts identical vectors rather than accumulating f32 drift.
    let (mut dot, mut aa, mut bb) = (0f64, 0f64, 0f64);
    for (&x, &y) in a.iter().zip(b) {
        let (x, y) = (f64::from(x), f64::from(y));
        dot += x * y;
        aa += x * x;
        bb += y * y;
    }
    (dot / (aa * bb).sqrt()).clamp(-1., 1.) as f32
}
pub(crate) fn medoid(unit: &[[f32; 128]], members: &[usize]) -> usize {
    // For unit vectors, argmin sum(1 - x.y) = argmax x.sum(y).
    // This is an EXACT cosine medoid in O(n*d), not a centroid approximation.
    let mut sum = [0f64; 128];
    for &i in members {
        for (s, &v) in sum.iter_mut().zip(&unit[i]) {
            *s += f64::from(v);
        }
    }
    let mut best = members[0];
    let mut score = f64::NEG_INFINITY;
    for &i in members {
        let value = unit[i]
            .iter()
            .zip(sum)
            .map(|(&v, s)| f64::from(v) * s)
            .sum::<f64>();
        if value > score {
            best = i;
            score = value;
        }
    }
    best
}
fn make_cluster(unit: &[[f32; 128]], mut members: Vec<usize>, eligible: bool) -> FaceCluster {
    members.sort_unstable();
    let medoid_index = medoid(unit, &members);
    FaceCluster {
        eligible: eligible && members.len() >= 2,
        members,
        medoid_index,
        medoid: unit[medoid_index],
    }
}

/// Compatible partition API: every input occurs once, including singleton noise.
/// `threshold` is cosine similarity, not HDBSCAN's distance epsilon.
pub fn cluster(embeddings: &[[f32; 128]], threshold: f32) -> Result<Vec<Vec<usize>>> {
    Ok(cluster_with_medoids(embeddings, threshold)?
        .clusters
        .into_iter()
        .map(|c| c.members)
        .collect())
}

/// HDBSCAN mutual-reachability MST, condensed hierarchy and excess-of-mass selection
/// via `hdbscan`. Minimum cluster size 2 / min samples 1; cosine metric.
/// Selection epsilon = 1-threshold merges fine density splits; a medoid similarity
/// gate rejects distant members. This is NOT the former complete-link constraint.
/// Noise is retained as ineligible singleton groups.
pub fn cluster_with_medoids(embeddings: &[[f32; 128]], threshold: f32) -> Result<ClusterResult> {
    validate_threshold(threshold)?;
    let unit = embeddings
        .iter()
        .map(normalize)
        .collect::<Result<Vec<_>>>()?;
    if unit.len() < 2 {
        return Ok(ClusterResult {
            eligibility: vec![true; unit.len()],
            clusters: (0..unit.len())
                .map(|i| make_cluster(&unit, vec![i], false))
                .collect(),
            approximate: false,
        });
    }
    if unit.len() > 1024 {
        // Fixed-seed reservoir sampling avoids periodic input/stride aliasing. This
        // approximates full-catalog HDBSCAN: rare unsampled identities stay noise.
        let mut sample: Vec<_> = (0..1024).collect();
        let mut seed = 0x9e3779b97f4a7c15u64;
        for i in 1024..unit.len() {
            seed ^= seed >> 12;
            seed ^= seed << 25;
            seed ^= seed >> 27;
            let j = (seed.wrapping_mul(2685821657736338717) % (i as u64 + 1)) as usize;
            if j < sample.len() {
                sample[j] = i;
            }
        }
        sample.sort_unstable();
        let selected: Vec<_> = sample.iter().map(|&i| unit[i]).collect();
        let base = cluster_with_medoids(&selected, threshold)?;
        let mut groups = Vec::<Vec<usize>>::new();
        let mut centers = Vec::new();
        let mut clusters = Vec::new();
        let mut sampled = vec![false; unit.len()];
        for &i in &sample {
            sampled[i] = true;
        }
        for c in base.clusters {
            let members: Vec<_> = c.members.into_iter().map(|i| sample[i]).collect();
            if c.eligible {
                centers.push(c.medoid);
                groups.push(members);
            } else {
                clusters.push(make_cluster(&unit, members, false));
            }
        }
        for (i, row) in unit.iter().enumerate() {
            if sampled[i] {
                continue;
            }
            if let Some(m) = nearest_unit(row, &centers, threshold) {
                groups[m.medoid_index].push(i);
            } else {
                clusters.push(make_cluster(&unit, vec![i], false));
            }
        }
        clusters.extend(groups.into_iter().map(|g| make_cluster(&unit, g, true)));
        clusters.sort_by_key(|c| c.members[0]);
        return Ok(ClusterResult {
            clusters,
            eligibility: vec![true; unit.len()],
            approximate: true,
        });
    }
    let mut data = vec![vec![0f32; unit.len()]; unit.len()];
    for i in 0..unit.len() {
        for j in 0..i {
            let distance = (1. - cosine(&unit[i], &unit[j])).max(0.);
            data[i][j] = distance;
            data[j][i] = distance;
        }
    }
    let params = HdbscanHyperParams::builder()
        .min_cluster_size(2)
        .min_samples(1)
        .allow_single_cluster(true)
        .epsilon(f64::from(1. - threshold))
        .dist_metric(DistanceMetric::Precalculated)
        .nn_algorithm(NnAlgorithm::BruteForce)
        .build();
    let labels = Hdbscan::new(&data, params).cluster()?;
    let mut groups = BTreeMap::<i32, Vec<usize>>::new();
    let mut clusters = Vec::new();
    for (i, label) in labels.into_iter().enumerate() {
        if label < 0 {
            clusters.push(make_cluster(&unit, vec![i], false));
        } else {
            groups.entry(label).or_default().push(i);
        }
    }
    for members in groups.into_values() {
        let center = medoid(&unit, &members);
        let (keep, reject): (Vec<_>, Vec<_>) = members
            .into_iter()
            .partition(|&i| cosine(&unit[i], &unit[center]) >= threshold);
        if !keep.is_empty() {
            clusters.push(make_cluster(&unit, keep, true));
        }
        clusters.extend(
            reject
                .into_iter()
                .map(|i| make_cluster(&unit, vec![i], false)),
        );
    }
    clusters.sort_by_key(|c| c.members[0]);
    Ok(ClusterResult {
        clusters,
        eligibility: vec![true; unit.len()],
        approximate: false,
    })
}
