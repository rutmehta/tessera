//! DDColor paper-tiny adapter; see MODELS.md for the pinned model contract.
use crate::color::{lab_to_rgb, pixel_count, rgb_to_lab, sample};
use crate::{Cancel, NeuralFilter, ParamSchema, Params, render, unit, validate};
use anyhow::{Context, Result, ensure};
use compositor::raster::Raster;
use ml_runtime::{
    Dtype, ExecutionPreference, ModelRegistry, PartitionReport, Session, SessionOptions,
    TensorInput, TensorOutput,
};
use std::sync::Mutex;

const SIDE: usize = 512;
const PLANE: usize = SIDE * SIDE;

/// A local chroma constraint; coordinates place pixel (0,0) at the first sample.
#[derive(Clone, Debug)]
pub struct ColorHint {
    pub position: [f32; 2],
    /// Normalized display-sRGB. Its L is deliberately ignored.
    pub rgb: [f32; 3],
    pub radius: f32,
    pub strength: f32,
}

pub struct Colorize {
    session: Mutex<Session>,
}
impl Colorize {
    /// This approved export runs explicitly on CPU. CoreML preferences are
    /// overridden only for DDColor, whose graph fails the strict partition guard.
    pub fn load(registry: &ModelRegistry, options: SessionOptions) -> Result<Self> {
        let handle = registry.resolve("filters/ddcolor")?;
        let spec = handle.spec();
        ensure!(
            spec.inputs.len() == 1 && spec.outputs.len() == 1,
            "DDColor requires one input/output"
        );
        ensure!(
            spec.inputs[0].name == "input"
                && spec.inputs[0].shape == [1, 3, SIDE, SIDE]
                && spec.inputs[0].dtype == Dtype::Fp32,
            "unsupported DDColor input contract"
        );
        ensure!(
            spec.outputs[0].name == "output"
                && spec.outputs[0].shape == [1, 2, SIDE, SIDE]
                && spec.outputs[0].dtype == Dtype::Fp32,
            "unsupported DDColor output contract"
        );
        Ok(Self {
            session: Mutex::new(Session::load(
                handle.path(),
                options.with_execution_preference(ExecutionPreference::CpuOnly),
            )?),
        })
    }
    /// Call after representative inference. This does not assert CoreML coverage.
    pub fn partition_report(&self) -> Result<PartitionReport> {
        self.session
            .lock()
            .map_err(|_| anyhow::anyhow!("DDColor session poisoned"))?
            .partition_report()
    }
}
pub(crate) const SCHEMA: &[ParamSchema] = &[
    ParamSchema {
        name: "Artifact Reduction",
        min: 0.0,
        max: 1.0,
        default: 0.0,
    },
    ParamSchema {
        name: "Saturation",
        min: 0.0,
        max: 2.0,
        default: 1.0,
    },
];
impl NeuralFilter for Colorize {
    fn name(&self) -> &'static str {
        "Colorize"
    }
    fn requires_weights(&self) -> bool {
        true
    }
    fn params_schema(&self) -> &'static [ParamSchema] {
        SCHEMA
    }
    fn apply(&self, input: &Raster, p: &Params, cancel: &Cancel) -> Result<Raster> {
        validate(input, cancel)?;
        validate_params(p)?;
        let (l, tensor) = preprocess(input, cancel)?;
        let outputs = {
            let mut session = self
                .session
                .lock()
                .map_err(|_| anyhow::anyhow!("DDColor session poisoned"))?;
            cancel.check()?;
            // Runtime currently cannot interrupt an in-flight ORT call. Never
            // publish cancelled work: check immediately after it completes.
            session.run_tensors(&[("input", tensor)])?
        };
        cancel.check()?;
        let ab = decode_output(&outputs)?;
        postprocess(input, &l, &ab, p, cancel)
    }
}

fn validate_params(p: &Params) -> Result<()> {
    unit(p.artifact_reduction)?;
    ensure!(
        p.saturation.is_finite() && (0.0..=2.0).contains(&p.saturation),
        "saturation must be 0..2"
    );
    for hint in &p.hints {
        ensure!(
            hint.position.iter().all(|v| v.is_finite()),
            "hint position must be finite"
        );
        ensure!(
            hint.radius.is_finite() && hint.radius > 0.0,
            "hint radius must be positive and finite"
        );
        unit(hint.strength)?;
        for &v in &hint.rgb {
            unit(v)?;
        }
    }
    Ok(())
}

