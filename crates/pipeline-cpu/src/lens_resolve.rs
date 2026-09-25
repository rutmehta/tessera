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
    if image.planes().len() != 3 {
        return Err(EngineError::invalid("lens", "RGB analysis required"));
    }
    resolve_with(
        (image.width(), image.height()),
        || analysis_images(image),
        s,
        metadata,
        context,
    )
}

/// [`resolve_lens`] for a RAW sensor plane, without a full-frame demosaic.
///
/// The reference analyses the demosaiced camera-RGB active area, but only at
/// the nearest-sample grid of [`analysis_images`] (at most 256 samples on the
/// long edge). Each sample is developed here from a small CFA patch through
/// the same highlight reconstruction and demosaic operators. Patches start at
/// multiples of the CFA period and either reach the real sensor edge or keep
/// a 7-pixel margin (4 highlight + 3 demosaic halo), so every sample is
/// bit-identical to the whole-frame reference. `plane` is the level-0 sensor
/// plane (`metadata.width` × `metadata.height`).
pub fn resolve_lens_sensor(
    plane: &[f32],
    metadata: &RawMetadata,
    settings: &engine_api::recipe::DevelopSettings,
    context: &LensContext<'_>,
) -> EngineResult<ResolvedLens> {
    let [_, _, cw, ch] = metadata.default_crop;
    // Only calibrating/CA-estimating settings develop the analysis samples.
    resolve_with(
        (cw, ch),
        || sensor_analysis(plane, metadata, settings),
        &settings.lens,
        Some(metadata),
        context,
    )
}

/// One developed camera-RGB sample at sensor (x, y), from a small CFA patch.
///
/// Demosaic reads ±3 rows/columns of highlight-reconstructed samples, which
/// read ±4 raw samples. Out-of-frame reads fold to the nearest same-phase
/// sample, up to `period - 1` inside the edge. The patch therefore covers
/// every sample transitively read, starts at a multiple of the CFA period
/// (same phase indexing) and ends either inside the frame or at its real
/// edge (same folding), so the result is bit-identical to the whole frame.
fn sensor_sample(
    plane: &[f32],
    metadata: &RawMetadata,
    (period, algorithm, mode): (
        u32,
        crate::DemosaicAlgorithm,
        engine_api::recipe::settings::HighlightReconstruction,
    ),
    x: u32,
    y: u32,
) -> EngineResult<[f32; 3]> {
    let (w, h) = (metadata.width, metadata.height);
    let span = |v: u32, n: u32| {
        let (v, n, p) = (i64::from(v), i64::from(n), i64::from(period));
        // Demosaic reads, after folding at the frame edges.
        let (mut first, mut last) = ((v - 3).max(0), (v + 3).min(n - 1));
        if v + 3 >= n {
            first = first.min(n - p);
        }
        if v - 3 < 0 {
            last = last.max(p - 1);
        }
        let lo = (first - 4).max(0) / p * p;
        let hi = (last + 5).min(n).max((lo + p).min(n));
        (lo as u32, hi as u32)
    };
    let (x0, x1) = span(x, w);
    let (y0, y1) = span(y, h);
    let (pw, ph) = (x1 - x0, y1 - y0);
    let mut patch = Vec::with_capacity(pw as usize * ph as usize);
    for row in y0..y1 {
        let from = row as usize * w as usize;
        patch.extend_from_slice(&plane[from + x0 as usize..from + x1 as usize]);
    }
    let cfa = metadata.cfa_layout;
    let coord = engine_api::tile::TileCoord::new(0, 0, 0);
    let raw = Image::new(pw, ph, vec![patch])?;
    let mut recovered = Image::blank(pw, ph, 1);
    recovered.put(&crate::reconstruct_highlights(
        &raw.tile(coord, 4, period)?,
        cfa,
        mode,
    )?)?;
    let rgb = crate::demosaic(&recovered.tile(coord, 3, period)?, cfa, algorithm)?;
    let data = rgb.samples::<f32>()?;
    let n = rgb.layout().plane_len();
    let i = ((y - y0) * pw + (x - x0)) as usize;
    Ok([data[i], data[n + i], data[2 * n + i]])
}

/// One analysis sample: active-area position and developed camera RGB.
type Developed = ((u32, u32), [f32; 3]);

