//! Ordered DNG corrections in their declared sensor/linear/post-colour stages.
#[cfg(test)]
#[path = "embedded_lens_tests.rs"]
mod tests;
use engine_api::{EngineError, EngineResult};
use lens::opcodes::{CorrectionOpcode, FixVignetteRadial, WarpRectilinear, parse_opcode_list};
use raw_decode::RawMetadata;
#[derive(Clone, Debug, Default)]
pub(crate) struct Embedded {
    pub stages: [Vec<CorrectionOpcode>; 3],
    pub warps: Vec<WarpRectilinear>,
    pub gains: Vec<FixVignetteRadial>,
    /// ENG-8c: a camera's maker-note built-in correction (raws without opcode
    /// lists), applied in the raw prefix like an opcode stage: one resample of
    /// demosaiced camera RGB with its distortion, lateral CA and vignetting
    /// ([`MakerPrefix`]).
    pub maker: Option<MakerPrefix>,
    size: [f64; 2],
    crop: [f64; 4],
}

/// The maker-note correction as one raw-prefix resample, in the active-area
/// [-1, 1] frame (`crate::maker_lens`). For output point p (green):
/// q = p·(1 + d·(F(r) − 1)), F = s + o₀r + k₁r² + o₁r³ + k₂r⁴ + k₃r⁶ with r the
/// half-diagonal radius of p; red/blue: q·(1 + c·(c₀ + c₁ρ² + c₂ρ⁴ − 1)) with
/// ρ the radius of the green source q; then the sample at the channel's source
/// times the vignetting gain there, 1 + v·(1/I − 1), I = 1 + v₀ρ² + v₁ρ⁴ +
/// v₂ρ⁶ (both clamped to 1/8..8 like profile vignetting). d, c and v are the
/// distortion, CA and vignetting amounts. The resident GPU kernel evaluates
/// the same expressions.
#[derive(Clone, Debug, PartialEq)]
pub struct MakerPrefix {
    /// `[s, o₀, k₁, o₁, k₂, k₃]`.
    pub distortion: [f64; 6],
    pub red: [f64; 3],
    pub blue: [f64; 3],
    pub vignette: [f64; 3],
    /// Axis multipliers from [-1, 1] coordinates to half-diagonal units.
    pub coordinate_scale: [f64; 2],
}

