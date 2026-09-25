//! Explicit profile inputs; no implicit filesystem/network database lookup.
use crate::Image;
use engine_api::{
    EngineError, EngineResult,
    recipe::settings::{LensProfileSource, LensSettings},
};
use lens::{CalibrationSample, Profile, ProfileDatabase};
use raw_decode::RawMetadata;

/// Caller-owned profiles, including profiles loaded by `lens::load_user_profile`.
#[derive(Default)]
pub struct LensContext<'a> {
    pub profile: Option<&'a Profile>,
    pub database: Option<&'a ProfileDatabase>,
    /// Overrides unavailable/missing capture data. Focus distance is not exposed by RawMetadata.
    pub capture: Option<[f64; 3]>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CorrectionSource {
    Embedded,
    Database,
    Image,
    Manual,
}
#[derive(Clone, Debug)]
pub struct ResolvedLens {
    pub(crate) source: CorrectionSource,
    pub(crate) sample: Option<CalibrationSample>,
    pub(crate) embedded: crate::embedded_lens::Embedded,
}
impl ResolvedLens {
    pub(crate) fn geometry_active(&self, s: &LensSettings) -> bool {
        s.manual_distortion != 0.
            || !self.embedded.warps.is_empty()
            || self.sample.as_ref().is_some_and(|p| {
                (s.distortion_scale != 0.
                    && (p.distortion != Default::default()
                        || p.distortion_scale != 1.
                        || p.radial_odd != [0.; 2]))
                    || (s.remove_chromatic_aberration
                        && s.chromatic_aberration_scale != 0.
                        && (p.ca_red != [1., 0., 0.] || p.ca_blue != [1., 0., 0.]))
            })
    }
    pub(crate) fn map(&self, p: [f64; 2], channel: usize, s: &LensSettings) -> [f64; 2] {
        let mut q = lens::BrownConrady {
            k1: s.manual_distortion.clamp(-100., 100.) as f64 / 200.,
            ..Default::default()
        }
        .distort(p);
        if let Some(sample) = &self.sample {
            let d = sample.distort(q);
            let amount = s.distortion_scale.clamp(0., 200.) as f64 / 100.;
            q = [q[0] + amount * (d[0] - q[0]), q[1] + amount * (d[1] - q[1])];
            if s.remove_chromatic_aberration && channel != 1 {
                let c = if channel == 0 {
                    sample.ca_red
                } else {
                    sample.ca_blue
                };
                let x = (q[0] - sample.distortion.cx) * sample.coordinate_scale[0];
                let y = (q[1] - sample.distortion.cy) * sample.coordinate_scale[1];
                let r = x * x + y * y;
                let scale = 1.
                    + (c[0] - 1. + r * (c[1] + r * c[2]))
                        * s.chromatic_aberration_scale.clamp(0., 200.) as f64
                        / 100.;
                q = [
                    sample.distortion.cx + (q[0] - sample.distortion.cx) * scale,
                    sample.distortion.cy + (q[1] - sample.distortion.cy) * scale,
                ];
            }
        }
        self.embedded.map(q, channel, s)
    }
    pub(crate) fn ca_active(&self, s: &LensSettings) -> bool {
        s.remove_chromatic_aberration
            && s.chromatic_aberration_scale != 0.
            && (self
                .sample
                .as_ref()
                .is_some_and(|p| p.ca_red != [1., 0., 0.] || p.ca_blue != [1., 0., 0.])
                || self.embedded.warps.iter().any(|w| {
                    w.coefficients.len() == 3
                        && (w.coefficients[0] != w.coefficients[1]
                            || w.coefficients[2] != w.coefficients[1])
                }))
    }
    // CA alone in the still-distorted sensor frame. Common geometry is deferred.
    pub(crate) fn ca_map(&self, p: [f64; 2], channel: usize, s: &LensSettings) -> Option<[f64; 2]> {
        if channel == 1 {
            return Some(p);
        }
        if let Some(sample) = &self.sample {
            let c = if channel == 0 {
                sample.ca_red
            } else {
                sample.ca_blue
            };
            let x = (p[0] - sample.distortion.cx) * sample.coordinate_scale[0];
            let y = (p[1] - sample.distortion.cy) * sample.coordinate_scale[1];
            let r = x * x + y * y;
            let scale = 1.
                + (c[0] - 1. + r * (c[1] + r * c[2]))
                    * s.chromatic_aberration_scale.clamp(0., 200.) as f64
                    / 100.;
            return Some([
                sample.distortion.cx + (p[0] - sample.distortion.cx) * scale,
                sample.distortion.cy + (p[1] - sample.distortion.cy) * scale,
            ]);
        }
        // Factor the complete embedded lookup F_c as A_c o F_green.
        // Merely subtracting green displacement fails with nonidentity common warps.
        let q = crate::upright::undistort(p, &|q, _| Some(self.embedded.map(q, 1, s)))?;
        Some(self.embedded.map(q, channel, s))
    }
    pub fn source(&self) -> CorrectionSource {
        self.source
    }
    pub fn sample(&self) -> Option<&CalibrationSample> {
        self.sample.as_ref()
    }
}
pub fn resolve_lens(
    image: &Image,
    s: &LensSettings,
    metadata: Option<&RawMetadata>,
    context: &LensContext<'_>,
) -> EngineResult<ResolvedLens> {
    crate::optics::validate(s)?;
    let mut out = ResolvedLens {
        source: CorrectionSource::Manual,
        sample: None,
        embedded: Default::default(),
    };
    if matches!(
        s.profile,
        LensProfileSource::Auto | LensProfileSource::Embedded
    ) {
        if let Some(m) = metadata {
            out.embedded = crate::embedded_lens::Embedded::parse(m)?;
        }
        if out.embedded.present() {
            out.source = CorrectionSource::Embedded;
            return Ok(out);
        }
        if matches!(s.profile, LensProfileSource::Embedded) {
            return Err(EngineError::invalid(
                "lens profile",
                "embedded calibration unavailable",
            ));
        }
    }
    if matches!(
        s.profile,
        LensProfileSource::Auto | LensProfileSource::Database { .. }
    ) {
        let named = match &s.profile {
            LensProfileSource::Database { profile } => Some(profile.name.as_str()),
            _ => None,
        };
        let profile = context.profile.or_else(|| {
            context.database.and_then(|db| {
                if let Some(name) = named {
                    db.profiles.iter().find(|p| p.model == name)
                } else {
                    metadata.and_then(|m| {
                        m.lens
                            .as_ref()
                            .and_then(|model| db.find_for_camera(&m.make, &m.model, "", model))
                    })
                }
            })
        });
        if let Some(p) = profile {
            let first = p
                .samples
                .first()
                .ok_or_else(|| EngineError::invalid("lens profile", "empty profile"))?;
            let positive = |x: f32, fallback: f64| {
                if x.is_finite() && x > 0. {
                    x as f64
                } else {
                    fallback
                }
            };
            let capture = context.capture.unwrap_or_else(|| {
                metadata.map_or([first.focal, first.aperture, first.distance], |m| {
                    [
                        positive(m.focal_mm, first.focal),
                        positive(m.aperture, first.aperture),
                        first.distance,
                    ]
                })
            });
            out.sample = Some(
                p.sample(capture[0], capture[1], capture[2])
                    .ok_or_else(|| {
                        EngineError::invalid(
                            "lens profile",
                            "invalid profile or capture coordinates",
                        )
                    })?,
            );
            out.source = CorrectionSource::Database;
        } else if named.is_some() {
            return Err(EngineError::invalid(
                "lens profile",
                "named profile not supplied",
            ));
        }
    }
    let calibrate = out.sample.is_none()
        && matches!(
            s.profile,
            LensProfileSource::Auto | LensProfileSource::AutoCalibrated
        );
    let ca_only = out.sample.is_none() && s.remove_chromatic_aberration;
    if (calibrate || ca_only) && image.width() >= 8 && image.height() >= 8 {
        let (gray, rgb) = analysis_images(image)?;
        let mut sample = CalibrationSample::default();
        let mut found = false;
        if calibrate {
            let lines = lens::detect_lines(&gray, 0.02, 12);
            let traces: Vec<_> = lines.into_iter().take(64).map(|l| l.points).collect();
            if let Some(e) = lens::estimate_k1(&traces, [-0.3, 0.3])
                .filter(|e| e.confidence >= 0.8 && e.value.abs() > 1e-4)
            {
                sample.distortion.k1 = e.value;
                found = true;
            }
            if let Some(e) = lens::estimate_vignette(&gray)
                .filter(|e| e.confidence >= 0.9 && e.value[0].abs() > 1e-4)
            {
                sample.vignette = e.value;
                found = true;
            }
        }
        if s.remove_chromatic_aberration
            && let Some(e) = lens::estimate_ca(&rgb, 0.02).filter(|e| e.confidence >= 0.8)
            && e.value
                .red
                .iter()
                .zip([1., 0., 0.])
                .chain(e.value.blue.iter().zip([1., 0., 0.]))
                .any(|(a, b)| (a - b).abs() > 1e-4)
        {
            sample.ca_red = e.value.red;
            sample.ca_blue = e.value.blue;
            found = true;
        }
        if found {
            out.sample = Some(sample);
            out.source = CorrectionSource::Image;
        }
    }
    Ok(out)
}

