//! A resolved lens correction as plain parameters for resident (GPU) export.
//!
//! The scalar reference applies lens corrections in three places, and a
//! resident backend must keep that order to stay within the full-chain
//! tolerance:
//! 1. lateral CA: per-channel radial scale of the demosaiced camera RGB in the
//!    sensor frame, bilinear, before the camera/white-balance matrices
//!    ([`CaPlan`]; channel mixing does not commute with the resample);
//! 2. vignetting: a per-pixel gain in the active-area frame after white
//!    balance, before Detail ([`VignettePlan`]);
//! 3. distortion with Upright/Transform/crop: one composed inverse map with
//!    normalized Lanczos-3 after Effects, before Output ([`MapPlan`]).
//!
//! [`ResolvedLens::plan`] returns `None` for anything a resident backend does
//! not implement (guided/auto Upright, defringe, embedded per-channel warps,
//! database CA on Bayer sensors); callers must then use the reference path.
use crate::{CorrectionSource, ResolvedLens};
use engine_api::{
    EngineResult,
    recipe::{
        DevelopSettings,
        settings::{GeometrySettings, LensSettings, NormalizedRect, UprightMode},
    },
};

/// At most this many embedded DNG warps or gains are ported.
pub const MAX_EMBEDDED: usize = 4;

#[derive(Clone, Debug)]
pub struct LensPlan {
    pub ca: Option<CaPlan>,
    pub vignette: Option<VignettePlan>,
    pub map: Option<MapPlan>,
}

impl LensPlan {
    pub fn is_identity(&self) -> bool {
        self.ca.is_none() && self.vignette.is_none() && self.map.is_none()
    }
}

/// Lateral CA from a calibration sample (never embedded warps).
#[derive(Clone, Debug)]
pub struct CaPlan {
    /// Channel radius / green radius = c0 + c1·r² + c2·r⁴, red then blue.
    pub red: [f64; 3],
    pub blue: [f64; 3],
    pub center: [f64; 2],
    pub coordinate_scale: [f64; 2],
    /// `chromatic_aberration_scale / 100`, clamped like the reference.
    pub amount: f64,
    /// Sensor active area defining the optical [-1, 1] coordinates.
    pub crop: [u32; 4],
    lens: ResolvedLens,
    settings: LensSettings,
}

impl CaPlan {
    /// Reference sample position (sensor pixels, unclamped) for channel 0 or 2
    /// at sensor pixel (x, y), exactly as `optics::lateral_ca` computes it.
    pub fn source(&self, x: u32, y: u32, channel: usize) -> Option<[f64; 2]> {
        let c = self.crop;
        let p = [
            2. * (x as f64 + 0.5 - c[0] as f64) / c[2] as f64 - 1.,
            2. * (y as f64 + 0.5 - c[1] as f64) / c[3] as f64 - 1.,
        ];
        let q = self
            .lens
            .ca_map(p, channel, &self.settings)
            .filter(|q| q.iter().all(|v| v.is_finite()))?;
        Some([
            (q[0] + 1.) * c[2] as f64 / 2. + c[0] as f64 - 0.5,
            (q[1] + 1.) * c[3] as f64 / 2. + c[1] as f64 - 0.5,
        ])
    }

    /// Largest sample displacement over a `width` × `height` sensor, in
    /// pixels (a dense grid including the corners; radial scales are smooth).
    pub fn max_displacement(&self, width: u32, height: u32) -> Option<f64> {
        let mut max = 0f64;
        for j in 0..=32u32 {
            for i in 0..=32u32 {
                let x = (u64::from(width - 1) * u64::from(i) / 32) as u32;
                let y = (u64::from(height - 1) * u64::from(j) / 32) as u32;
                for channel in [0, 2] {
                    let s = self.source(x, y, channel)?;
                    max = max
                        .max((s[0] - x as f64).abs())
                        .max((s[1] - y as f64).abs());
                }
            }
        }
        Some(max)
    }
}

/// Vignetting gains (profile/embedded, then manual), in the active area.
#[derive(Clone, Debug)]
pub struct VignettePlan {
    pub profile: Option<ProfileVignette>,
    /// Manual vignetting: (`amount / 50`, exponent `0.25 + 3.75·midpoint`).
    pub manual: Option<[f64; 2]>,
}

