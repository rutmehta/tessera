use engine_api::{
    EngineError, EngineResult,
    color::{ChromaticAdaptation, ColorMatrix3, WorkingSpace},
    recipe::settings::{GamutMapping, HighlightReconstruction},
    tile::TileLayout,
};
use image_core::Op;
use pipeline_cpu::DemosaicAlgorithm;
use raw_decode::CfaLayout;

pub(crate) fn parameters(
    op: &Op<'_>,
    input: TileLayout,
    origin: (u32, u32),
) -> EngineResult<(Vec<f32>, TileLayout)> {
    let mut p = vec![0.0; 33];
    let mut out = input;
    p[1] = input.extent.width as f32;
    p[2] = input.extent.height as f32;
    p[3] = input.halo as f32;
    p[5] = input.plane_len() as f32;
    // Only parity and dither phase matter; avoid f32 loss for large origins.
    p[7] = (origin.0 % 4) as f32;
    p[8] = (origin.1 % 4) as f32;
    let mut matrix = None;
    match *op {
        Op::Detail(_) | Op::Geometry(_) | Op::Effects(..) | Op::EffectsInCrop(..) => {
            return Err(EngineError::invalid(
                "GPU operator",
                "requires CPU fallback",
            ));
        }
        Op::ToneExtra(s) => crate::curves::parameters(s, &mut p)?,
        Op::Color(s) => crate::color::parameters(s, &mut p)?,
        Op::Highlights { cfa, .. } | Op::Demosaic { cfa, .. } => {
            if input.channels != 1 {
                return Err(EngineError::invalid("CFA tile", "one plane required"));
            }
            let CfaLayout::Bayer(pattern) = cfa else {
                return Err(EngineError::invalid("CFA", "expected Bayer"));
            };
            let pattern = pattern.map(|r| r.map(|c| if c == 3 { 1 } else { c }));
            let count = |c| pattern.iter().flatten().filter(|&&v| v == c).count();
            if count(0) != 1
                || count(1) != 2
                || count(2) != 1
                || pattern[0][0] + pattern[1][1] != 2
                || pattern[0][1] + pattern[1][0] != 2
            {
                return Err(EngineError::invalid("CFA", "malformed Bayer"));
            }
            for (i, v) in pattern.iter().flatten().enumerate() {
                p[10 + i] = *v as f32;
            }
            let halo = match *op {
                Op::Highlights { mode, .. } => {
                    p[0] = 0.0;
                    match mode {
                        HighlightReconstruction::Clip => 0,
                        HighlightReconstruction::ReconstructColor => {
                            p[9] = 1.0;
                            4
                        }
                        _ => return Err(EngineError::invalid("highlights", "unsupported mode")),
                    }
                }
                Op::Demosaic { algorithm, .. } => {
                    p[0] = 1.0;
                    p[9] = if algorithm == DemosaicAlgorithm::Bilinear {
                        0.0
                    } else {
                        1.0
                    };
                    out.channels = 3;
                    2
                }
                _ => unreachable!(),
            };
            if input.halo < halo {
                return Err(EngineError::invalid("CFA tile", "insufficient halo"));
            }
            out.halo = 0;
        }
        Op::Matrix(m) => {
            p[0] = 2.0;
            matrix = Some(m);
        }
        Op::Tone(s) => {
            p[0] = 3.0;
            let values = [
                s.exposure,
                s.contrast,
                s.highlights,
                s.shadows,
                s.whites,
                s.blacks,
            ];
            if values.iter().any(|v| !v.is_finite()) {
                return Err(EngineError::invalid("tone", "parameters must be finite"));
            }
            p[25] = s.exposure.clamp(-10.0, 10.0).exp2();
            p[26] = (s.contrast.clamp(-100.0, 100.0) / 100.0).exp2();
            for i in 0..4 {
                p[27 + i] = values[2 + i].clamp(-100.0, 100.0) / 100.0;
            }
            p[31] = if values[1..].iter().all(|v| *v == 0.0) {
                1.0
            } else {
                0.0
            };
        }
        Op::Display { gamut, headroom } => {
            p[0] = 4.0;
            p[9] = if gamut == GamutMapping::Clip {
                0.0
            } else {
                1.0
            };
            out.halo = 0;
            matrix = Some(
                WorkingSpace::LinearRec2020
                    .conversion_to(WorkingSpace::LinearSrgb, ChromaticAdaptation::Cat16)?,
            );
            match headroom {
                // SDR: encoded 8-bit output, constants unchanged.
                None => p[32] = (0.18 * (0.18f32.powf(-1.0) - 1.0).powf(1.0 / 1.5)).ln(),
                // EDR: p[10] > 0 selects the linear branch and is its peak.
                Some(h) => {
                    p[10] = pipeline_cpu::sanitize_headroom(h);
                    p[32] = pipeline_cpu::hdr_sigmoid_ln_a(h);
                }
            }
        }
    }
    if p[0] >= 2.0 && input.channels != 3 {
        return Err(EngineError::invalid("RGB", "three planes required"));
    }
    if let Some(ColorMatrix3(m)) = matrix {
        if m.iter().flatten().any(|v| !v.is_finite()) {
            return Err(EngineError::invalid("matrix", "non-finite coefficient"));
        }
        for (i, v) in m.iter().flatten().enumerate() {
            p[16 + i] = *v as f32;
        }
    }
    p[4] = out.halo as f32;
    p[6] = out.plane_len() as f32;
    Ok((p, out))
}
