//! YuNet/SFace face signals.
use anyhow::{Result, ensure};
mod geometry;
mod models;
pub mod people;
mod scorer;
mod strip;
pub use geometry::{FaceSignals, Letterbox, align_crop, face_signals, letterbox, nms};
pub use models::FaceModels;
pub use scorer::FaceScorer;
pub use strip::{FaceChip, face_strip, face_strip_from_index, frames_with_person_eyes_closed};

#[derive(Debug, Clone)]
pub struct Face {
    /// Image pixel coordinates, x/y/width/height.
    pub bbox: [f32; 4],
    /// Right eye, left eye, nose, right mouth corner, left mouth corner.
    pub landmarks5: [[f32; 2]; 5],
    pub score: f32,
}

mod clustering;
pub use clustering::{
    ClusterResult, FaceCluster, FaceQuality, MedoidMatch, QualityGate, cluster, cluster_eligible,
    cluster_with_medoids, nearest_medoid,
};

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
