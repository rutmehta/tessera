use engine_api::{
    EngineError, EngineResult,
    color::{ChromaticAdaptation, ColorMatrix3, WhitePoint, WorkingSpace},
    recipe::settings::{WhiteBalanceMode, WhiteBalanceSettings},
    tile::Tile,
};

/// Invert the calibrated XYZ->camera transform without changing chromaticity.
/// Independently normalizing XYZ rows would distort the calibration: equal
/// unbalanced camera channels are not a D65 neutral.
pub fn camera_to_xyz(cam_xyz: ColorMatrix3) -> EngineResult<ColorMatrix3> {
    cam_xyz.inverse()
}

/// Bake matrix coefficients once to f32; pixel arithmetic stays scalar f32.
pub fn apply_matrix(tile: &mut Tile, matrix: ColorMatrix3) -> EngineResult<()> {
    if matrix.0.iter().flatten().any(|v| !v.is_finite()) {
        return Err(EngineError::invalid("matrix", "non-finite coefficient"));
    }
    let m = matrix.to_f32();
    crate::map_rgb(tile, |v| m.map(|r| r[0] * v[0] + r[1] * v[1] + r[2] * v[2]))
}

/// CAT16 conjugated into linear Rec.2020. Matrix input is the same calibrated
/// camera->XYZ transform used by CameraProfile, before any white balance.
pub fn white_balance_matrix(
    settings: &WhiteBalanceSettings,
    camera_xyz: ColorMatrix3,
    multipliers: [f32; 4],
) -> EngineResult<ColorMatrix3> {
    let white = match settings.mode {
        WhiteBalanceMode::AsShot => as_shot_white(camera_xyz, multipliers)?,
        WhiteBalanceMode::Custom => temperature_white(settings.temperature, settings.tint)?,
        WhiteBalanceMode::Daylight | WhiteBalanceMode::Flash => WhitePoint::D55,
        WhiteBalanceMode::Cloudy => WhitePoint::D65,
        WhiteBalanceMode::Shade => WhitePoint::D75,
        WhiteBalanceMode::Tungsten => WhitePoint::A,
        WhiteBalanceMode::Fluorescent => WhitePoint::F2,
        WhiteBalanceMode::Auto => {
            return Err(EngineError::invalid(
                "white balance",
                "Auto is not implemented in M1",
            ));
        }
    };
    let work = WorkingSpace::LinearRec2020.to_xyz();
    Ok(work.inverse()? * ChromaticAdaptation::Cat16.matrix(white, WhitePoint::D65)? * work)
}

/// CCT and perpendicular CIE 1960 Duv. Tint = 3000 * Duv, so positive tint
/// selects a greener source white and produces a magenta correction.
pub fn temperature_white(kelvin: f32, tint: f32) -> EngineResult<WhitePoint> {
    if !kelvin.is_finite()
        || !tint.is_finite()
        || !(1667.0..=25000.0).contains(&kelvin)
        || !(-150.0..=150.0).contains(&tint)
    {
        return Err(EngineError::invalid(
            "temperature/tint",
            "expected 1667..25000 K and -150..150 tint",
        ));
    }
    let t = f64::from(kelvin);
    let [u, v] = locus_uv(t);
    let [nu, nv] = locus_normal(t);
    let u = u + nu * f64::from(tint) / 3000.0;
    let v = v + nv * f64::from(tint) / 3000.0;
    let d = 2.0 * u - 8.0 * v + 4.0;
    Ok(WhitePoint::new(3.0 * u / d, 2.0 * v / d))
}

fn as_shot_white(camera_xyz: ColorMatrix3, multipliers: [f32; 4]) -> EngineResult<WhitePoint> {
    if multipliers[..3].iter().any(|v| !v.is_finite() || *v <= 0.0) {
        return Err(EngineError::invalid(
            "as_shot_wb",
            "positive finite multipliers required",
        ));
    }
    let xyz = camera_xyz.apply(std::array::from_fn(|c| 1.0 / f64::from(multipliers[c])));
    let sum: f64 = xyz.iter().sum();
    if !sum.is_finite() || xyz.iter().any(|v| !v.is_finite() || *v <= 0.0) {
        return Err(EngineError::invalid("scene white", "invalid XYZ white"));
    }
    Ok(WhitePoint::new(xyz[0] / sum, xyz[1] / sum))
}

/// Unrounded as-shot slider coordinates. Robertson-style isotherm search:
/// bisect reciprocal temperature until the white lies on the locus normal.
/// Uses the same locus and signed normal as `temperature_white`, not McCamy.
/// Whites outside the representable slider domain are errors, never clamped.
pub fn as_shot_temperature_tint(
    camera_xyz: ColorMatrix3,
    multipliers: [f32; 4],
) -> EngineResult<(f32, f32)> {
    let w = as_shot_white(camera_xyz, multipliers)?;
    let d = -2.0 * w.x + 12.0 * w.y + 3.0;
    let uv = [4.0 * w.x / d, 6.0 * w.y / d];
    let distance = |t| {
        let p = locus_uv(t);
        let n = locus_normal(t);
        (uv[0] - p[0]) * -n[1] + (uv[1] - p[1]) * n[0]
    };
    if distance(1667.0) < -1e-12 || distance(25000.0) > 1e-12 {
        return Err(EngineError::invalid(
            "as_shot_wb",
            "CCT outside 1667..25000 K",
        ));
    }
    let (mut lo, mut hi) = (1.0 / 25000.0, 1.0 / 1667.0);
    for _ in 0..60 {
        let mid = (lo + hi) * 0.5;
        if distance(1.0 / mid) > 0.0 {
            hi = mid;
        } else {
            lo = mid;
        }
    }
    let t = 2.0 / (lo + hi);
    let p = locus_uv(t);
    let n = locus_normal(t);
    let tint = 3000.0 * ((uv[0] - p[0]) * n[0] + (uv[1] - p[1]) * n[1]);
    if tint.abs() > 150.0 + 1e-6 {
        return Err(EngineError::invalid(
            "as_shot_wb",
            format!("Duv outside -0.05..0.05: T={t}, tint={tint}"),
        ));
    }
    Ok((t as f32, tint.clamp(-150.0, 150.0) as f32))
}

// Krystek (1985) rational approximation of the Planckian locus in CIE 1960
// UCS. Unlike the piecewise xy fit, this has no seams at 2222/4000 K.
fn locus_uv(t: f64) -> [f64; 2] {
    [
        (0.860117757 + 1.54118254e-4 * t + 1.28641212e-7 * t * t)
            / (1.0 + 8.42420235e-4 * t + 7.08145163e-7 * t * t),
        (0.317398726 + 4.22806245e-5 * t + 4.20481691e-8 * t * t)
            / (1.0 - 2.89741816e-5 * t + 1.61456053e-7 * t * t),
    ]
}

fn locus_normal(t: f64) -> [f64; 2] {
    let a = locus_uv(t - 0.01);
    let b = locus_uv(t + 0.01);
    let du = b[0] - a[0];
    let dv = b[1] - a[1];
    let length = du.hypot(dv);
    [dv / length, -du / length]
}