pub(crate) fn analysis_images(image: &Image) -> EngineResult<(lens::GrayImage, lens::RgbImage)> {
    if image.planes().len() != 3 {
        return Err(EngineError::invalid("lens", "RGB analysis required"));
    }
    // Endpoint-aligned nearest samples match lens's [-1,1] pixel-center convention.
    let scale = image.width().max(image.height()).div_ceil(256);
    let w = image.width().div_ceil(scale).max(3) as usize;
    let h = image.height().div_ceil(scale).max(3) as usize;
    let pixels: Vec<[f64; 3]> = (0..w * h)
        .map(|i| {
            let x = (i % w) * (image.width() as usize - 1) / (w - 1);
            let y = (i / w) * (image.height() as usize - 1) / (h - 1);
            std::array::from_fn(|c| image.planes()[c][y * image.width() as usize + x] as f64)
        })
        .collect();
    let gray = pixels
        .iter()
        .map(|p| 0.2627 * p[0] + 0.678 * p[1] + 0.0593 * p[2])
        .collect();
    let error = |e: lens::Error| EngineError::invalid("lens analysis", e.to_string());
    Ok((
        lens::GrayImage::new(w, h, gray).map_err(error)?,
        lens::RgbImage::new(w, h, pixels).map_err(error)?,
    ))
}
