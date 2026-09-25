//! Supported post-demosaic DNG subset. Never relocate pre-demosaic opcodes.
use engine_api::{EngineError, EngineResult};
use lens::opcodes::{CorrectionOpcode, FixVignetteRadial, WarpRectilinear, parse_opcode_list};
use raw_decode::RawMetadata;
#[derive(Clone, Debug, Default)]
pub(crate) struct Embedded {
    pub warps: Vec<WarpRectilinear>,
    pub gains: Vec<FixVignetteRadial>,
    size: [f64; 2],
    crop: [f64; 4],
}
impl Embedded {
    pub fn parse(m: &RawMetadata) -> EngineResult<Self> {
        let mut out = Self {
            size: [m.width as f64, m.height as f64],
            crop: m.default_crop.map(f64::from),
            ..Default::default()
        };
        for (stage, bytes) in m.opcode_lists.iter().enumerate() {
            let Some(bytes) = bytes else {
                continue;
            };
            let ops = parse_opcode_list(bytes)
                .map_err(|e| EngineError::invalid("DNG opcodes", e.to_string()))?;
            // The lens extractor deliberately skips unknown corrections. A renderer
            // must not silently skip a required operation (including gain maps).
            // Framing is already validated by parse_opcode_list above.
            let count = u32::from_be_bytes(bytes[..4].try_into().unwrap());
            let mut cursor = 4;
            for _ in 0..count {
                let word = |offset| {
                    u32::from_be_bytes(
                        bytes[cursor + offset..cursor + offset + 4]
                            .try_into()
                            .unwrap(),
                    )
                };
                let id = word(0);
                let flags = word(8);
                let length = word(12) as usize;
                // Explicit policy: FixBadPixelsConstant/List are intentionally ignored.
                // Other unknown required operations still fail closed.
                if !matches!(id, 1 | 3 | 4 | 5) && flags & 1 == 0 {
                    return Err(EngineError::Unsupported {
                        what: format!("required DNG opcode {id} is not implemented"),
                    });
                }
                cursor += 16 + length;
            }
            for op in ops {
                if op.minimum_version > 0x01030000 || op.flags & !3 != 0 {
                    if op.flags & 1 != 0 {
                        continue;
                    }
                    return Err(EngineError::Unsupported {
                        what: "DNG opcode version or flags".into(),
                    });
                }
                if stage != 2 {
                    return Err(EngineError::Unsupported {
                        what: format!(
                            "lens correction in OpcodeList{} requires raw-stage execution",
                            stage + 1
                        ),
                    });
                }
                match op.correction {
                    CorrectionOpcode::WarpRectilinear(w) => {
                        if w.coefficients.len() == 2 {
                            return Err(EngineError::Unsupported {
                                what: "two-plane warp on RGB".into(),
                            });
                        }
                        out.warps.push(w);
                    }
                    CorrectionOpcode::FixVignetteRadial(v) => {
                        if !out.warps.is_empty() {
                            return Err(EngineError::Unsupported {
                                what:
                                    "vignette after warp requires stage-coordinate gain composition"
                                        .into(),
                            });
                        }
                        out.gains.push(v);
                    }
                }
            }
        }
        Ok(out)
    }
    pub fn present(&self) -> bool {
        !self.warps.is_empty() || !self.gains.is_empty()
    }
    /// Pixel-space centre and radius of a normalized opcode centre, and the
    /// active-area crop the public [-1, 1] coordinates refer to.
    pub(crate) fn frame(&self, center: [f64; 2]) -> ([f64; 2], f64, [f64; 4]) {
        let c = [center[0] * self.size[0], center[1] * self.size[1]];
        let radius = c[0]
            .max(self.size[0] - c[0])
            .hypot(c[1].max(self.size[1] - c[1]))
            .max(1e-12);
        (c, radius, self.crop)
    }
    fn metric(&self, p: [f64; 2], center: [f64; 2]) -> ([f64; 2], f64, [f64; 2]) {
        let c = [center[0] * self.size[0], center[1] * self.size[1]];
        let radius = c[0]
            .max(self.size[0] - c[0])
            .hypot(c[1].max(self.size[1] - c[1]))
            .max(1e-12);
        let pixel = [
            self.crop[0] + (p[0] + 1.) * self.crop[2] / 2.,
            self.crop[1] + (p[1] + 1.) * self.crop[3] / 2.,
        ];
        (
            [(pixel[0] - c[0]) / radius, (pixel[1] - c[1]) / radius],
            radius,
            c,
        )
    }
    fn warp(&self, p: [f64; 2], w: &WarpRectilinear, channel: usize) -> [f64; 2] {
        let (q, radius, c) = self.metric(p, w.center);
        let k = w.coefficients[if w.coefficients.len() == 1 {
            0
        } else {
            channel
        }];
        let r = q[0] * q[0] + q[1] * q[1];
        let radial = k[0] + r * (k[1] + r * (k[2] + r * k[3]));
        let x = q[0] * radial + 2. * k[4] * q[0] * q[1] + k[5] * (r + 2. * q[0] * q[0]);
        let y = q[1] * radial + k[4] * (r + 2. * q[1] * q[1]) + 2. * k[5] * q[0] * q[1];
        [
            2. * (c[0] + radius * x - self.crop[0]) / self.crop[2] - 1.,
            2. * (c[1] + radius * y - self.crop[1]) / self.crop[3] - 1.,
        ]
    }
    pub fn map(
        &self,
        mut p: [f64; 2],
        channel: usize,
        s: &engine_api::recipe::settings::LensSettings,
    ) -> [f64; 2] {
        // Inverse lookup order is the reverse of opcode execution order.
        for w in self.warps.iter().rev() {
            let green = self.warp(p, w, 1);
            let chroma = self.warp(p, w, channel);
            let distortion = s.distortion_scale.clamp(0., 200.) as f64 / 100.;
            let ca = if s.remove_chromatic_aberration {
                s.chromatic_aberration_scale.clamp(0., 200.) as f64 / 100.
            } else {
                0.
            };
            p = std::array::from_fn(|i| {
                p[i] + distortion * (green[i] - p[i]) + ca * (chroma[i] - green[i])
            });
        }
        p
    }
    pub fn gain(&self, p: [f64; 2]) -> f64 {
        self.gains.iter().fold(1., |gain, v| {
            let (p, _, _) = self.metric(p, v.center);
            let r = p[0] * p[0] + p[1] * p[1];
            let polynomial = v.coefficients.iter().rev().fold(0., |a, k| a * r + k);
            (gain * (1. + r * polynomial)).clamp(0.125, 8.)
        })
    }
}