#[derive(Clone, Debug)]
pub struct ProfileVignette {
    /// Relative illumination 1 + v0·r² + v1·r⁴ + v2·r⁶ (identity when zero).
    pub vignette: [f64; 3],
    pub center: [f64; 2],
    pub coordinate_scale: [f64; 2],
    /// `vignetting_scale / 100`.
    pub amount: f64,
    /// Embedded FixVignetteRadial gains, in application order.
    pub embedded: Vec<EmbeddedGain>,
    /// Sensor crop the embedded opcodes' public coordinates refer to.
    pub embedded_crop: [f64; 4],
}

#[derive(Clone, Copy, Debug)]
pub struct EmbeddedGain {
    pub center: [f64; 2],
    pub radius: f64,
    pub coefficients: [f64; 5],
}

/// Channel-independent composed inverse map (lateral CA is applied earlier).
#[derive(Clone, Debug)]
pub struct MapPlan {
    pub crop: NormalizedRect,
    /// Straighten angle in degrees.
    pub angle: f32,
    pub transform: Option<TransformPlan>,
    pub lens: Option<LensMap>,
    resolved: ResolvedLens,
    common: LensSettings,
    geometry: GeometrySettings,
}

#[derive(Clone, Copy, Debug)]
pub struct TransformPlan {
    /// Offsets in normalized units (`offset / 50`).
    pub offset: [f64; 2],
    /// Rotation sine and cosine.
    pub rotate: [f64; 2],
    /// Normalized x and y divisors (scale and aspect).
    pub scale: [f64; 2],
    /// Horizontal and vertical perspective (`value / 200`).
    pub perspective: [f64; 2],
}

#[derive(Clone, Debug)]
pub struct LensMap {
    /// Manual distortion k1 (`manual_distortion / 200`).
    pub manual_k1: f64,
    pub sample: Option<SampleMap>,
    /// Embedded warps in inverse-lookup order (reverse opcode order).
    pub embedded: Vec<EmbeddedWarp>,
    pub embedded_crop: [f64; 4],
    /// `distortion_scale / 100` (embedded warps blend by this amount).
    pub distortion: f64,
}

#[derive(Clone, Copy, Debug)]
pub struct SampleMap {
    /// Brown-Conrady k1, k2, k3.
    pub k: [f64; 3],
    /// Tangential p1, p2.
    pub p: [f64; 2],
    pub center: [f64; 2],
    pub distortion_scale: f64,
    pub radial_odd: [f64; 2],
    pub coordinate_scale: [f64; 2],
    /// `distortion_scale` setting / 100.
    pub amount: f64,
}

#[derive(Clone, Copy, Debug)]
pub struct EmbeddedWarp {
    /// Green-plane coefficients (kr0..kr3, kt0, kt1).
    pub k: [f64; 6],
    pub center: [f64; 2],
    pub radius: f64,
}

impl MapPlan {
    /// Output size for an input of `iw` × `ih` (the reference's rounding).
    pub fn output_extent(&self, iw: u32, ih: u32) -> (u32, u32) {
        let r = self.crop;
        let (cw, ch) = (
            (r.right - r.left) * iw as f32,
            (r.bottom - r.top) * ih as f32,
        );
        (cw.round().max(1.) as u32, ch.round().max(1.) as u32)
    }

    /// Reference sample position for output pixel (x, y) of an `iw` × `ih`
    /// input, mirroring `geometry_effects::geometry_mapped`. None: no sample.
    pub fn source(&self, x: u32, y: u32, iw: u32, ih: u32) -> Option<[f32; 2]> {
        let r = self.crop;
        let (iwf, ihf) = (iw as f32, ih as f32);
        let (cw, ch) = ((r.right - r.left) * iwf, (r.bottom - r.top) * ihf);
        let (w, h) = self.output_extent(iw, ih);
        let (cx, cy) = ((r.left + r.right) * iwf / 2., (r.top + r.bottom) * ihf / 2.);
        let (sin, cos) = self.angle.to_radians().sin_cos();
        let dx = (x as f32 + 0.5) * cw / w as f32 - cw / 2.;
        let dy = (y as f32 + 0.5) * ch / h as f32 - ch / 2.;
        let (mut sx, mut sy) = (
            cx + cos * dx + sin * dy - 0.5,
            cy - sin * dx + cos * dy - 0.5,
        );
        if self.transform.is_some() || self.lens.is_some() {
            let t = &self.geometry.transform;
            let mut p = [
                2. * (sx as f64 + 0.5) / iwf as f64 - 1.,
                2. * (sy as f64 + 0.5) / ihf as f64 - 1.,
            ];
            if self.transform.is_some() {
                p[0] -= t.offset_x.clamp(-100., 100.) as f64 / 50.;
                p[1] -= t.offset_y.clamp(-100., 100.) as f64 / 50.;
                let (sin, cos) = (t.rotate.clamp(-10., 10.) as f64).to_radians().sin_cos();
                p = [
                    cos * p[0] + sin * p[1] * ihf as f64 / iwf as f64,
                    -sin * p[0] * iwf as f64 / ihf as f64 + cos * p[1],
                ];
                p[0] /= t.scale as f64 / 100. * (t.aspect.clamp(-100., 100.) as f64 / 100.).exp2();
                p[1] /= t.scale as f64 / 100.;
                let d = 1.
                    - t.horizontal.clamp(-100., 100.) as f64 / 200. * p[0]
                    - t.vertical.clamp(-100., 100.) as f64 / 200. * p[1];
                if d.abs() < 1e-8 {
                    return None;
                }
                p = [p[0] / d, p[1] / d];
            }
            let p = if self.lens.is_some() {
                self.resolved.map(p, 1, &self.common)
            } else {
                p
            };
            sx = ((p[0] + 1.) * iwf as f64 / 2. - 0.5) as f32;
            sy = ((p[1] + 1.) * ihf as f64 / 2. - 0.5) as f32;
        }
        (sx.is_finite()
            && sy.is_finite()
            && sx >= -0.5
            && sy >= -0.5
            && sx < iwf - 0.5
            && sy < ihf - 0.5)
            .then_some([sx, sy])
    }

