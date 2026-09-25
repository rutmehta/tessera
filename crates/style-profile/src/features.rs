use engine_api::{EngineError, EngineResult};
use serde::{Deserialize, Serialize};

/// Scene descriptors in linear-light units; missing face luminance stays optional.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Features {
    pub embedding_model: String,
    pub embedding: Vec<f32>,
    pub mean_luminance: f64,
    /// Linear-light luminance at the 10th, 50th and 90th percentiles.
    pub percentiles: [f64; 3],
    pub shadow_clipping: f64,
    pub highlight_clipping: f64,
    pub as_shot_cct: f64,
    pub as_shot_duv: f64,
    pub face_count: usize,
    pub face_mean_luminance: Option<f64>,
    pub face_fraction: f64,
    pub face_sharpness: f64,
    pub camera: String,
    pub lens: String,
}

impl Default for Features {
    fn default() -> Self {
        Self {
            embedding_model: "siglip".into(),
            embedding: vec![0.; 64],
            mean_luminance: 0.18,
            percentiles: [0.18; 3],
            shadow_clipping: 0.,
            highlight_clipping: 0.,
            as_shot_cct: 6500.,
            as_shot_duv: 0.,
            face_count: 0,
            face_mean_luminance: None,
            face_fraction: 0.,
            face_sharpness: 0.,
            camera: String::new(),
            lens: String::new(),
        }
    }
}

impl Features {
    pub fn validate(&self) -> EngineResult<()> {
        if self.embedding_model.trim().is_empty()
            || !(1..=4096).contains(&self.embedding.len())
            || self.embedding.iter().any(|v| !v.is_finite())
        {
            return Err(EngineError::invalid(
                "embedding",
                "requires a model and 1..4096 finite values",
            ));
        }
        for (name, value) in [
            ("mean_luminance", self.mean_luminance),
            (
                "face_mean_luminance",
                self.face_mean_luminance.unwrap_or(0.),
            ),
            ("face_sharpness", self.face_sharpness),
        ]
        .into_iter()
        .chain(self.percentiles.into_iter().map(|x| ("percentiles", x)))
        {
            if !value.is_finite() || value < 0. {
                return Err(EngineError::invalid(name, "must be finite and nonnegative"));
            }
        }
        if self.percentiles.windows(2).any(|p| p[0] > p[1]) {
            return Err(EngineError::invalid("percentiles", "must be ordered"));
        }
        for (name, value) in [
            ("shadow_clipping", self.shadow_clipping),
            ("highlight_clipping", self.highlight_clipping),
            ("face_fraction", self.face_fraction),
        ] {
            if !(0.0..=1.0).contains(&value) {
                return Err(EngineError::invalid(name, "must be in [0,1]"));
            }
        }
        if !self.as_shot_cct.is_finite() || self.as_shot_cct <= 0. {
            return Err(EngineError::invalid(
                "as_shot_cct",
                "must be finite and positive",
            ));
        }
        if !self.as_shot_duv.is_finite() || self.as_shot_duv.abs() > 1. {
            return Err(EngineError::invalid(
                "as_shot_duv",
                "must be finite and in [-1,1]",
            ));
        }
        Ok(())
    }

    /// Continuous descriptors only. Log luminance and CCT keep HDR values and
    /// kelvin scales comparable; Duv is measured in hundredths. A presence flag
    /// distinguishes absent face luminance from a genuinely black face.
    pub(crate) fn numeric(&self) -> Vec<f64> {
        let luminance = |x: f64| x.ln_1p();
        let mut values = vec![luminance(self.mean_luminance)];
        values.extend(self.percentiles.map(luminance));
        values.extend([
            self.shadow_clipping,
            self.highlight_clipping,
            self.as_shot_cct.ln() - 6500_f64.ln(),
            self.as_shot_duv / 0.01,
            (self.face_count as f64).ln_1p(),
            self.face_mean_luminance.map(luminance).unwrap_or(0.),
            if self.face_mean_luminance.is_some() {
                1.
            } else {
                0.
            },
            self.face_fraction,
            self.face_sharpness.ln_1p(),
        ]);
        values
    }
}

