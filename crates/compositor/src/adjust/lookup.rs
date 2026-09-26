//! Checked LUT loaders. Red is the fastest-varying axis internally.
use super::Adjustment;
use engine_api::{EngineError, EngineResult};
fn invalid(message: &str) -> EngineError {
    EngineError::invalid("color_lookup", message)
}
fn triple(fields: &[&str]) -> EngineResult<[f32; 3]> {
    if fields.len() != 3 {
        return Err(invalid("expected three numeric components"));
    }
    let mut out = [0.0_f32; 3];
    for i in 0..3 {
        out[i] = fields[i].parse().map_err(|_| invalid("invalid number"))?;
        if !out[i].is_finite() {
            return Err(invalid("non-finite LUT entry"));
        }
    }
    Ok(out)
}
pub(super) fn valid(size: u32, data: &[[f32; 3]]) -> bool {
    (2..=256).contains(&size)
        && (size as usize).checked_pow(3) == Some(data.len())
        && data.iter().flatten().all(|v| v.is_finite())
}
pub(super) fn sample(size: u32, data: &[[f32; 3]], c: [f32; 3]) -> [f32; 3] {
    // Shape is checked by constructors/validation and once on compilation.
    let n = size as usize;
    let x = c.map(|v| v.clamp(0.0, 1.0) * (n - 1) as f32);
    let lo = x.map(|v| (v as usize).min(n - 2));
    let f = std::array::from_fn::<_, 3, _>(|i| x[i] - lo[i] as f32);
    let mut o = [0.0; 3];
    for b in 0..2 {
        for g in 0..2 {
            for r in 0..2 {
                let w = (if r == 0 { 1.0 - f[0] } else { f[0] })
                    * (if g == 0 { 1.0 - f[1] } else { f[1] })
                    * (if b == 0 { 1.0 - f[2] } else { f[2] });
                let p = data[lo[0] + r + n * (lo[1] + g + n * (lo[2] + b))];
                for i in 0..3 {
                    o[i] += w * p[i];
                }
            }
        }
    }
    o
}
impl Adjustment {
    /// Load the common blue-fastest, integer 3DL format with a uniform input
    /// knot row. Explicit output scale avoids guessing bit depth from contents.
    /// Nonuniform shapers are rejected; rounded uniform integer knots are allowed.
    pub fn color_lookup_from_3dl(text: &str, output_max: f32) -> EngineResult<Self> {
        if !output_max.is_finite() || output_max <= 0.0 {
            return Err(invalid("positive output scale required"));
        }
        let mut lines = text
            .lines()
            .map(|s| s.split('#').next().unwrap_or("").trim())
            .filter(|s| !s.is_empty());
        let first = lines
            .next()
            .ok_or_else(|| invalid("missing 3DL input knots"))?;
        let knots: Vec<f32> = first
            .split_whitespace()
            .map(|s| s.parse::<f32>().map_err(|_| invalid("invalid 3DL knot")))
            .collect::<EngineResult<_>>()?;
        let n = knots.len();
        if !(2..=256).contains(&n)
            || knots[0] != 0.0
            || knots.iter().any(|v| !v.is_finite())
            || knots[n - 1] <= 0.0
            || knots.windows(2).any(|w| w[1] <= w[0])
        {
            return Err(invalid("invalid 3DL input grid"));
        }
        let max = knots[n - 1];
        if knots
            .iter()
            .enumerate()
            .any(|(i, v)| (v - i as f32 * max / (n - 1) as f32).abs() > 1.0)
        {
            return Err(invalid("nonuniform 3DL shapers require pre-resampling"));
        }
        let mut data = Vec::new();
        for line in lines {
            if data.len() >= n.pow(3) {
                return Err(invalid("too many 3DL entries"));
            }
            let p = triple(&line.split_whitespace().collect::<Vec<_>>())?;
            if p.iter().any(|v| *v < 0.0 || *v > output_max) {
                return Err(invalid("3DL output outside declared scale"));
            }
            data.push(p.map(|v| v / output_max));
        }
        if data.len() != n.pow(3) {
            return Err(invalid("3DL entry count does not match grid"));
        }
        let mut reordered = vec![[0.0; 3]; data.len()];
        for r in 0..n {
            for g in 0..n {
                for b in 0..n {
                    reordered[r + n * (g + n * b)] = data[b + n * (g + n * r)];
                }
            }
        }
        Ok(Self::ColorLookup {
            size: n as u32,
            data: reordered,
        })
    }

    /// Load a unit-domain 3D CUBE. 1D/shaper LUTs and non-unit domains are
    /// rejected explicitly rather than silently evaluated with wrong inputs.
    pub fn color_lookup_from_cube(text: &str) -> EngineResult<Self> {
        let mut size = None;
        let mut data = Vec::new();
        for line in text.lines() {
            let fields: Vec<_> = line
                .split('#')
                .next()
                .unwrap_or("")
                .split_whitespace()
                .collect();
            if fields.is_empty() {
                continue;
            }
            match fields[0] {
                "TITLE" => {}
                "LUT_3D_SIZE" => {
                    if size.is_some() || fields.len() != 2 {
                        return Err(invalid("duplicate or invalid LUT_3D_SIZE"));
                    }
                    let n: u32 = fields[1]
                        .parse()
                        .map_err(|_| invalid("invalid cube size"))?;
                    if !(2..=256).contains(&n) {
                        return Err(invalid("cube size must be 2..256"));
                    }
                    size = Some(n);
                }
                "DOMAIN_MIN" | "DOMAIN_MAX" => {
                    let domain = triple(&fields[1..])?;
                    let expected = if fields[0] == "DOMAIN_MIN" { 0.0 } else { 1.0 };
                    if domain != [expected; 3] {
                        return Err(invalid("non-unit CUBE domains require pre-resampling"));
                    }
                }
                s if s.starts_with("LUT_") => {
                    return Err(invalid("only 3D CUBE LUTs are supported"));
                }
                _ => {
                    let n = size.ok_or_else(|| invalid("LUT_3D_SIZE must precede data"))?;
                    if data.len() >= (n as usize).pow(3) {
                        return Err(invalid("too many LUT entries"));
                    }
                    data.push(triple(&fields)?);
                }
            }
        }
        let size = size.ok_or_else(|| invalid("missing LUT_3D_SIZE"))?;
        if !valid(size, &data) {
            return Err(invalid("cube entry count does not match size cubed"));
        }
        Ok(Self::ColorLookup { size, data })
    }
}