fn preprocess(input: &Raster, cancel: &Cancel) -> Result<(Vec<f32>, TensorInput)> {
    let w = input.extent().width as usize;
    let h = input.extent().height as usize;
    let n = pixel_count(w, h).context("invalid image dimensions")?;
    let mut rgb = Vec::with_capacity(n);
    let mut l = Vec::with_capacity(n);
    for y in 0..h {
        cancel.check()?;
        for x in 0..w {
            let px = input.pixel(x as u32, y as u32);
            let c = [px[0], px[1], px[2]];
            rgb.push(c);
            l.push(rgb_to_lab(c)[0]);
        }
    }
    let mut data = vec![0.0; 3 * PLANE];
    for y in 0..SIDE {
        cancel.check()?;
        for x in 0..SIDE {
            let small = sample(&rgb, w, h, x, y, SIDE, SIDE);
            let neutral = lab_to_rgb([rgb_to_lab(small)[0], 0.0, 0.0]);
            for c in 0..3 {
                data[c * PLANE + y * SIDE + x] = neutral[c].clamp(0.0, 1.0);
            }
        }
    }
    Ok((
        l,
        TensorInput::F32 {
            shape: vec![1, 3, SIDE, SIDE],
            data,
        },
    ))
}

fn decode_output(outputs: &[TensorOutput]) -> Result<Vec<[f32; 2]>> {
    ensure!(outputs.len() == 1, "DDColor output count mismatch");
    let out = &outputs[0];
    ensure!(
        out.name == "output" && out.shape == [1, 2, SIDE, SIDE] && out.data.len() == 2 * PLANE,
        "DDColor output shape/name mismatch"
    );
    // Extreme finite values are invalid Lab and could overflow reconstruction.
    ensure!(
        out.data.iter().all(|v| v.is_finite() && v.abs() <= 1000.0),
        "invalid DDColor chroma"
    );
    Ok((0..PLANE)
        .map(|i| [out.data[i], out.data[PLANE + i]])
        .collect())
}

fn postprocess(
    input: &Raster,
    l: &[f32],
    model_ab: &[[f32; 2]],
    p: &Params,
    cancel: &Cancel,
) -> Result<Raster> {
    let w = input.extent().width as usize;
    let h = input.extent().height as usize;
    let n = pixel_count(w, h).context("invalid image dimensions")?;
    ensure!(
        l.len() == n && model_ab.len() == PLANE,
        "DDColor reconstruction shape mismatch"
    );
    let mut ab = Vec::with_capacity(n);
    for y in 0..h {
        cancel.check()?;
        for x in 0..w {
            ab.push(sample(model_ab, SIDE, SIDE, x, y, w, h));
        }
    }
    if p.artifact_reduction > 0.0 {
        ab = reduce_artifacts(&ab, l, w, h, p.artifact_reduction, cancel)?;
    }
    // Hints follow smoothing so the brush cannot leak beyond its finite radius.
    for hint in &p.hints {
        apply_hint(&mut ab, w, h, hint, cancel)?;
    }
    render(input, cancel, |x, y, px| {
        let i = y as usize * w + x as usize;
        let rgb = lab_to_rgb([l[i], ab[i][0] * p.saturation, ab[i][1] * p.saturation]);
        for c in 0..3 {
            px[c] = rgb[c].clamp(0.0, 1.0);
        }
        // Alpha is never passed through the neural graph or reconstructed.
    })
}

fn apply_hint(
    ab: &mut [[f32; 2]],
    w: usize,
    h: usize,
    hint: &ColorHint,
    cancel: &Cancel,
) -> Result<()> {
    let lab = rgb_to_lab(hint.rgb);
    for y in 0..h {
        cancel.check()?;
        for x in 0..w {
            // f64 prevents squared radii/positions overflowing even for finite f32.
            let dx = (x as f64 - hint.position[0] as f64) / hint.radius as f64;
            let dy = (y as f64 - hint.position[1] as f64) / hint.radius as f64;
            let falloff = (1.0 - dx * dx - dy * dy).max(0.0);
            let t = (falloff * falloff) as f32 * hint.strength;
            if t > 0.0 {
                for c in 0..2 {
                    ab[y * w + x][c] = ab[y * w + x][c] * (1.0 - t) + lab[c + 1] * t;
                }
            }
        }
    }
    Ok(())
}