    /// Input rows `[first, end)` read by output rows `rows` (Lanczos-3 support
    /// plus a margin for backend coordinate rounding). Samples the map on a
    /// grid dense enough for the smooth lens/perspective maps.
    pub fn source_rows(&self, rows: std::ops::Range<u32>, iw: u32, ih: u32) -> (u32, u32) {
        let (w, _) = self.output_extent(iw, ih);
        let (mut lo, mut hi) = (f32::INFINITY, f32::NEG_INFINITY);
        let xs: Vec<u32> = (0..w).step_by(8).chain([w - 1]).collect();
        let ys: Vec<u32> = rows
            .clone()
            .step_by(8)
            .chain([rows.end.saturating_sub(1).max(rows.start)])
            .collect();
        for &y in &ys {
            for &x in &xs {
                if let Some([_, sy]) = self.source(x, y, iw, ih) {
                    lo = lo.min(sy);
                    hi = hi.max(sy);
                }
            }
        }
        if lo > hi {
            return (0, 1.min(ih));
        }
        let first = (lo.floor() as i64 - 4).clamp(0, i64::from(ih) - 1) as u32;
        let end = (hi.floor() as i64 + 6).clamp(i64::from(first) + 1, i64::from(ih)) as u32;
        (first, end)
    }
}

impl ResolvedLens {
    /// Test/analysis constructor: an image-derived calibration.
    pub fn from_calibration(sample: lens::CalibrationSample) -> Self {
        Self {
            source: CorrectionSource::Image,
            sample: Some(sample),
            embedded: Default::default(),
        }
    }

