//! YuNet/SFace face signals.
use anyhow::{Result, ensure};
mod geometry;
mod models;
mod scorer;
pub use geometry::{FaceSignals, Letterbox, align_crop, face_signals, letterbox, nms};
pub use models::FaceModels;
pub use scorer::FaceScorer;

#[derive(Debug, Clone)]
pub struct Face {
    /// Image pixel coordinates, x/y/width/height.
    pub bbox: [f32; 4],
    /// Right eye, left eye, nose, right mouth corner, left mouth corner.
    pub landmarks5: [[f32; 2]; 5],
    pub score: f32,
}

/// Deterministic complete-link cosine clustering; entries are input ordinals.
pub fn cluster(embeddings: &[[f32; 128]], threshold: f32) -> Result<Vec<Vec<usize>>> {
    ensure!(
        (-1.0..=1.0).contains(&threshold),
        "invalid cosine threshold"
    );
    let unit = embeddings
        .iter()
        .map(normalize)
        .collect::<Result<Vec<_>>>()?;
    let mut groups: Vec<Vec<usize>> = (0..unit.len()).map(|i| vec![i]).collect();
    loop {
        let mut best = None;
        let mut best_similarity = f32::NEG_INFINITY;
        for a in 0..groups.len() {
            for b in a + 1..groups.len() {
                let similarity = groups[a]
                    .iter()
                    .flat_map(|&i| groups[b].iter().map(move |&j| (i, j)))
                    .map(|(i, j)| unit[i].iter().zip(unit[j]).map(|(x, y)| x * y).sum::<f32>())
                    .fold(1.0, f32::min);
                if similarity >= threshold && similarity > best_similarity {
                    best = Some((a, b));
                    best_similarity = similarity;
                }
            }
        }
        let Some((a, b)) = best else { break };
        let members = groups.remove(b);
        groups[a].extend(members);
        groups[a].sort_unstable();
    }
    Ok(groups)
}

fn normalize(input: &[f32; 128]) -> Result<[f32; 128]> {
    ensure!(input.iter().all(|v| v.is_finite()), "nonfinite descriptor");
    let norm = input
        .iter()
        .map(|&v| f64::from(v).powi(2))
        .sum::<f64>()
        .sqrt();
    ensure!(norm > 1e-12, "zero descriptor");
    Ok(input.map(|v| (f64::from(v) / norm) as f32))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn clustering_is_scale_invariant_and_keeps_distinct_people() {
        let mut a = [0.; 128];
        a[0] = 1.;
        let mut b = a;
        b[0] = 7.;
        let mut c = [0.; 128];
        c[1] = 1.;
        assert_eq!(cluster(&[a, b, c], 0.5).unwrap(), vec![vec![0, 1], vec![2]]);
        assert!(cluster(&[[0.; 128]], 0.5).is_err());
        assert!(cluster(&[a], f32::NAN).is_err());
        assert!(cluster(&[], 0.5).unwrap().is_empty());
    }
}
