// ENG-3f: sweeps that cross the actual denominator switch D = |Y| of
//   D = max(|Y|, epsilon * clamp(1 - rho / k, 0, 1)),  rho = |Y| / A,
// i.e. rho* = k * (1 - |Y| / epsilon). Shared by the luminance curve
// (pipeline-gpu) and Photo Filter (filters) tests, on CPU and Metal.
//
// With target luminance T the output luminance is
//   Yout = T                        when D == |Y| (ratio branch),
//   Yout = Y + (T - Y) * |Y| / D    when D >  |Y| (floor branch),
// which is continuous at D = |Y| and piecewise C1, so by the mean value
// theorem every adjacent step obeys
//   |dYout| <= S_rho * |d rho| + S_y * |d Y| + rounding,
// where S_rho and S_y are suprema of the partial derivatives over the sweep
// domain. Each operator test supplies those two suprema with its derivation.
pub const EPSILON: f64 = 1e-3;
pub const K: f64 = 0.25;
pub const SAMPLES: usize = 2001;
const W: [f64; 3] = [0.2627, 0.678, 0.0593];
// Rounding allowance per sample, in units of 2^-24 (f32 half-ulp, relative)
// times the weighted absolute output A_out = sum w_c * |out_c|:
//   2  input luminance (three products, two sums of cancelling terms; its
//      relative error 2u/rho is amplified by the gain into 2u * A_out),
//   2  rho -> D (d D = epsilon / k * d rho, and A >= 4 * epsilon here),
//   3  target evaluation and gain arithmetic (difference, quotient, sum),
//   1  final product rounded to f32.
const ROUNDING_UNITS: f64 = 8.0;

pub fn luma(p: [f32; 3]) -> f64 {
    W[0] * p[0] as f64 + W[1] * p[1] as f64 + W[2] * p[2] as f64
}
fn weighted_abs(p: [f32; 3]) -> f64 {
    luma(p.map(f32::abs))
}

pub struct Sweep {
    pub name: &'static str,
    /// Start and end of rho = |Y| / A.
    pub rho: [f64; 2],
    /// Start and end of the positive luminance Y.
    pub y: [f64; 2],
}
/// Actual (f32-rounded) extent of a sweep, for the slope suprema.
#[allow(dead_code)] // Each operator reads only the fields its bound needs.
pub struct Domain {
    pub rho_min: f64,
    pub y_min: f64,
    pub y_max: f64,
}
pub struct Measured {
    pub max_step: f64,
    pub bound_at_max: f64,
    pub switch_step: f64,
}

pub fn sweeps() -> [Sweep; 2] {
    [
        // rho* = 0.25 * (1 - 0.5) = 0.125 lies mid-sweep.
        Sweep {
            name: "rho 0.10..0.15 at Y=5e-4",
            rho: [0.10, 0.15],
            y: [5e-4, 5e-4],
        },
        // At rho = 0.05 the floor is 1e-3 * (1 - 0.2) = 8e-4, mid-sweep.
        Sweep {
            name: "|Y| 7.5e-4..8.5e-4 at rho=0.05",
            rho: [0.05, 0.05],
            y: [7.5e-4, 8.5e-4],
        },
    ]
}

impl Sweep {
    /// Opposing red/green contributions with zero blue: Y and A are
    /// prescribed independently, so rho = Y / A is whatever is requested.
    pub fn pixels(&self) -> Vec<[f32; 3]> {
        (0..SAMPLES)
            .map(|i| {
                let t = i as f64 / (SAMPLES - 1) as f64;
                let rho = self.rho[0] + (self.rho[1] - self.rho[0]) * t;
                let y = self.y[0] + (self.y[1] - self.y[0]) * t;
                let a = y / rho;
                [
                    ((a + y) / (2. * W[0])) as f32,
                    ((y - a) / (2. * W[1])) as f32,
                    0.,
                ]
            })
            .collect()
    }

