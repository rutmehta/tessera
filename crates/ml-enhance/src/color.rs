//! Reversible calibrated camera-linear ↔ white-balanced linear sRGB.
use anyhow::{Result, ensure};
use engine_api::color::{ChromaticAdaptation, ColorMatrix3, WhitePoint, WorkingSpace};
use ml_runtime::Tensor;

pub struct CameraSrgb {
    forward: ColorMatrix3,
    inverse: ColorMatrix3,
}
impl CameraSrgb {
    /// ColorMatrix is XYZ(D65)→camera, and neutral is DNG AsShotNeutral.
    /// Adapt the inferred scene white in XYZ, never independently normalize
    /// calibrated matrix rows. The inverse removes this temporary white balance
    /// so the output DNG can retain its original matrix and neutral tags.
    pub fn new(matrix: [[f64; 3]; 3], neutral: [f64; 3]) -> Result<Self> {
        ensure!(
            matrix.iter().flatten().all(|v| v.is_finite())
                && neutral.iter().all(|v| v.is_finite() && *v > 0.0),
            "invalid camera calibration"
        );
        let camera_xyz = ColorMatrix3(matrix).inverse()?;
        let white = camera_xyz.apply(neutral);
        ensure!(
            white.iter().all(|v| v.is_finite() && *v > 0.0),
            "unsupported camera white point"
        );
        let sum: f64 = white.iter().sum();
        let source = WhitePoint::new(white[0] / sum, white[1] / sum);
        let adapt = ChromaticAdaptation::Bradford.matrix(source, WhitePoint::D65)?;
        let xyz_srgb =
            ColorMatrix3::rgb_to_xyz(&WorkingSpace::LinearSrgb.primaries())?.inverse()?;
        let forward = xyz_srgb * adapt * camera_xyz;
        let inverse = forward.inverse()?;
        Ok(Self { forward, inverse })
    }
    pub fn to_srgb(&self, camera: &Tensor) -> Result<Tensor> {
        let rgb = transform(camera, self.forward)?;
        ensure!(
            rgb.data().iter().all(|v| (0.0..=1.0).contains(v)),
            "enhancement requires bounded linear sRGB; HDR, negative and out-of-sRGB-gamut samples are unsupported (no clipping applied)"
        );
        Ok(rgb)
    }
    pub fn to_camera(&self, rgb: &Tensor) -> Result<Tensor> {
        transform(rgb, self.inverse)
    }
}
fn transform(input: &Tensor, matrix: ColorMatrix3) -> Result<Tensor> {
    let [_, c, h, w] = input.shape();
    ensure!(
        c == 3 && input.data().iter().all(|v| v.is_finite()),
        "finite RGB tensor required"
    );
    let n = h * w;
    let mut output = vec![0.0; 3 * n];
    for i in 0..n {
        let rgb = matrix.apply(std::array::from_fn(|c| input.data()[c * n + i] as f64));
        for c in 0..3 {
            output[c * n + i] = rgb[c] as f32;
        }
    }
    ensure!(
        output.iter().all(|v| v.is_finite()),
        "nonfinite transformed RGB"
    );
    Tensor::new(3, h, w, output)
}