/// Joint bilateral 3x3 on chroma only, guided by the untouched full-resolution L.
fn reduce_artifacts(
    ab: &[[f32; 2]],
    l: &[f32],
    w: usize,
    h: usize,
    amount: f32,
    cancel: &Cancel,
) -> Result<Vec<[f32; 2]>> {
    ensure!(
        pixel_count(w, h) == Some(ab.len()) && l.len() == ab.len(),
        "invalid chroma smoothing shape"
    );
    let mut out = ab.to_vec();
    for y in 0..h {
        cancel.check()?;
        for x in 0..w {
            let i = y * w + x;
            let mut sum = [0.0; 2];
            let mut total = 0.0;
            for yy in y.saturating_sub(1)..=(y + 1).min(h - 1) {
                for xx in x.saturating_sub(1)..=(x + 1).min(w - 1) {
                    let j = yy * w + xx;
                    let spatial = (xx as f32 - x as f32).powi(2) + (yy as f32 - y as f32).powi(2);
                    let weight = (-spatial / 2.0 - (l[j] - l[i]).powi(2) / 50.0).exp();
                    total += weight;
                    for c in 0..2 {
                        sum[c] += weight * ab[j][c];
                    }
                }
            }
            for c in 0..2 {
                out[i][c] = ab[i][c] * (1.0 - amount) + sum[c] / total * amount;
            }
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use compositor::{geom::Rect, raster::Depth};
    use engine_api::tile::Extent;

    #[test]
    fn preprocessing_resizes_rgb_then_neutralizes_lab() -> Result<()> {
        let mut src = Raster::new(Extent::new(2, 1), 4, Depth::F32, 0.0);
        src.edit_region(Rect::new(0, 0, 2, 1), 1, |x, _, p| {
            *p = if x == 0 {
                [1.0, 0.0, 0.0, 0.2]
            } else {
                [0.0, 0.0, 1.0, 0.8]
            }
        })?;
        let (l, tensor) = preprocess(&src, &Cancel::new())?;
        assert!((l[0] - 53.2408).abs() < 0.002);
        assert!((l[1] - 32.297).abs() < 0.002);
        let expected = lab_to_rgb([
            rgb_to_lab(sample(
                &[[1.0, 0.0, 0.0], [0.0, 0.0, 1.0]],
                2,
                1,
                256,
                0,
                SIDE,
                SIDE,
            ))[0],
            0.0,
            0.0,
        ]);
        let TensorInput::F32 { shape, data } = tensor else {
            panic!("wrong type")
        };
        assert_eq!(shape, [1, 3, SIDE, SIDE]);
        for c in 0..3 {
            assert!((data[c * PLANE + 256] - expected[c]).abs() < 1e-6);
        }
        Ok(())
    }
    #[test]
    fn reconstruction_keeps_full_resolution_lightness_and_alpha() -> Result<()> {
        let mut src = Raster::new(Extent::new(17, 3), 4, Depth::F32, 0.0);
        src.edit_region(Rect::new(0, 0, 17, 3), 1, |x, y, p| {
            let gray = 0.3 + (x % 2) as f32 * 0.3 + y as f32 * 0.05;
            *p = [gray, gray, gray, x as f32 / 16.0];
        })?;
        let (l, _) = preprocess(&src, &Cancel::new())?;
        let ab = vec![[4.0, -3.0]; PLANE];
        for saturation in [0.0, 1.0, 2.0] {
            let p = Params {
                saturation,
                artifact_reduction: 1.0,
                ..Params::default()
            };
            let out = postprocess(&src, &l, &ab, &p, &Cancel::new())?;
            for y in 0..3 {
                for x in 0..17 {
                    let px = out.pixel(x, y);
                    assert_eq!(px[3], src.pixel(x, y)[3]);
                    assert!(
                        (rgb_to_lab([px[0], px[1], px[2]])[0] - l[y as usize * 17 + x as usize])
                            .abs()
                            < 0.0001
                    );
                    if saturation == 0.0 {
                        assert!((px[0] - px[2]).abs() < 0.00001);
                    }
                }
            }
        }
        assert!(postprocess(&src, &l[..1], &ab, &Params::default(), &Cancel::new()).is_err());
        Ok(())
    }
    #[test]
    fn hints_are_local_chroma_only_and_strength_controlled() -> Result<()> {
        let mut ab = vec![[0.0; 2]; 9];
        let hint = ColorHint {
            position: [4.0, 0.0],
            rgb: [0.7, 0.3, 0.4],
            radius: 2.0,
            strength: 1.0,
        };
        apply_hint(&mut ab, 9, 1, &hint, &Cancel::new())?;
        let expected = rgb_to_lab(hint.rgb);
        assert_eq!(ab[4], [expected[1], expected[2]]);
        assert!(ab[3][0] > 0.0 && ab[3][0] < ab[4][0]);
        for i in [0, 1, 2, 6, 7, 8] {
            assert_eq!(ab[i], [0.0; 2]);
        }
        let original = ab.clone();
        apply_hint(
            &mut ab,
            9,
            1,
            &ColorHint {
                strength: 0.0,
                ..hint
            },
            &Cancel::new(),
        )?;
        assert_eq!(original, ab);
        Ok(())
    }
    #[test]
    fn artifact_reduction_smooths_chroma_without_crossing_l_edges() -> Result<()> {
        let ab = vec![
            [0.0, 0.0],
            [20.0, 10.0],
            [0.0, 0.0],
            [60.0, 40.0],
            [60.0, 40.0],
        ];
        let l = vec![20.0, 20.0, 20.0, 90.0, 90.0];
        let out = reduce_artifacts(&ab, &l, 5, 1, 1.0, &Cancel::new())?;
        assert!(out[1][0] < ab[1][0]);
        assert!((out[3][0] - 60.0).abs() < 1e-5);
        assert_eq!(reduce_artifacts(&ab, &l, 5, 1, 0.0, &Cancel::new())?, ab);
        let cancel = Cancel::new();
        cancel.cancel();
        assert!(reduce_artifacts(&ab, &l, 5, 1, 1.0, &cancel).is_err());
        Ok(())
    }
    #[test]
    fn malformed_output_and_parameters_fail_closed() -> Result<()> {
        assert!(decode_output(&[]).is_err());
        let mut tensor = TensorOutput {
            name: "output".into(),
            shape: vec![1, 2, SIDE, SIDE],
            data: vec![0.0; 2 * PLANE],
        };
        assert_eq!(decode_output(&[tensor.clone()])?.len(), PLANE);
        tensor.shape[1] = 3;
        assert!(decode_output(&[tensor.clone()]).is_err());
        tensor.shape[1] = 2;
        tensor.data[0] = f32::NAN;
        assert!(decode_output(&[tensor.clone()]).is_err());
        tensor.data[0] = f32::MAX;
        assert!(decode_output(&[tensor.clone()]).is_err());
        tensor.data.pop();
        assert!(decode_output(&[tensor]).is_err());
        for value in [-0.1, 2.1, f32::NAN, f32::INFINITY] {
            assert!(
                validate_params(&Params {
                    saturation: value,
                    ..Params::default()
                })
                .is_err()
            );
        }
        assert!(
            validate_params(&Params {
                artifact_reduction: 1.1,
                ..Params::default()
            })
            .is_err()
        );
        let hint = ColorHint {
            position: [0.0, 0.0],
            rgb: [0.5; 3],
            radius: 1.0,
            strength: 1.0,
        };
        for invalid in [
            ColorHint {
                radius: 0.0,
                ..hint.clone()
            },
            ColorHint {
                position: [f32::NAN, 0.0],
                ..hint.clone()
            },
            ColorHint {
                rgb: [2.0; 3],
                ..hint.clone()
            },
            ColorHint {
                strength: -1.0,
                ..hint
            },
        ] {
            assert!(
                validate_params(&Params {
                    hints: vec![invalid],
                    ..Params::default()
                })
                .is_err()
            );
        }
        Ok(())
    }
}