/// [`analysis_images`] of the demosaiced active area, from sparse patches.
fn sensor_analysis(
    plane: &[f32],
    metadata: &RawMetadata,
    settings: &engine_api::recipe::DevelopSettings,
) -> EngineResult<(lens::GrayImage, lens::RgbImage)> {
    use engine_api::recipe::settings::DemosaicMethod;
    let (w, h) = (metadata.width, metadata.height);
    let [left, top, cw, ch] = metadata.default_crop;
    if plane.len() != w as usize * h as usize
        || cw == 0
        || ch == 0
        || u64::from(left) + u64::from(cw) > u64::from(w)
        || u64::from(top) + u64::from(ch) > u64::from(h)
    {
        return Err(EngineError::invalid(
            "lens",
            "sensor plane or active area mismatch",
        ));
    }
    let cfa = metadata.cfa_layout;
    crate::mosaic::validate_cfa(cfa)?;
    let period = if matches!(cfa, raw_decode::CfaLayout::XTrans(_)) {
        6
    } else {
        2
    };
    if w < period || h < period {
        return Err(EngineError::invalid(
            "CFA",
            "image must contain a complete CFA period",
        ));
    }
    let algorithm = match settings.demosaic.method {
        DemosaicMethod::Auto => crate::DemosaicAlgorithm::MalvarHeCutler,
        DemosaicMethod::Bilinear => crate::DemosaicAlgorithm::Bilinear,
        _ => {
            return Err(EngineError::invalid(
                "demosaic",
                "only Auto (MHC) and Bilinear implemented",
            ));
        }
    };
    let ops = (
        period,
        algorithm,
        settings.linearize.highlight_reconstruction,
    );
    let develop = |x: u32, y: u32| sensor_sample(plane, metadata, ops, x, y);
    let (points, aw, _) = analysis_grid(cw, ch);
    // Parallel over sample rows; each sample is independent and deterministic.
    let workers = std::thread::available_parallelism()
        .map_or(1, usize::from)
        .clamp(1, 8);
    let rows: Vec<u32> = points
        .iter()
        .map(|p| p.1)
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .collect();
    let cols: Vec<u32> = points[..aw].iter().map(|p| p.0).collect();
    let mut developed: std::collections::HashMap<(u32, u32), [f32; 3]> =
        std::collections::HashMap::with_capacity(points.len());
    std::thread::scope(|scope| -> EngineResult<()> {
        let handles: Vec<_> = (0..workers)
            .map(|worker| {
                let (rows, cols, develop) = (&rows, &cols, &develop);
                scope.spawn(move || -> EngineResult<Vec<Developed>> {
                    let mut out = Vec::new();
                    for &y in rows.iter().skip(worker).step_by(workers) {
                        for &x in cols {
                            out.push(((x, y), develop(left + x, top + y)?));
                        }
                    }
                    Ok(out)
                })
            })
            .collect();
        for handle in handles {
            developed.extend(handle.join().expect("lens analysis worker panicked")?);
        }
        Ok(())
    })?;
    analysis_from((cw, ch), &|x, y| developed[&(x, y)])
}

/// The nearest-sample analysis grid of an active area: points (row-major),
/// grid width and height. Endpoint-aligned like lens's [-1,1] pixel centres.
fn analysis_grid(width: u32, height: u32) -> (Vec<(u32, u32)>, usize, usize) {
    let scale = width.max(height).div_ceil(256);
    let w = width.div_ceil(scale).max(3) as usize;
    let h = height.div_ceil(scale).max(3) as usize;
    let points = (0..w * h)
        .map(|i| {
            let x = (i % w) * (width as usize - 1) / (w - 1);
            let y = (i / w) * (height as usize - 1) / (h - 1);
            (x as u32, y as u32)
        })
        .collect();
    (points, w, h)
}