    /// `inputs` are the conditioned pixels the operator actually sees and
    /// `outputs` its RGB results. `slopes` returns [S_rho, S_y] for `Domain`.
    pub fn check(
        &self,
        operator: &str,
        backend: &str,
        inputs: &[[f32; 3]],
        outputs: &[[f32; 3]],
        slopes: impl Fn(&Domain) -> [f64; 2],
    ) -> Measured {
        assert_eq!(inputs.len(), SAMPLES);
        assert_eq!(outputs.len(), SAMPLES);
        let y: Vec<f64> = inputs.iter().map(|p| luma(*p)).collect();
        let rho: Vec<f64> = inputs
            .iter()
            .zip(&y)
            .map(|(p, y)| y / weighted_abs(*p))
            .collect();
        assert!(y.iter().all(|v| *v > 0.));
        // Independent f64 evaluation of which side of the switch each sample is.
        let floor: Vec<bool> = y
            .iter()
            .zip(&rho)
            .map(|(y, rho)| EPSILON * (1. - rho / K).clamp(0., 1.) > *y)
            .collect();
        let crossings: Vec<usize> = (1..SAMPLES).filter(|i| floor[*i] != floor[i - 1]).collect();
        assert_eq!(
            crossings.len(),
            1,
            "{operator} {backend} {}: the sweep must cross D=|Y| exactly once",
            self.name
        );
        let switch_index = crossings[0];
        assert!(floor[0] && !floor[SAMPLES - 1]);
        // The switch is well inside the sweep: both branches are exercised.
        assert!(switch_index > SAMPLES / 4 && switch_index < 3 * SAMPLES / 4);
        let domain = Domain {
            rho_min: rho.iter().copied().fold(f64::INFINITY, f64::min),
            y_min: y.iter().copied().fold(f64::INFINITY, f64::min),
            y_max: y.iter().copied().fold(0., f64::max),
        };
        let [s_rho, s_y] = slopes(&domain);
        let out_y: Vec<f64> = outputs.iter().map(|p| luma(*p)).collect();
        let rounding: Vec<f64> = outputs
            .iter()
            .map(|p| ROUNDING_UNITS * weighted_abs(*p) / 16_777_216.)
            .collect();
        let mut measured = Measured {
            max_step: 0.,
            bound_at_max: 0.,
            switch_step: 0.,
        };
        for i in 1..SAMPLES {
            let step = (out_y[i] - out_y[i - 1]).abs();
            let bound = s_rho * (rho[i] - rho[i - 1]).abs()
                + s_y * (y[i] - y[i - 1]).abs()
                + rounding[i]
                + rounding[i - 1];
            assert!(
                step <= bound,
                "{operator} {backend} {}: step {step:e} > bound {bound:e} at i={i} \
                 (switch at i={switch_index}); rho {:e}->{:e} Y {:e}->{:e} side {}->{} \
                 Yout {:e}->{:e} inputs {:?}->{:?}",
                self.name,
                rho[i - 1],
                rho[i],
                y[i - 1],
                y[i],
                side(floor[i - 1]),
                side(floor[i]),
                out_y[i - 1],
                out_y[i],
                inputs[i - 1],
                inputs[i]
            );
            if step > measured.max_step {
                measured.max_step = step;
                measured.bound_at_max = bound;
            }
            if i == switch_index {
                measured.switch_step = step;
            }
        }
        eprintln!(
            "ENG3f {operator} {backend} {}: max_step={:e} (bound there {:e}) \
             switch_step={:e} at i={switch_index} rho={:e} Y={:e} S_rho={s_rho:e} S_y={s_y:e} \
             Yout first/last={:e}/{:e}",
            self.name,
            measured.max_step,
            measured.bound_at_max,
            measured.switch_step,
            rho[switch_index],
            y[switch_index],
            out_y[0],
            out_y[SAMPLES - 1]
        );
        measured
    }
}
fn side(floor: bool) -> &'static str {
    if floor { "floor" } else { "ratio" }
}