    /// Resident parameters for `settings`, or None when a stage is not
    /// portable (the caller must use the reference renderer).
    pub fn plan(
        &self,
        settings: &DevelopSettings,
        metadata: &raw_decode::RawMetadata,
    ) -> EngineResult<Option<LensPlan>> {
        let s = &settings.lens;
        let g = &settings.geometry;
        crate::optics::validate(s)?;
        if s.defringe_purple.amount != 0.
            || s.defringe_green.amount != 0.
            || g.upright.mode != UprightMode::Off
            || !g.upright.guides.is_empty()
            || g.orientation != 1
            || g.constrain_crop
            || self.embedded.warps.len() > MAX_EMBEDDED
            || self.embedded.gains.len() > MAX_EMBEDDED
        {
            return Ok(None);
        }
        let ca = if self.ca_active(s) {
            let Some(sample) = &self.sample else {
                // Embedded per-channel warps need a Newton channel factorization.
                return Ok(None);
            };
            if self.source == CorrectionSource::Database
                && matches!(metadata.cfa_layout, raw_decode::CfaLayout::Bayer(_))
            {
                // The reference corrects database CA on CFA phases.
                return Ok(None);
            }
            Some(CaPlan {
                red: sample.ca_red,
                blue: sample.ca_blue,
                center: [sample.distortion.cx, sample.distortion.cy],
                coordinate_scale: sample.coordinate_scale,
                amount: s.chromatic_aberration_scale.clamp(0., 200.) as f64 / 100.,
                crop: metadata.default_crop,
                lens: self.clone(),
                settings: s.clone(),
            })
        } else {
            None
        };
        let default = lens::CalibrationSample::default();
        let sample = self.sample.as_ref().unwrap_or(&default);
        let profile = (!(sample.vignette == [0.; 3] && self.embedded.gains.is_empty())
            && s.vignetting_scale != 0.)
            .then(|| ProfileVignette {
                vignette: sample.vignette,
                center: [sample.distortion.cx, sample.distortion.cy],
                coordinate_scale: sample.coordinate_scale,
                amount: s.vignetting_scale.clamp(0., 200.) as f64 / 100.,
                embedded: self
                    .embedded
                    .gains
                    .iter()
                    .map(|v| {
                        let (center, radius, _) = self.embedded.frame(v.center);
                        EmbeddedGain {
                            center,
                            radius,
                            coefficients: v.coefficients,
                        }
                    })
                    .collect(),
                embedded_crop: self.embedded.frame([0.; 2]).2,
            });
        let manual = (s.manual_vignetting != 0.).then(|| {
            [
                s.manual_vignetting.clamp(-100., 100.) as f64 / 50.,
                0.25 + 3.75 * s.manual_vignetting_midpoint.clamp(0., 100.) as f64 / 100.,
            ]
        });
        let vignette =
            (profile.is_some() || manual.is_some()).then_some(VignettePlan { profile, manual });
        let mut common = s.clone();
        common.remove_chromatic_aberration = false;
        let lens_active = self.geometry_active(&common);
        let t = &g.transform;
        let transform_active = *t != Default::default();
        let r = g.crop.rect;
        let map =
            if r == NormalizedRect::FULL && g.crop.angle == 0. && !lens_active && !transform_active
            {
                None
            } else {
                if !r.is_valid()
                    || !g.crop.angle.is_finite()
                    || !(-45.0..=45.0).contains(&g.crop.angle)
                    || g.crop.aspect.is_some_and(|a| a.contains(&0))
                    || [
                        t.vertical,
                        t.horizontal,
                        t.rotate,
                        t.aspect,
                        t.scale,
                        t.offset_x,
                        t.offset_y,
                    ]
                    .iter()
                    .any(|v| !v.is_finite())
                    || !(50.0..=150.0).contains(&t.scale)
                {
                    // Invalid controls: the reference reports the error.
                    return Ok(None);
                }
                let transform = transform_active.then(|| {
                    let (sin, cos) = (t.rotate.clamp(-10., 10.) as f64).to_radians().sin_cos();
                    TransformPlan {
                        offset: [
                            t.offset_x.clamp(-100., 100.) as f64 / 50.,
                            t.offset_y.clamp(-100., 100.) as f64 / 50.,
                        ],
                        rotate: [sin, cos],
                        scale: [
                            t.scale as f64 / 100.
                                * (t.aspect.clamp(-100., 100.) as f64 / 100.).exp2(),
                            t.scale as f64 / 100.,
                        ],
                        perspective: [
                            t.horizontal.clamp(-100., 100.) as f64 / 200.,
                            t.vertical.clamp(-100., 100.) as f64 / 200.,
                        ],
                    }
                });
                let lens = lens_active.then(|| LensMap {
                    manual_k1: s.manual_distortion.clamp(-100., 100.) as f64 / 200.,
                    sample: self.sample.as_ref().map(|p| SampleMap {
                        k: [p.distortion.k1, p.distortion.k2, p.distortion.k3],
                        p: [p.distortion.p1, p.distortion.p2],
                        center: [p.distortion.cx, p.distortion.cy],
                        distortion_scale: p.distortion_scale,
                        radial_odd: p.radial_odd,
                        coordinate_scale: p.coordinate_scale,
                        amount: s.distortion_scale.clamp(0., 200.) as f64 / 100.,
                    }),
                    embedded: self
                        .embedded
                        .warps
                        .iter()
                        .rev()
                        .map(|w| {
                            let (center, radius, _) = self.embedded.frame(w.center);
                            EmbeddedWarp {
                                k: w.coefficients[if w.coefficients.len() == 1 { 0 } else { 1 }],
                                center,
                                radius,
                            }
                        })
                        .collect(),
                    embedded_crop: self.embedded.frame([0.; 2]).2,
                    distortion: s.distortion_scale.clamp(0., 200.) as f64 / 100.,
                });
                Some(MapPlan {
                    crop: r,
                    angle: g.crop.angle,
                    transform,
                    lens,
                    resolved: self.clone(),
                    common,
                    geometry: g.clone(),
                })
            };
        Ok(Some(LensPlan { ca, vignette, map }))
    }
}