/// Statistics of nonnegative linear Rec.709 RGB, retaining values above white.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct SceneStats {
    pub mean_luminance: f64,
    /// Linearly interpolated 10th, 50th and 90th percentiles.
    pub percentiles: [f64; 3],
    /// Fraction of pixels whose every channel is at black (zero).
    pub shadow_clipping: f64,
    /// Fraction of pixels with at least one channel at or above white (one).
    pub highlight_clipping: f64,
}

impl SceneStats {
    pub fn from_linear_rgb(pixels: &[[f32; 3]]) -> EngineResult<Self> {
        if pixels.is_empty() || pixels.iter().flatten().any(|v| !v.is_finite() || *v < 0.) {
            return Err(EngineError::invalid(
                "pixels",
                "requires nonempty finite nonnegative linear RGB",
            ));
        }
        let count = pixels.len() as f64;
        let mut luminance: Vec<f64> = pixels
            .iter()
            .map(|p| 0.2126 * f64::from(p[0]) + 0.7152 * f64::from(p[1]) + 0.0722 * f64::from(p[2]))
            .collect();
        let mean_luminance = luminance.iter().map(|v| v / count).sum();
        luminance.sort_unstable_by(f64::total_cmp);
        let percentiles = [0.1, 0.5, 0.9].map(|q| {
            let position = q * (luminance.len() - 1) as f64;
            let lo = position.floor() as usize;
            let hi = position.ceil() as usize;
            luminance[lo] + (luminance[hi] - luminance[lo]) * position.fract()
        });
        Ok(Self {
            mean_luminance,
            percentiles,
            shadow_clipping: pixels.iter().filter(|p| p.iter().all(|v| *v == 0.)).count() as f64
                / count,
            highlight_clipping: pixels.iter().filter(|p| p.iter().any(|v| *v >= 1.)).count() as f64
                / count,
        })
    }
}

/// Centered PCA of L2-normalized embeddings, using covariance-free power
/// iteration (as in cull). Rank-deficient inputs are zero-padded to 64 scores;
/// no original embedding coordinates are substituted for missing components.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Pca {
    pub model: String,
    mean: Vec<f64>,
    axes: Vec<Vec<f64>>,
}

fn invalid_embedding() -> EngineError {
    EngineError::invalid(
        "embedding",
        "requires a matching model, dimensions in 1..4096, and finite values",
    )
}

fn dot(a: &[f64], b: &[f64]) -> f64 {
    a.iter().zip(b).map(|(x, y)| x * y).sum()
}

fn unit(input: &[f32], dimension: usize) -> EngineResult<Vec<f64>> {
    if input.len() != dimension || input.iter().any(|x| !x.is_finite()) {
        return Err(invalid_embedding());
    }
    let mut row: Vec<_> = input.iter().map(|x| f64::from(*x)).collect();
    let norm = dot(&row, &row).sqrt();
    if norm > 0. {
        for x in &mut row {
            *x /= norm;
        }
    }
    Ok(row)
}

