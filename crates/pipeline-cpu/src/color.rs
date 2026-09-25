use engine_api::{
    EngineError, EngineResult,
    color::{ChromaticAdaptation, ColorMatrix3, WhitePoint, WorkingSpace},
    recipe::settings::{WhiteBalanceMode, WhiteBalanceSettings},
    tile::Tile,
};

/// Invert raw XYZ->camera, then normalize inverse rows to D65 row sums.
/// This fixes the arbitrary scale of each XYZ row, not the as-shot illuminant.
pub fn camera_to_xyz(cam_xyz: ColorMatrix3) -> EngineResult<ColorMatrix3> {
    let mut inverse = cam_xyz.inverse()?;
    for (row, white) in inverse.0.iter_mut().zip(WhitePoint::D65.to_xyz()) {
        let sum: f64 = row.iter().sum();
        if !sum.is_finite() || sum <= 1e-12 {
            return Err(EngineError::invalid("cam_xyz", "invalid inverse row sum"));
        }
        for v in row {
            *v *= white / sum;
        }
    }
    Ok(inverse)
}

/// Bake matrix coefficients once to f32; pixel arithmetic stays scalar f32.
pub fn apply_matrix(tile: &mut Tile, matrix: ColorMatrix3) -> EngineResult<()> {
    if matrix.0.iter().flatten().any(|v| !v.is_finite()) {
        return Err(EngineError::invalid("matrix", "non-finite coefficient"));
    }
    let m = matrix.to_f32();
    crate::map_rgb(tile, |v| m.map(|r| r[0] * v[0] + r[1] * v[1] + r[2] * v[2]))
}

/// CAT16 conjugated into linear Rec.2020. Matrix input is the same normalized
/// camera->XYZ transform used by CameraProfile, before any white balance.
pub fn white_balance_matrix(
    settings: &WhiteBalanceSettings,
    camera_xyz: ColorMatrix3,
    multipliers: [f32; 4],
) -> EngineResult<ColorMatrix3> {
    let white = match settings.mode {
        WhiteBalanceMode::AsShot => {
            if multipliers[..3].iter().any(|v| !v.is_finite() || *v <= 0.0) {
                return Err(EngineError::invalid(
                    "as_shot_wb",
                    "positive finite multipliers required",
                ));
            }
            let xyz = camera_xyz.apply(std::array::from_fn(|c| 1.0 / f64::from(multipliers[c])));
            let sum: f64 = xyz.iter().sum();
            if xyz.iter().any(|v| !v.is_finite() || *v <= 0.0) {
                return Err(EngineError::invalid("scene white", "invalid XYZ white"));
            }
            WhitePoint::new(xyz[0] / sum, xyz[1] / sum)
        }
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

/// Planckian-locus polynomial approximation, 1667..25000 K. Positive tint
/// raises source v in CIE 1960 uv (green source -> magenta correction).
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
    let x = if t <= 4000.0 {
        -0.2661239e9 / t.powi(3) - 0.2343580e6 / t.powi(2) + 0.8776956e3 / t + 0.179910
    } else {
        -3.0258469e9 / t.powi(3) + 2.1070379e6 / t.powi(2) + 0.2226347e3 / t + 0.240390
    };
    let y = if t <= 2222.0 {
        -1.1063814 * x.powi(3) - 1.34811020 * x * x + 2.18555832 * x - 0.20219683
    } else if t <= 4000.0 {
        -0.9549476 * x.powi(3) - 1.37418593 * x * x + 2.09137015 * x - 0.16748867
    } else {
        3.0817580 * x.powi(3) - 5.87338670 * x * x + 3.75112997 * x - 0.37001483
    };
    let d = -2.0 * x + 12.0 * y + 3.0;
    let u = 4.0 * x / d;
    let v = 6.0 * y / d + f64::from(tint) * 0.00005;
    let d = 2.0 * u - 8.0 * v + 4.0;
    Ok(WhitePoint::new(3.0 * u / d, 2.0 * v / d))
}
