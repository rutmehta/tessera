use engine_api::{EngineError, EngineResult};
use serde::{Deserialize, Serialize};

/// Centered, covariance-free power iteration. Rank-deficient libraries are padded
/// with zero components, never arbitrary axes. Input vectors are L2-normalized.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(super) struct Pca {
    pub model: String,
    mean: Vec<f64>,
    axes: Vec<Vec<f64>>,
}
fn invalid() -> EngineError {
    EngineError::invalid(
        "embedding",
        "requires a model, consistent dimensions (1..4096), and finite values",
    )
}
fn dot(a: &[f64], b: &[f64]) -> f64 {
    a.iter().zip(b).map(|(x, y)| x * y).sum()
}
fn unit(input: &[f32], dimension: usize) -> EngineResult<Vec<f64>> {
    if input.len() != dimension || input.iter().any(|x| !x.is_finite()) {
        return Err(invalid());
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
        if model.is_empty() || !(1..=4096).contains(&dimension) {
            return Err(invalid());
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
        for _ in 0..32.min(dimension) {
            let Some(seed) = rows.iter().max_by(|a, b| dot(a, a).total_cmp(&dot(b, b))) else {
                break;
            };
            // A single sample may be exactly orthogonal to the leading PC.
            // Mix every residual row with deterministic pseudo-random coefficients.
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
            for _ in 0..96 {
                let mut next = vec![0.; dimension];
                for row in &rows {
                    let projection = dot(row, &axis);
                    for (x, r) in next.iter_mut().zip(row) {
                        *x += projection * r;
                    }
                }
                // Reorthogonalize to prevent recovered axes drifting into earlier PCs.
                for previous in &axes {
                    let projection = dot(&next, previous);
                    for (x, p) in next.iter_mut().zip(previous) {
                        *x -= projection * p;
                    }
                }
                let norm = dot(&next, &next).sqrt();
                if norm < 1e-12 {
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
    pub fn project(&self, model: &str, input: &[f32]) -> EngineResult<[f64; 32]> {
        if model != self.model {
            return Err(invalid());
        }
        let mut row = unit(input, self.mean.len())?;
        for (x, m) in row.iter_mut().zip(&self.mean) {
            *x -= m;
        }
        let mut output = [0.; 32];
        for (x, axis) in output.iter_mut().zip(&self.axes) {
            *x = dot(&row, axis).clamp(-1., 1.);
        }
        Ok(output)
    }
    pub fn valid(&self) -> bool {
        !self.model.is_empty()
            && (1..=4096).contains(&self.mean.len())
            && self
                .mean
                .iter()
                .all(|x| x.is_finite() && x.abs() <= 1.000001)
            && self.axes.len() <= 32.min(self.mean.len())
            && self.axes.iter().enumerate().all(|(i, a)| {
                a.len() == self.mean.len()
                    && a.iter().all(|x| x.is_finite())
                    && (dot(a, a) - 1.).abs() < 1e-6
                    && self.axes[..i].iter().all(|b| dot(a, b).abs() < 1e-6)
            })
    }
}
