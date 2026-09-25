//! Host-side curve coefficients. Pixels are evaluated by the compute shader.
use engine_api::{EngineError, EngineResult, recipe::settings::ToneSettings};

thread_local! {
    static CONSTANTS: std::cell::RefCell<crate::fused::ConstantsCache<ToneSettings>> = std::cell::RefCell::new(Default::default());
}

pub(crate) fn parameters(s: &ToneSettings, p: &mut Vec<f32>) -> EngineResult<()> {
    let prepared = CONSTANTS.with(|cache| {
        cache.borrow_mut().get_or_try_insert(s, || {
            let mut prepared = vec![0.; 33];
            build_parameters(s, &mut prepared)?;
            Ok(prepared)
        })
    })?;
    p[0] = prepared[0];
    p.truncate(9);
    p.extend_from_slice(&prepared[9..]);
    Ok(())
}
fn build_parameters(s: &ToneSettings, p: &mut Vec<f32>) -> EngineResult<()> {
    let param = &s.curves.parametric;
    let amounts = [param.shadows, param.darks, param.lights, param.highlights];
    let splits = [
        0.0,
        param.shadow_split / 100.0,
        param.midtone_split / 100.0,
        param.highlight_split / 100.0,
        1.0,
    ];
    if amounts.iter().chain(splits.iter()).any(|v| !v.is_finite())
        || splits.windows(2).any(|v| v[0] >= v[1])
    {
        return Err(EngineError::invalid(
            "parametric",
            "finite amounts and ordered splits required",
        ));
    }
    p[0] = 5.0;
    p[9] = if amounts.iter().all(|v| *v == 0.0) {
        0.0
    } else {
        1.0
    };
    p[10..15].copy_from_slice(&splits);
    p[15..19].copy_from_slice(&amounts.map(|v| v.clamp(-100.0, 100.0) / 100.0));
    p[24] = (1.0_f32 / 0.18).ln_1p();
    for (index, c) in [
        &s.curves.rgb,
        &s.curves.red,
        &s.curves.green,
        &s.curves.blue,
        &s.curves.luminance,
    ]
    .into_iter()
    .enumerate()
    {
        if c.0.iter().any(|v| {
            !v.x.is_finite()
                || !v.y.is_finite()
                || !(0.0..=1.0).contains(&v.x)
                || !(0.0..=1.0).contains(&v.y)
        }) || c.0.windows(2).any(|v| v[0].x >= v[1].x || v[0].y > v[1].y)
        {
            return Err(EngineError::invalid(
                "curve",
                "finite ordered knots required",
            ));
        }
        if c.is_identity() {
            continue;
        }
        let mut points: Vec<_> = c.0.iter().map(|v| (v.x, v.y)).collect();
        if points.is_empty() || points[0].0 > 0.0 {
            points.insert(0, (0.0, 0.0));
        }
        if points.last().unwrap().0 < 1.0 {
            points.push((1.0, 1.0));
        }
        let finite = |v: f32| v.clamp(-f32::MAX, f32::MAX);
        let d: Vec<_> = points
            .windows(2)
            .map(|v| finite((v[1].1 - v[0].1) / (v[1].0 - v[0].0)))
            .collect();
        let mut slopes = vec![0.0; points.len()];
        slopes[0] = d[0];
        *slopes.last_mut().unwrap() = *d.last().unwrap();
        for i in 1..slopes.len() - 1 {
            slopes[i] = d[i - 1] * 0.5 + d[i] * 0.5;
        }
        for i in 0..d.len() {
            if d[i] == 0.0 {
                slopes[i] = 0.0;
                slopes[i + 1] = 0.0;
            } else {
                slopes[i] = slopes[i].min(finite(3.0 * d[i]));
                slopes[i + 1] = slopes[i + 1].min(finite(3.0 * d[i]));
                let a = slopes[i] / d[i];
                let b = slopes[i + 1] / d[i];
                let r = a.hypot(b);
                if r > 3.0 {
                    slopes[i] = (3.0 * a / r) * d[i];
                    slopes[i + 1] = (3.0 * b / r) * d[i];
                }
            }
        }
        p[19 + index] = p.len() as f32;
        p.push(points.len() as f32);
        for ((x, y), m) in points.into_iter().zip(slopes) {
            p.extend([x, y, m]);
        }
    }
    Ok(())
}
