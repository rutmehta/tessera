use engine_api::{
    EngineError, EngineResult,
    recipe::settings::{Curve, ToneCurves},
};

/// Independently chosen Adobe-Standard-inspired S curve, not an Adobe table.
pub fn default_tone(x: f32) -> f32 {
    use engine_api::recipe::settings::CurvePoint;
    static CURVE: std::sync::OnceLock<Curve> = std::sync::OnceLock::new();
    let curve = CURVE.get_or_init(|| {
        Curve(
            [
                (0., 0.),
                (0.02, 0.012),
                (0.08, 0.07),
                (0.18, 0.22),
                (0.5, 0.62),
                (1., 1.),
            ]
            .map(|(x, y)| CurvePoint { x, y })
            .to_vec(),
        )
    });
    evaluate(x.clamp(0., 1.), curve)
}

pub fn apply(rgb: [f32; 3], curves: &ToneCurves) -> [f32; 3] {
    apply_domain(rgb, curves, false)
}
/// Signed power encoding extends the existing Adobe approximation to HDR knots.
pub fn apply_domain(rgb: [f32; 3], curves: &ToneCurves, extended: bool) -> [f32; 3] {
    if [
        &curves.rgb,
        &curves.red,
        &curves.green,
        &curves.blue,
        &curves.luminance,
    ]
    .iter()
    .all(|c| c.is_identity())
    {
        return rgb;
    }
    let encode = |v: f32| {
        if extended {
            v.signum() * v.abs().powf(1. / 2.2)
        } else {
            v.max(0.).powf(1. / 2.2)
        }
    };
    let decode = |v: f32| {
        if extended {
            v.signum() * v.abs().powf(2.2)
        } else {
            v.max(0.).powf(2.2)
        }
    };
    let evaluate_extended = |x: f32, c: &Curve| {
        if extended && !c.is_identity() && c.0.first().is_some_and(|p| x < p.x) {
            c.0[0].y + x - c.0[0].x
        } else {
            evaluate(x, c)
        }
    };
    let channels = [&curves.red, &curves.green, &curves.blue];
    let mut out = std::array::from_fn(|i| {
        let x = encode(rgb[i]);
        decode(evaluate_extended(
            evaluate_extended(x, &curves.rgb),
            channels[i],
        ))
    });
    let y = crate::luminance(out);
    if (y > 0. || (extended && y < 0.)) && !curves.luminance.is_identity() {
        let mapped = decode(evaluate_extended(encode(y), &curves.luminance));
        out = out.map(|v| v * mapped / y);
    }
    out
}

// Monotone cubic Hermite, harmonic interior slopes and flat extrema.
fn evaluate(x: f32, curve: &Curve) -> f32 {
    let p = &curve.0;
    if p.len() < 2 || curve.is_identity() {
        return x;
    }
    if x <= p[0].x {
        return p[0].y;
    }
    if x >= p[p.len() - 1].x {
        return p[p.len() - 1].y + (x - p[p.len() - 1].x);
    }
    let j = p.partition_point(|v| v.x <= x) - 1;
    let slope = |k: usize| (p[k + 1].y - p[k].y) / (p[k + 1].x - p[k].x);
    let tangent = |k: usize| {
        if k == 0 {
            return slope(0);
        }
        if k == p.len() - 1 {
            return slope(k - 1);
        }
        let (a, b) = (slope(k - 1), slope(k));
        if a * b <= 0. {
            0.
        } else {
            2. * a * b / (a + b)
        }
    };
    let h = p[j + 1].x - p[j].x;
    let t = (x - p[j].x) / h;
    let v = (2. * t * t * t - 3. * t * t + 1.) * p[j].y
        + (t * t * t - 2. * t * t + t) * h * tangent(j)
        + (-2. * t * t * t + 3. * t * t) * p[j + 1].y
        + (t * t * t - t * t) * h * tangent(j + 1);
    v.clamp(p[j].y.min(p[j + 1].y), p[j].y.max(p[j + 1].y))
}
pub fn validate(curves: &ToneCurves) -> EngineResult<()> {
    validate_domain(curves, false)
}
pub fn validate_domain(curves: &ToneCurves, extended: bool) -> EngineResult<()> {
    for curve in [
        &curves.rgb,
        &curves.red,
        &curves.green,
        &curves.blue,
        &curves.luminance,
    ] {
        if curve.0.len() == 1
            || curve.0.iter().any(|p| {
                !p.x.is_finite()
                    || !p.y.is_finite()
                    || (!extended && (!(0. ..=1.).contains(&p.x) || !(0. ..=1.).contains(&p.y)))
            })
            || curve
                .0
                .windows(2)
                .any(|p| p[0].x >= p[1].x || (extended && p[0].y > p[1].y))
        {
            return Err(EngineError::invalid(
                "curve",
                "finite ordered normalized knots required",
            ));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use engine_api::recipe::settings::CurvePoint;
    #[test]
    fn default_profile_curve_has_toe_and_mid_contrast() {
        assert!(default_tone(0.02) < 0.02);
        assert!(default_tone(0.4) > 0.4);
        assert_eq!(default_tone(0.), 0.);
        assert_eq!(default_tone(1.), 1.);
        let mut previous = 0.;
        for i in 0..=1000 {
            let y = default_tone(i as f32 / 1000.);
            assert!(y >= previous && y <= 1.);
            previous = y;
        }
    }
    #[test]
    fn channel_curve_uses_encoded_axis_and_leaves_other_channels() {
        let mut s = ToneCurves {
            red: Curve(vec![
                CurvePoint { x: 0., y: 0. },
                CurvePoint { x: 0.5, y: 0.75 },
                CurvePoint { x: 1., y: 1. },
            ]),
            ..Default::default()
        };
        let x = 0.5f32.powf(2.2);
        let out = apply([x; 3], &s);
        assert!((out[0] - 0.75f32.powf(2.2)).abs() < 1e-6);
        assert!((out[1] - x).abs() < 1e-6 && (out[2] - x).abs() < 1e-6);
        assert!(validate(&s).is_ok());
        s.red.0[1].x = -1.;
        assert!(validate(&s).is_err());
    }
}