impl Pca {
    pub fn fit(model: &str, samples: &[Vec<f32>]) -> EngineResult<Self> {
        let dimension = samples.first().map_or(0, Vec::len);
        if model.trim().is_empty() || !(1..=4096).contains(&dimension) {
            return Err(invalid_embedding());
        }
        let mut rows = samples
            .iter()
            .map(|r| unit(r, dimension))
            .collect::<EngineResult<Vec<_>>>()?;
        let mut mean = vec![0.; dimension];
        for row in &rows {
            for (m, x) in mean.iter_mut().zip(row) {
                *m += x / rows.len() as f64;
            }
        }
        for row in &mut rows {
            for (x, m) in row.iter_mut().zip(&mean) {
                *x -= m;
            }
        }
        let mut axes: Vec<Vec<f64>> = Vec::new();
        for _ in 0..64.min(dimension).min(samples.len() - 1) {
            let Some(seed) = rows.iter().max_by(|a, b| dot(a, a).total_cmp(&dot(b, b))) else {
                break;
            };
            // Mix all residual rows so an individual sample orthogonal to the
            // leading component cannot trap power iteration on a lesser axis.
            let mut state = 0x243f6a8885a308d3u64;
            let mut axis = vec![0.; dimension];
            for row in &rows {
                state = state.wrapping_mul(6364136223846793005).wrapping_add(1);
                let coefficient = ((state >> 32) as f64 / u32::MAX as f64) - 0.5;
                for (a, x) in axis.iter_mut().zip(row) {
                    *a += coefficient * x;
                }
            }
            if dot(&axis, &axis) < 1e-20 {
                axis = seed.clone();
            }
            let norm = dot(&axis, &axis).sqrt();
            if norm < 1e-10 {
                break;
            }
            for x in &mut axis {
                *x /= norm;
            }
            for _ in 0..128 {
                let mut next = vec![0.; dimension];
                for row in &rows {
                    let projection = dot(row, &axis);
                    for (x, r) in next.iter_mut().zip(row) {
                        *x += projection * r;
                    }
                }
                // Double modified Gram-Schmidt resists numerical rank leakage.
                for _ in 0..2 {
                    for previous in &axes {
                        let projection = dot(&next, previous);
                        for (x, p) in next.iter_mut().zip(previous) {
                            *x -= projection * p;
                        }
                    }
                }
                let norm = dot(&next, &next).sqrt();
                if norm < 1e-20 {
                    break;
                }
                for x in &mut next {
                    *x /= norm;
                }
                let change: f64 = next.iter().zip(&axis).map(|(x, y)| (x - y).powi(2)).sum();
                axis = next;
                if change < 1e-16 {
                    break;
                }
            }
            // Deterministic sign: largest-magnitude loading is positive.
            if axis
                .iter()
                .max_by(|a, b| a.abs().total_cmp(&b.abs()))
                .is_some_and(|x| *x < 0.)
            {
                for x in &mut axis {
                    *x = -*x;
                }
            }
            for row in &mut rows {
                let projection = dot(row, &axis);
                for (x, a) in row.iter_mut().zip(&axis) {
                    *x -= projection * a;
                }
            }
            axes.push(axis);
        }
        Ok(Self {
            model: model.into(),
            mean,
            axes,
        })
    }

    pub fn project(&self, model: &str, input: &[f32]) -> EngineResult<Vec<f64>> {
        if model != self.model || !self.valid() {
            return Err(invalid_embedding());
        }
        let mut row = unit(input, self.mean.len())?;
        for (x, m) in row.iter_mut().zip(&self.mean) {
            *x -= m;
        }
        let mut output = vec![0.; 64];
        for (x, axis) in output.iter_mut().zip(&self.axes) {
            *x = dot(&row, axis);
        }
        Ok(output)
    }