impl MakerPrefix {
    pub(crate) fn from_sample(s: &lens::CalibrationSample) -> Self {
        Self {
            distortion: [
                s.distortion_scale,
                s.radial_odd[0],
                s.distortion.k1,
                s.radial_odd[1],
                s.distortion.k2,
                s.distortion.k3,
            ],
            red: s.ca_red,
            blue: s.ca_blue,
            vignette: s.vignette,
            coordinate_scale: s.coordinate_scale,
        }
    }
    fn radius2(&self, q: [f64; 2]) -> f64 {
        let x = q[0] * self.coordinate_scale[0];
        let y = q[1] * self.coordinate_scale[1];
        x * x + y * y
    }
    /// Source position of `channel` for output point `p`, with the
    /// distortion and CA amounts.
    pub fn map(&self, p: [f64; 2], channel: usize, amounts: [f64; 2]) -> [f64; 2] {
        let r2 = self.radius2(p);
        let r = r2.sqrt();
        let d = self.distortion;
        let f = d[0] + r * (d[1] + r * (d[2] + r * (d[3] + r * (d[4] + r * r * d[5]))));
        let g = 1. + amounts[0] * (f - 1.);
        let q = [p[0] * g, p[1] * g];
        if channel == 1 {
            return q;
        }
        let c = if channel == 0 { self.red } else { self.blue };
        let rho2 = self.radius2(q);
        let ratio = c[0] + rho2 * (c[1] + rho2 * c[2]);
        let k = 1. + amounts[1] * (ratio - 1.);
        [q[0] * k, q[1] * k]
    }
    /// Vignetting gain at source point `q`, with the vignetting amount.
    pub fn gain(&self, q: [f64; 2], amount: f64) -> f64 {
        let r2 = self.radius2(q);
        let v = self.vignette;
        let illumination = (1. + r2 * (v[0] + r2 * (v[1] + r2 * v[2]))).clamp(0.125, 8.);
        (1. + (1. / illumination - 1.) * amount).clamp(0.125, 8.)
    }
}
pub(crate) fn sample_phase(
    image: &crate::Image,
    plane: usize,
    q: [f64; 2],
    phase: [u32; 2],
    step: u32,
) -> f64 {
    let [px, py] = phase;
    let (w, h) = (
        (image.width() - 1 - px) / step,
        (image.height() - 1 - py) / step,
    );
    let u = ((q[0] - px as f64) / step as f64).clamp(0., w as f64);
    let v = ((q[1] - py as f64) / step as f64).clamp(0., h as f64);
    let (a, b) = (u.floor() as u32, v.floor() as u32);
    let at = |x: u32, y: u32| {
        image.planes()[plane]
            [((y.min(h) * step + py) * image.width() + x.min(w) * step + px) as usize]
            as f64
    };
    (at(a, b) * (1. - u.fract()) + at(a + 1, b) * u.fract()) * (1. - v.fract())
        + (at(a, b + 1) * (1. - u.fract()) + at(a + 1, b + 1) * u.fract()) * v.fract()
}
impl Embedded {
    pub fn parse(m: &RawMetadata) -> EngineResult<Self> {
        let mut out = Self {
            size: [m.width as f64, m.height as f64],
            crop: m.default_crop.map(f64::from),
            ..Default::default()
        };
        // Opcode lists win; the two are never combined (ENG-8).
        if m.opcode_lists.iter().all(Option::is_none) {
            out.maker = crate::maker_lens::sample(m).map(|s| MakerPrefix::from_sample(&s));
        }
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
                if !matches!(id, 1 | 3 | 4 | 5 | 9) && flags & 1 == 0 {
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
                if let CorrectionOpcode::WarpRectilinear(w) = &op.correction
                    && w.coefficients.len() == 2
                {
                    return Err(EngineError::Unsupported {
                        what: "two-plane warp".into(),
                    });
                }
                out.stages[stage].push(op.correction);
            }
        }
        Ok(out)
    }
    /// Execute each opcode in file order, in the full sensor coordinate frame.
    /// CFA resampling stays on the destination's exact phase lattice.
    pub fn apply(
        &self,
        mut image: crate::Image,
        stage: usize,
        cfa: Option<raw_decode::CfaLayout>,
        s: &engine_api::recipe::settings::LensSettings,
    ) -> EngineResult<crate::Image> {
        for op in &self.stages[stage] {
            let mut planes = image.planes().to_vec();
            if let CorrectionOpcode::GainMap(g) = op
                && g.plane + g.planes > planes.len() as u32
            {
                return Err(EngineError::invalid(
                    "DNG GainMap",
                    "plane range exceeds stage image",
                ));
            }
            for (plane, dst) in planes.iter_mut().enumerate() {
                for y in 0..image.height() {
                    for x in 0..image.width() {
                        let i = (y * image.width() + x) as usize;
                        let p = [
                            2. * (x as f64 + 0.5 - self.crop[0]) / self.crop[2] - 1.,
                            2. * (y as f64 + 0.5 - self.crop[1]) / self.crop[3] - 1.,
                        ];
                        let value = match op {
                            CorrectionOpcode::WarpRectilinear(w) => {
                                let channel = cfa.map_or(plane, |c| {
                                    let n = c.channel_at(x, y);
                                    if n == 3 { 1 } else { n }
                                });
                                let green = self.warp(p, w, 1);
                                let chroma = self.warp(p, w, channel);
                                let amount = s.distortion_scale.clamp(0., 200.) as f64 / 100.;
                                // Built-in corrections apply in full, like
                                // Lightroom; the Remove CA switch governs only
                                // estimated and profile CA (ENG-7b).
                                let ca = s.chromatic_aberration_scale.clamp(0., 200.) as f64 / 100.;
                                let q: [f64; 2] = std::array::from_fn(|j| {
                                    p[j] + amount * (green[j] - p[j]) + ca * (chroma[j] - green[j])
                                });
                                let sx = (q[0] + 1.) * self.crop[2] / 2. + self.crop[0] - 0.5;
                                let sy = (q[1] + 1.) * self.crop[3] / 2. + self.crop[1] - 0.5;
                                if !sx.is_finite() || !sy.is_finite() {
                                    return Err(EngineError::invalid(
                                        "DNG warp",
                                        "nonfinite source coordinate",
                                    ));
                                }
                                let step = match cfa {
                                    Some(raw_decode::CfaLayout::Bayer(_)) => 2,
                                    Some(raw_decode::CfaLayout::XTrans(_)) => 6,
                                    _ => 1,
                                };
                                sample_phase(&image, plane, [sx, sy], [x % step, y % step], step)
                            }
                            CorrectionOpcode::FixVignetteRadial(v) => {
                                let (q, _, _) = self.metric(p, v.center);
                                let r = q[0] * q[0] + q[1] * q[1];
                                let gain =
                                    1. + r * v.coefficients.iter().rev().fold(0., |a, k| a * r + k);
                                image.planes()[plane][i] as f64
                                    * (1.
                                        + (gain - 1.) * s.vignetting_scale.clamp(0., 200.) as f64
                                            / 100.)
                            }
                            CorrectionOpcode::GainMap(g) => {
                                let [top, left, bottom, right] = g.area;
                                if y < top
                                    || y >= bottom
                                    || x < left
                                    || x >= right
                                    || (y - top) % g.pitch[0] != 0
                                    || (x - left) % g.pitch[1] != 0
                                    || (plane as u32) < g.plane
                                    || plane as u32 >= g.plane + g.planes
                                {
                                    continue;
                                }
                                let v = (((y as f64 + 0.5) / self.size[1] - g.origin[0])
                                    / g.spacing[0])
                                    .clamp(0., (g.points[0] - 1) as f64);
                                let u = (((x as f64 + 0.5) / self.size[0] - g.origin[1])
                                    / g.spacing[1])
                                    .clamp(0., (g.points[1] - 1) as f64);
                                let mp = if g.map_planes == 1 {
                                    0
                                } else {
                                    plane as u32 - g.plane
                                };
                                let at = |yy: u32, xx: u32| {
                                    g.gains[((yy.min(g.points[0] - 1) * g.points[1]
                                        + xx.min(g.points[1] - 1))
                                        * g.map_planes
                                        + mp) as usize] as f64
                                };
                                let (a, b) = (u.floor() as u32, v.floor() as u32);
                                let gain = (at(b, a) * (1. - u.fract()) + at(b, a + 1) * u.fract())
                                    * (1. - v.fract())
                                    + (at(b + 1, a) * (1. - u.fract())
                                        + at(b + 1, a + 1) * u.fract())
                                        * v.fract();
                                image.planes()[plane][i] as f64 * gain
                            }
                        };
                        if !value.is_finite() || value.abs() > f32::MAX as f64 {
                            return Err(EngineError::invalid("DNG opcode", "nonfinite output"));
                        }
                        dst[i] = value as f32;
                    }
                }
            }
            image = crate::Image::new(image.width(), image.height(), planes)?;
        }
        Ok(image)
    }
    pub fn present(&self) -> bool {
        self.stages.iter().any(|s| !s.is_empty())
            || !self.warps.is_empty()
            || !self.gains.is_empty()
            || self.maker.is_some()
    }
    /// Only a maker-note prefix correction (no opcode stages or warps).
    pub fn maker_only(&self) -> bool {
        self.maker.is_some()
            && self.stages.iter().all(Vec::is_empty)
            && self.warps.is_empty()
            && self.gains.is_empty()
    }
    /// Apply the maker-note correction to demosaiced camera RGB in the full
    /// sensor frame (ENG-8c), where opcode stages run. Rows run in parallel;
    /// every sample is computed independently (deterministic).
    pub fn apply_maker(
        &self,
        image: crate::Image,
        s: &engine_api::recipe::settings::LensSettings,
    ) -> EngineResult<crate::Image> {
        let Some(maker) = &self.maker else {
            return Ok(image);
        };
        let amounts = [
            s.distortion_scale.clamp(0., 200.) as f64 / 100.,
            s.chromatic_aberration_scale.clamp(0., 200.) as f64 / 100.,
        ];
        let vignette = s.vignetting_scale.clamp(0., 200.) as f64 / 100.;
        if amounts == [0.; 2] && vignette == 0. {
            return Ok(image);
        }
        let (w, h) = (image.width() as usize, image.height() as usize);
        let crop = self.crop;
        let workers = std::thread::available_parallelism()
            .map_or(1, usize::from)
            .clamp(1, 16);
        let rows_per = h.div_ceil(workers).max(1);
        let mut planes = vec![vec![0f32; w * h]; image.planes().len()];
        let image = &image;
        let failed = std::sync::atomic::AtomicBool::new(false);
        std::thread::scope(|scope| {
            let mut chunks: Vec<_> = planes
                .iter_mut()
                .map(|p| p.chunks_mut(rows_per * w).collect::<Vec<_>>())
                .collect();
            for band in 0..h.div_ceil(rows_per) {
                let mut outs: Vec<&mut [f32]> = chunks
                    .iter_mut()
                    .map(|c| std::mem::take(&mut c[band]))
                    .collect();
                let failed = &failed;
                scope.spawn(move || {
                    let y0 = band * rows_per;
                    for (c, out) in outs.iter_mut().enumerate() {
                        for (i, dst) in out.iter_mut().enumerate() {
                            let (x, y) = (i % w, y0 + i / w);
                            let p = [
                                2. * (x as f64 + 0.5 - crop[0]) / crop[2] - 1.,
                                2. * (y as f64 + 0.5 - crop[1]) / crop[3] - 1.,
                            ];
                            let q = maker.map(p, c.min(2), amounts);
                            let sx = (q[0] + 1.) * crop[2] / 2. + crop[0] - 0.5;
                            let sy = (q[1] + 1.) * crop[3] / 2. + crop[1] - 0.5;
                            let v = sample_phase(image, c, [sx, sy], [0, 0], 1)
                                * maker.gain(q, vignette);
                            if !v.is_finite() || v.abs() > f32::MAX as f64 {
                                failed.store(true, std::sync::atomic::Ordering::Relaxed);
                            }
                            *dst = v as f32;
                        }
                    }
                });
            }
        });
        if failed.into_inner() {
            return Err(EngineError::invalid(
                "maker-note lens correction",
                "nonfinite output",
            ));
        }
        crate::Image::new(w as u32, h as u32, planes)
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
            // Built-in per-plane warps apply with the CA scale whatever the
            // Remove CA switch (ENG-7b); common geometry passes a zero scale.
            let ca = s.chromatic_aberration_scale.clamp(0., 200.) as f64 / 100.;
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