fn resolve_with(
    (width, height): (u32, u32),
    analysis: impl FnOnce() -> EngineResult<(lens::GrayImage, lens::RgbImage)>,
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
    if (calibrate || ca_only) && width >= 8 && height >= 8 {
        let (gray, rgb) = analysis()?;
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
    analysis_from((image.width(), image.height()), &|x, y| {
        std::array::from_fn(|c| image.planes()[c][(y * image.width() + x) as usize])
    })
}

fn analysis_from(
    (width, height): (u32, u32),
    pixel: &dyn Fn(u32, u32) -> [f32; 3],
) -> EngineResult<(lens::GrayImage, lens::RgbImage)> {
    // Endpoint-aligned nearest samples match lens's [-1,1] pixel-center convention.
    let (points, w, h) = analysis_grid(width, height);
    let pixels: Vec<[f64; 3]> = points
        .iter()
        .map(|&(x, y)| pixel(x, y).map(f64::from))
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

#[cfg(test)]
mod tests {
    use super::*;
    use engine_api::recipe::DevelopSettings;
    use raw_decode::CfaLayout;

    fn metadata(cfa: CfaLayout, width: u32, height: u32, crop: [u32; 4]) -> RawMetadata {
        RawMetadata {
            make: "test".into(),
            model: "test".into(),
            lens: None,
            iso: 100.,
            shutter_s: 0.01,
            aperture: 4.,
            focal_mm: 50.,
            capture_time: 0,
            orientation: 1,
            width,
            height,
            cfa_layout: cfa,
            black_levels: [0.; 4],
            white_level: 65535,
            as_shot_wb: [1.; 4],
            camera_to_xyz: engine_api::color::ColorMatrix3::IDENTITY,
            cam_xyz: [[1., 0., 0.], [0., 1., 0.], [0., 0., 1.], [0., 0., 0.]],
            rgb_cam: [[1., 0., 0., 0.], [0., 1., 0., 0.], [0., 0., 1., 0.]],
            default_crop: crop,
            has_gain_map: false,
            has_opcode_list: false,
            opcode_lists: [None, None, None],
        }
    }

    /// The sparse sensor analysis is bit-identical to the whole-frame one,
    /// including clipped highlights, sensor edges and both CFA families.
    #[test]
    fn sparse_sensor_analysis_matches_whole_frame() {
        let xtrans = CfaLayout::XTrans([
            [1, 2, 1, 1, 0, 1],
            [0, 1, 0, 2, 1, 2],
            [1, 2, 1, 1, 0, 1],
            [1, 0, 1, 1, 2, 1],
            [2, 1, 2, 0, 1, 0],
            [1, 0, 1, 1, 2, 1],
        ]);
        for (cfa, period) in [(CfaLayout::Bayer([[0, 1], [3, 2]]), 2), (xtrans, 6)] {
            for (w, h, crop) in [(611, 397, [5, 3, 600, 390]), (300, 520, [0, 0, 300, 520])] {
                let m = metadata(cfa, w, h, crop);
                let plane: Vec<f32> = (0..w * h)
                    .map(|i| {
                        let (x, y) = (i % w, i / w);
                        // Edges, gradients and clipped (>= 1) highlights.
                        let v = 0.05 + ((x * 7 + y * 13) % 97) as f32 / 90.;
                        if (x / 40 + y / 30) % 5 == 0 {
                            v * 1.6
                        } else {
                            v
                        }
                    })
                    .collect();
                for method in [
                    engine_api::recipe::settings::HighlightReconstruction::Clip,
                    engine_api::recipe::settings::HighlightReconstruction::ReconstructColor,
                ] {
                    let mut settings = DevelopSettings::default();
                    settings.linearize.highlight_reconstruction = method;
                    let raw = Image::new(w, h, vec![plane.clone()]).unwrap();
                    let mut rec = Image::blank(w, h, 1);
                    for c in raw.coords() {
                        rec.put(
                            &crate::reconstruct_highlights(
                                &raw.tile(c, 4, period).unwrap(),
                                cfa,
                                method,
                            )
                            .unwrap(),
                        )
                        .unwrap();
                    }
                    let mut rgb = Image::blank(w, h, 3);
                    for c in rec.coords() {
                        rgb.put(
                            &crate::demosaic(
                                &rec.tile(c, 3, period).unwrap(),
                                cfa,
                                crate::DemosaicAlgorithm::MalvarHeCutler,
                            )
                            .unwrap(),
                        )
                        .unwrap();
                    }
                    let whole = analysis_images(&rgb.downsample_crop(crop, 1).unwrap()).unwrap();
                    let sparse = sensor_analysis(&plane, &m, &settings).unwrap();
                    assert_eq!(
                        format!("{sparse:?}"),
                        format!("{whole:?}"),
                        "{cfa:?} {w}x{h} {method:?}"
                    );
                }
            }
        }
    }
}