    /// Validate persisted model shape, finite values and orthonormal axes.
    pub fn valid(&self) -> bool {
        !self.model.trim().is_empty()
            && (1..=4096).contains(&self.mean.len())
            && self
                .mean
                .iter()
                .all(|x| x.is_finite() && x.abs() <= 1.000001)
            && dot(&self.mean, &self.mean) <= 1.000001
            && self.axes.len() <= 64.min(self.mean.len())
            && self.axes.iter().enumerate().all(|(i, a)| {
                a.len() == self.mean.len()
                    && a.iter().all(|x| x.is_finite())
                    && (dot(a, a) - 1.).abs() < 1e-6
                    && self.axes[..i].iter().all(|b| dot(a, b).abs() < 1e-6)
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pca_validates_inputs_and_rejects_corrupt_persisted_models() {
        for samples in [
            vec![],
            vec![vec![]],
            vec![vec![0.], vec![0., 1.]],
            vec![vec![f32::NAN]],
            vec![vec![0.; 4097]],
        ] {
            assert!(Pca::fit("siglip", &samples).is_err());
        }
        assert!(Pca::fit("  ", &[vec![0.]]).is_err());
        let pca = Pca::fit("siglip", &[vec![1., 0.], vec![0., 1.]]).unwrap();
        assert!(pca.project("other", &[1., 0.]).is_err());
        assert!(pca.project("siglip", &[1.]).is_err());
        assert!(pca.project("siglip", &[f32::INFINITY, 0.]).is_err());
        let restored: Pca = serde_json::from_str(&serde_json::to_string(&pca).unwrap()).unwrap();
        assert!(restored.valid());
        assert_eq!(
            restored.project("siglip", &[1., 0.]).unwrap(),
            pca.project("siglip", &[1., 0.]).unwrap()
        );
        let mut corrupt = pca.clone();
        corrupt.axes[0][0] = f64::NAN;
        assert!(!corrupt.valid());
        assert!(corrupt.project("siglip", &[1., 0.]).is_err());
        corrupt = pca.clone();
        corrupt.axes[0].pop();
        assert!(!corrupt.valid());
        corrupt = pca.clone();
        corrupt.axes.push(corrupt.axes[0].clone());
        assert!(!corrupt.valid());
        corrupt = pca;
        corrupt.mean = vec![1., 1.];
        assert!(!corrupt.valid());
    }

    #[test]
    fn pca_preserves_all_64_components_and_is_deterministic() {
        let samples: Vec<_> = (0..64)
            .flat_map(|i| {
                let mut a = vec![0.; 72];
                a[i + 8] = 1.;
                let mut b = a.clone();
                b[i + 8] = -1.;
                [a, b]
            })
            .collect();
        let pca = Pca::fit("siglip", &samples).unwrap();
        assert!(pca.valid());
        assert_eq!(pca.axes.len(), 64);
        for row in &samples {
            let output = pca.project("siglip", row).unwrap();
            assert!((dot(&output, &output) - 1.).abs() < 1e-8);
        }
        let again = Pca::fit("siglip", &samples).unwrap();
        assert_eq!(pca.axes, again.axes);
        for samples in [vec![vec![0.; 64]], vec![vec![3., 4.]; 3]] {
            let pca = Pca::fit("siglip", &samples).unwrap();
            assert!(pca.valid());
            assert_eq!(pca.project("siglip", &samples[0]).unwrap(), vec![0.; 64]);
        }
    }

    #[test]
    fn pca_orders_components_by_variance_without_clamping_scores() {
        // Unequal counts yield a nonzero mean and a centered score above one.
        let samples = vec![vec![1., 0.], vec![1., 0.], vec![1., 0.], vec![-1., 0.]];
        let pca = Pca::fit("siglip", &samples).unwrap();
        assert!((pca.project("siglip", &[-1., 0.]).unwrap()[0] + 1.5).abs() < 1e-8);
        let samples = vec![
            vec![1., 0.],
            vec![-1., 0.],
            vec![1., 0.],
            vec![-1., 0.],
            vec![0., 1.],
            vec![0., -1.],
        ];
        let pca = Pca::fit("siglip", &samples).unwrap();
        assert!(pca.project("siglip", &[1., 0.]).unwrap()[0] > 0.999999);
        assert!(pca.project("siglip", &[0., 1.]).unwrap()[1] > 0.999999);
    }

    #[test]
    fn pca_is_centered_and_recovers_signal_beyond_first_64_dimensions() {
        let mut a = vec![0.; 768];
        let mut b = a.clone();
        a[700] = 1.;
        b[700] = -1.;
        let pca = Pca::fit("siglip", &[a.clone(), b.clone()]).unwrap();
        assert!(pca.valid());
        let positive = pca.project("siglip", &a).unwrap();
        let negative = pca.project("siglip", &b).unwrap();
        assert_eq!(positive.len(), 64);
        assert!((positive[0] - 1.).abs() < 1e-9);
        assert!((negative[0] + 1.).abs() < 1e-9);
        assert!(positive[1..].iter().all(|v| *v == 0.));
        let offset = Pca::fit("siglip", &[vec![1., 0.], vec![0., 1.]]).unwrap();
        let x = offset.project("siglip", &[1., 0.]).unwrap();
        let y = offset.project("siglip", &[0., 1.]).unwrap();
        assert!((x[0] + y[0]).abs() < 1e-9);
        assert!((x[0].abs() - 0.5_f64.sqrt()).abs() < 1e-9);
    }

    #[test]
    fn scene_statistics_use_linear_rec709_and_interpolated_percentiles() {
        let stats = SceneStats::from_linear_rgb(&[[0.; 3], [0.5; 3], [1.; 3]]).unwrap();
        assert!((stats.mean_luminance - 0.5).abs() < 1e-12);
        for (actual, expected) in stats.percentiles.into_iter().zip([0.1, 0.5, 0.9]) {
            assert!((actual - expected).abs() < 1e-12);
        }
        assert!((stats.shadow_clipping - 1. / 3.).abs() < 1e-12);
        assert!((stats.highlight_clipping - 1. / 3.).abs() < 1e-12);
        let red = SceneStats::from_linear_rgb(&[[1., 0., 0.]]).unwrap();
        assert!((red.mean_luminance - 0.2126).abs() < 1e-12);
        assert_eq!(red.highlight_clipping, 1.);
        let hdr = SceneStats::from_linear_rgb(&[[2.; 3]]).unwrap();
        assert!((hdr.mean_luminance - 2.).abs() < 1e-12);
        assert!(SceneStats::from_linear_rgb(&[]).is_err());
        assert!(SceneStats::from_linear_rgb(&[[f32::NAN; 3]]).is_err());
        assert!(SceneStats::from_linear_rgb(&[[-1.; 3]]).is_err());
    }

    #[test]
    fn rejects_invalid_descriptors_but_accepts_hdr() {
        let mut f = Features {
            mean_luminance: f64::NAN,
            ..Features::default()
        };
        assert!(f.validate().is_err());
        f = Features::default();
        f.percentiles = [0.5, 0.1, 0.9];
        assert!(f.validate().is_err());
        for bad in [f64::NAN, f64::INFINITY, -0.1, 1.1] {
            f = Features::default();
            f.shadow_clipping = bad;
            assert!(f.validate().is_err());
            f = Features::default();
            f.highlight_clipping = bad;
            assert!(f.validate().is_err());
            f = Features::default();
            f.face_fraction = bad;
            assert!(f.validate().is_err());
        }
        f = Features::default();
        f.as_shot_cct = 0.;
        assert!(f.validate().is_err());
        f = Features::default();
        f.as_shot_duv = f64::NAN;
        assert!(f.validate().is_err());
        f = Features::default();
        f.face_mean_luminance = Some(-1.);
        assert!(f.validate().is_err());
        f = Features::default();
        f.face_sharpness = -1.;
        assert!(f.validate().is_err());
        f = Features::default();
        f.embedding.clear();
        assert!(f.validate().is_err());
        f = Features::default();
        f.embedding[0] = f32::NAN;
        assert!(f.validate().is_err());
        f = Features::default();
        f.embedding_model = "  ".into();
        assert!(f.validate().is_err());
        f = Features::default();
        f.mean_luminance = 4.;
        f.percentiles = [0., 2., 8.];
        f.validate().unwrap();
    }

    #[test]
    fn valid_extreme_descriptors_have_finite_numeric_values() {
        let f = Features {
            as_shot_cct: f64::from_bits(1),
            mean_luminance: f64::MAX,
            face_mean_luminance: Some(f64::MAX),
            face_sharpness: f64::MAX,
            face_count: usize::MAX,
            ..Features::default()
        };
        f.validate().unwrap();
        assert!(f.numeric().iter().all(|v| v.is_finite()));
    }

    #[test]
    fn neutral_features_are_valid_and_numeric_excludes_identity() {
        let a = Features::default();
        a.validate().unwrap();
        assert_eq!(a.embedding_model, "siglip");
        assert_eq!(a.embedding, vec![0.; 64]);
        let mut b = a.clone();
        b.camera = "other camera".into();
        b.lens = "other lens".into();
        b.embedding = vec![1.; 768];
        assert_eq!(a.numeric(), b.numeric());
        assert!(a.numeric().iter().all(|x| x.is_finite()));
        assert_eq!(
            serde_json::from_str::<Features>(&serde_json::to_string(&a).unwrap()).unwrap(),
            a
        );
    }
}
