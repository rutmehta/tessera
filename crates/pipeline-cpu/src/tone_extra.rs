//! Additional scene-linear tone operators; see ../TONE_M2.md.
use engine_api::{
    EngineError, EngineResult,
    recipe::settings::{Curve, ToneSettings},
    tile::Tile,
};

/// Apply after basic tone, before colour/display. See TONE_M2.md for halo rules.
/// Dehaze statistics cover this allocation only (including halos). Use
/// [`tone_extra_image`] for image-global statistics and seam-free rendering.
pub fn tone_extra(tile: &mut Tile, s: &ToneSettings) -> EngineResult<()> {
    if tile.layout().channels != 3 {
        return Err(EngineError::invalid("tile", "expected three RGB planes"));
    }
    let prepared = prepare(tile.samples::<f32>()?, s)?;
    if let Some((param, splines)) = prepared {
        let layout = tile.layout();
        apply(
            tile.samples_mut::<f32>()?,
            layout.stride(),
            layout.rows(),
            s,
            &param,
            &splines,
        );
    }
    Ok(())
}

/// Whole-image reference with global airlight/confidence, independent of tiles.
/// Image-edge windows are truncated and renormalized, not halo-extended.
pub fn tone_extra_image(image: &crate::Image, s: &ToneSettings) -> EngineResult<crate::Image> {
    if image.planes().len() != 3 {
        return Err(EngineError::invalid("image", "expected three RGB planes"));
    }
    let mut data: Vec<f32> = image.planes().iter().flatten().copied().collect();
    if let Some((param, splines)) = prepare(&data, s)? {
        apply(
            &mut data,
            image.width() as usize,
            image.height() as usize,
            s,
            &param,
            &splines,
        );
    }
    let n = image.width() as usize * image.height() as usize;
    crate::Image::new(
        image.width(),
        image.height(),
        data.chunks_exact(n).map(<[f32]>::to_vec).collect(),
    )
}

fn prepare(input: &[f32], s: &ToneSettings) -> EngineResult<Option<(Parametric, Vec<Spline>)>> {
    if [s.texture, s.clarity, s.dehaze]
        .iter()
        .any(|v| !v.is_finite())
    {
        return Err(EngineError::invalid(
            "presence",
            "parameters must be finite",
        ));
    }
    let param = Parametric::new(s)?;
    let curves = [
        &s.curves.rgb,
        &s.curves.red,
        &s.curves.green,
        &s.curves.blue,
        &s.curves.luminance,
    ];
    let splines: Vec<_> = curves
        .iter()
        .map(|c| Spline::new(c))
        .collect::<EngineResult<_>>()?;

    if input.iter().any(|v| !v.is_finite()) {
        return Err(EngineError::invalid("tile", "nonfinite tone input"));
    }
    if s.texture == 0.0
        && s.clarity == 0.0
        && s.dehaze == 0.0
        && param.neutral
        && curves.iter().all(|c| c.is_identity())
    {
        return Ok(None);
    }
    Ok(Some((param, splines)))
}

fn apply(
    data: &mut [f32],
    w: usize,
    h: usize,
    s: &ToneSettings,
    param: &Parametric,
    splines: &[Spline],
) {
    let n = w * h;
    if s.texture != 0.0 || s.clarity != 0.0 {
        presence(data, w, h, s);
    }
    if s.dehaze != 0.0 {
        dehaze(data, w, h, s.dehaze);
    }
    for i in 0..n {
        let mut rgb = std::array::from_fn::<_, 3, _>(|c| data[c * n + i]);
        let y = luma(rgb);
        if y > 0.0 && !param.neutral {
            let gain = finite(decode(param.eval(encode(y))) / y);
            rgb = rgb.map(|v| finite(v * gain));
        }
        for c in 0..3 {
            rgb[c] = splines[c + 1].linear(splines[0].linear(rgb[c]));
        }
        let y = luma(rgb);
        if y > 0.0 {
            let gain = finite(splines[4].linear(y) / y);
            rgb = rgb.map(|v| finite(v * gain));
        } else if rgb.iter().all(|&v| v == 0.0) {
            rgb = [splines[4].linear(0.0); 3];
        }
        for c in 0..3 {
            data[c * n + i] = finite(rgb[c]);
        }
    }
}
fn finite(v: f32) -> f32 {
    v.clamp(-f32::MAX, f32::MAX)
}
fn luma(v: [f32; 3]) -> f32 {
    finite(0.2627 * v[0] + 0.678 * v[1] + 0.0593 * v[2])
}
fn encode(v: f32) -> f32 {
    let log = if v <= f32::MAX * 0.18 {
        (v / 0.18).ln_1p()
    } else {
        v.ln() - 0.18_f32.ln()
    };
    log / (1.0_f32 / 0.18).ln_1p()
}
fn decode(v: f32) -> f32 {
    let exponent = v * (1.0_f32 / 0.18).ln_1p();
    if exponent < 80.0 {
        0.18 * exponent.exp_m1()
    } else {
        finite((exponent + 0.18_f32.ln()).exp())
    }
}

// Separable local sums avoid whole-frame prefix cancellation in f32.
// Fixed small radii keep this O(N*r), with identical overlapping-window order.
fn mean(v: &[f32], w: usize, h: usize, r: usize) -> Vec<f32> {
    let mut horizontal = vec![0.0; v.len()];
    for y in 0..h {
        for x in 0..w {
            let x0 = x.saturating_sub(r);
            let x1 = (x + r + 1).min(w);
            let mut sum = 0.0;
            for xx in x0..x1 {
                sum += v[y * w + xx];
            }
            horizontal[y * w + x] = sum / (x1 - x0) as f32;
        }
    }
    let mut out = vec![0.0; v.len()];
    for y in 0..h {
        for x in 0..w {
            let y0 = y.saturating_sub(r);
            let y1 = (y + r + 1).min(h);
            let mut sum = 0.0;
            for yy in y0..y1 {
                sum += horizontal[yy * w + x];
            }
            out[y * w + x] = sum / (y1 - y0) as f32;
        }
    }
    out
}
fn guided(guide: &[f32], p: &[f32], w: usize, h: usize, r: usize, eps: f32) -> Vec<f32> {
    let mi = mean(guide, w, h, r);
    let mp = mean(p, w, h, r);
    let ii: Vec<_> = guide.iter().map(|i| i * i).collect();
    let ip: Vec<_> = guide.iter().zip(p).map(|(i, p)| i * p).collect();
    let mii = mean(&ii, w, h, r);
    let mip = mean(&ip, w, h, r);
    let a: Vec<_> = (0..p.len())
        .map(|i| (mip[i] - mi[i] * mp[i]) / ((mii[i] - mi[i] * mi[i]).max(0.0) + eps))
        .collect();
    let b: Vec<_> = (0..p.len()).map(|i| mp[i] - a[i] * mi[i]).collect();
    let ma = mean(&a, w, h, r);
    let mb = mean(&b, w, h, r);
    (0..p.len()).map(|i| ma[i] * guide[i] + mb[i]).collect()
}
fn range(v: &[f32], w: usize, h: usize, r: usize) -> (Vec<f32>, Vec<f32>) {
    let mut lo = vec![f32::INFINITY; v.len()];
    let mut hi = vec![f32::NEG_INFINITY; v.len()];
    for y in 0..h {
        for x in 0..w {
            let i = y * w + x;
            for yy in y.saturating_sub(r)..=(y + r).min(h - 1) {
                for xx in x.saturating_sub(r)..=(x + r).min(w - 1) {
                    lo[i] = lo[i].min(v[yy * w + xx]);
                    hi[i] = hi[i].max(v[yy * w + xx]);
                }
            }
        }
    }
    (lo, hi)
}
fn presence(data: &mut [f32], w: usize, h: usize, s: &ToneSettings) {
    let n = w * h;
    let y: Vec<_> = (0..n)
        .map(|i| luma([data[i], data[n + i], data[2 * n + i]]))
        .collect();
    let z: Vec<_> = y.iter().map(|&v| encode(v.max(0.0))).collect();
    let fine = guided(&z, &z, w, h, 1, 0.001);
    let mid = guided(&z, &z, w, h, 3, 0.001);
    let wide = if s.clarity != 0.0 {
        guided(&z, &z, w, h, 8, 0.001)
    } else {
        mid.clone()
    };
    let (lo, hi) = range(&z, w, h, 1);
    for i in 0..n {
        if y[i] <= 0.0 {
            continue;
        }
        let weight = 4.0 * z[i].clamp(0.0, 1.0) * (1.0 - z[i].clamp(0.0, 1.0));
        let delta = s.texture.clamp(-100.0, 100.0) / 100.0 * (fine[i] - mid[i])
            + s.clarity.clamp(-100.0, 100.0) / 100.0 * weight * (mid[i] - wide[i]);
        // No newly created extrema: removes the bright/dark step-edge lobes.
        let out = (z[i] + delta).clamp(lo[i], hi[i]);
        if out == z[i] {
            continue;
        }
        let gain = finite(decode(out) / y[i]);
        for c in 0..3 {
            data[c * n + i] = finite(data[c * n + i] * gain);
        }
    }
}

fn smoothstep(x: f32) -> f32 {
    let t = x.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}
fn percentile(v: &[f32], q: f32) -> f32 {
    let mut sorted = v.to_vec();
    sorted.sort_by(f32::total_cmp);
    sorted[(((sorted.len() - 1) as f32 * q).round() as usize).min(sorted.len() - 1)]
}
fn dehaze(data: &mut [f32], w: usize, h: usize, amount: f32) {
    let n = w * h;
    let rgb: Vec<[f32; 3]> = (0..n)
        .map(|i| std::array::from_fn(|c| data[c * n + i]))
        .collect();
    let y: Vec<_> = rgb.iter().map(|&v| luma(v).max(0.0)).collect();
    let dc: Vec<_> = rgb
        .iter()
        .map(|v| v.iter().copied().fold(f32::INFINITY, f32::min).max(0.0))
        .collect();
    let dark = range(&dc, w, h, 3).0;
    // Use many candidates, not the brightest pixel: resist lamps/speculars.
    let threshold = percentile(&dark, 0.90);
    let ceiling = percentile(&y, 0.99);
    let candidates: Vec<_> = (0..n)
        .filter(|&i| dark[i] >= threshold && y[i] <= ceiling)
        .collect();
    if candidates.is_empty() {
        return;
    }
    let mut air: [f32; 3] = std::array::from_fn(|c| {
        percentile(
            &candidates
                .iter()
                .map(|&i| rgb[i][c].max(0.0))
                .collect::<Vec<_>>(),
            0.5,
        )
    });
    let ay = luma(air);
    if ay < 1e-8 {
        return;
    }
    // Data are already WB corrected; do not infer a strong illuminant tint
    // from a coloured foreground. Retain only modest atmospheric chroma.
    air = air.map(|v| v.clamp(0.75 * ay, finite(1.25 * ay)).max(1e-8));
    let confidence = smoothstep(((percentile(&y, 0.90) - percentile(&y, 0.10)) / ay - 0.05) / 0.20);
    if confidence == 0.0 {
        return;
    }
    let normalized: Vec<_> = rgb
        .iter()
        .map(|v| {
            (0..3)
                .map(|c| (v[c] / air[c]).max(0.0))
                .fold(f32::INFINITY, f32::min)
        })
        .collect();
    let dark = range(&normalized, w, h, 3).0;
    let strength = amount.abs().min(100.0) / 100.0 * confidence;
    let transmission: Vec<_> = dark
        .iter()
        .map(|&d| (1.0 - 0.85 * strength * d.clamp(0.0, 1.0)).clamp(0.15, 1.0))
        .collect();
    let guide: Vec<_> = y.iter().map(|&v| encode(v)).collect();
    let transmission = guided(&guide, &transmission, w, h, 4, 0.001);
    for i in 0..n {
        let t = transmission[i].clamp(0.15, 1.0);
        for c in 0..3 {
            if rgb[i][c] < 0.0 {
                continue;
            }
            let v = if amount > 0.0 {
                (air[c] + (rgb[i][c] - air[c]) / t).max(0.0)
            } else {
                rgb[i][c] * t + air[c] * (1.0 - t)
            };
            data[c * n + i] = finite(v);
        }
    }
}

struct Parametric {
    splits: [f32; 5],
    amounts: [f32; 4],
    neutral: bool,
}
impl Parametric {
    fn new(s: &ToneSettings) -> EngineResult<Self> {
        let p = &s.curves.parametric;
        let amounts = [p.shadows, p.darks, p.lights, p.highlights];
        let splits = [
            0.0,
            p.shadow_split / 100.0,
            p.midtone_split / 100.0,
            p.highlight_split / 100.0,
            1.0,
        ];
        if amounts.iter().any(|v| !v.is_finite())
            || splits.iter().any(|v| !v.is_finite())
            || splits.windows(2).any(|p| p[0] >= p[1])
        {
            return Err(EngineError::invalid(
                "parametric",
                "finite amounts and strictly ordered splits inside (0,100) required",
            ));
        }
        Ok(Self {
            splits,
            amounts: amounts.map(|v| v.clamp(-100.0, 100.0) / 100.0),
            neutral: amounts.iter().all(|&v| v == 0.0),
        })
    }
    fn eval(&self, x: f32) -> f32 {
        if self.neutral || !(0.0..1.0).contains(&x) {
            return x;
        }
        let i = self
            .splits
            .partition_point(|&p| p <= x)
            .saturating_sub(1)
            .min(3);
        let width = self.splits[i + 1] - self.splits[i];
        let u = (x - self.splits[i]) / width;
        x + 3.0 * self.amounts[i] * width * u * u * (1.0 - u) * (1.0 - u)
    }
}

/// Fritsch–Carlson monotone Hermite spline, evaluated directly (no LUT error).
struct Spline {
    points: Vec<(f32, f32)>,
    slopes: Vec<f32>,
    identity: bool,
}
impl Spline {
    fn new(c: &Curve) -> EngineResult<Self> {
        if c.0.iter().any(|p| {
            !p.x.is_finite()
                || !p.y.is_finite()
                || !(0.0..=1.0).contains(&p.x)
                || !(0.0..=1.0).contains(&p.y)
        }) || c.0.windows(2).any(|p| p[1].x <= p[0].x || p[1].y < p[0].y)
        {
            return Err(EngineError::invalid(
                "curve",
                "finite ordered x and nondecreasing y in [0,1] required",
            ));
        }
        let mut points: Vec<_> = c.0.iter().map(|p| (p.x, p.y)).collect();
        if points.is_empty() || points[0].0 > 0.0 {
            points.insert(0, (0.0, 0.0));
        }
        if points.last().unwrap().0 < 1.0 {
            points.push((1.0, 1.0));
        }
        let d: Vec<_> = points
            .windows(2)
            .map(|p| finite((p[1].1 - p[0].1) / (p[1].0 - p[0].0)))
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
        Ok(Self {
            points,
            slopes,
            identity: c.is_identity(),
        })
    }
    fn eval(&self, x: f32) -> f32 {
        if self.identity {
            return x;
        }
        let last = *self.points.last().unwrap();
        if x >= last.0 {
            return x + last.1 - last.0;
        }
        if x <= 0.0 {
            return self.points[0].1 + x;
        }
        let i = self.points.partition_point(|p| p.0 <= x).saturating_sub(1);
        let (x0, y0) = self.points[i];
        let (x1, y1) = self.points[i + 1];
        let h = x1 - x0;
        let t = (x - x0) / h;
        if y0 == y1 {
            return y0;
        }
        (y0 + (y1 - y0) * smoothstep(t) + t * (1.0 - t) * (1.0 - t) * (h * self.slopes[i])
            - t * t * (1.0 - t) * (h * self.slopes[i + 1]))
            .clamp(y0, y1)
    }
    fn linear(&self, v: f32) -> f32 {
        if self.identity || v < 0.0 {
            v
        } else {
            decode(self.eval(encode(v)))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use engine_api::{
        recipe::settings::{Curve, CurvePoint},
        tile::{Extent, TileCoord, TileLayout},
    };
    fn tile(values: &[f32]) -> Tile {
        Tile::from_samples(
            TileCoord::new(0, 0, 0),
            TileLayout {
                extent: Extent::new(values.len() as u32, 1),
                halo: 0,
                channels: 3,
            },
            values.repeat(3),
        )
        .unwrap()
    }
    #[test]
    fn whole_image_uses_global_statistics_beyond_tile_extent() {
        let values: Vec<_> = (0..512).map(|x| 0.2 + 0.6 * x as f32 / 511.0).collect();
        let image = crate::Image::new(512, 1, vec![values.clone(); 3]).unwrap();
        let s = ToneSettings {
            dehaze: 75.0,
            ..ToneSettings::default()
        };
        let out = tone_extra_image(&image, &s).unwrap();
        assert_eq!(out.width(), 512);
        assert!(out.planes()[0][128] < values[128]);
        let mut local = tile(&values[..256]);
        tone_extra(&mut local, &s).unwrap();
        assert!((out.planes()[0][128] - local.plane::<f32>(0).unwrap()[128]).abs() > 0.01);
        let small = crate::Image::new(256, 1, vec![values[..256].to_vec(); 3]).unwrap();
        let small = tone_extra_image(&small, &s).unwrap();
        assert_eq!(small.planes()[0], local.plane::<f32>(0).unwrap());
        let neutral = tone_extra_image(&image, &ToneSettings::default()).unwrap();
        assert_eq!(neutral.planes(), image.planes());
    }
    #[test]
    fn finite_extremes_and_subnormal_knots_stay_bounded() {
        let values = [-f32::MAX, 0.0, f32::from_bits(1), 0.18, 1.0, f32::MAX];
        for amount in [-100.0, 100.0] {
            let mut t = tile(&values);
            t.plane_mut::<f32>(1).unwrap().reverse();
            let mut s = ToneSettings {
                texture: amount,
                clarity: amount,
                dehaze: amount,
                ..ToneSettings::default()
            };
            s.curves.rgb = Curve(vec![
                CurvePoint { x: 0.0, y: 0.0 },
                CurvePoint {
                    x: f32::from_bits(1),
                    y: 0.5,
                },
                CurvePoint { x: 1.0, y: 1.0 },
            ]);
            s.curves.luminance = lift();
            tone_extra(&mut t, &s).unwrap();
            assert!(t.samples::<f32>().unwrap().iter().all(|v| v.is_finite()));
        }
        assert!(encode(f32::MAX).is_finite());
        assert!(decode(encode(f32::MAX)).is_finite());
        assert!(
            tone_extra_image(
                &crate::Image::new(1, 1, vec![vec![0.5]]).unwrap(),
                &ToneSettings::default()
            )
            .is_err()
        );
    }
    #[test]
    fn log_axis_is_scalar_single_precision() {
        let input = 0.3_f32;
        let actual: f32 = encode(input);
        let expected = (input / 0.18).ln_1p() / (1.0_f32 / 0.18).ln_1p();
        assert_eq!(actual.to_bits(), expected.to_bits());
    }
    #[test]
    fn texture_changes_fine_contrast_without_step_ringing() {
        let values: Vec<_> = (0..128)
            .map(|x| 0.3 + 0.01 * (x as f32 * 0.8).sin())
            .collect();
        let mut t = tile(&values);
        let mut s = ToneSettings {
            texture: 100.0,
            ..ToneSettings::default()
        };
        tone_extra(&mut t, &s).unwrap();
        let energy = |v: &[f32]| v[20..100].iter().map(|x| (x - 0.3).powi(2)).sum::<f32>();
        assert!(energy(t.plane::<f32>(0).unwrap()) > energy(&values) * 1.01);
        let step: Vec<_> = (0..128).map(|x| if x < 64 { 0.1 } else { 0.8 }).collect();
        let mut t = tile(&step);
        s.clarity = 100.0;
        tone_extra(&mut t, &s).unwrap();
        for (&a, &b) in t.plane::<f32>(0).unwrap().iter().zip(&step) {
            assert!((a - b).abs() < 1e-4, "{a} vs {b}");
        }
    }
    #[test]
    fn clarity_changes_midscale_but_preserves_constant_rgb() {
        let values: Vec<_> = (0..128)
            .map(|x| 0.3 + 0.01 * (x as f32 * 0.2).sin())
            .collect();
        let mut t = tile(&values);
        let s = ToneSettings {
            clarity: 100.0,
            ..ToneSettings::default()
        };
        tone_extra(&mut t, &s).unwrap();
        assert!(
            t.plane::<f32>(0)
                .unwrap()
                .iter()
                .zip(&values)
                .any(|(a, b)| (a - b).abs() > 1e-4)
        );
        let mut t = tile(&[0.3; 128]);
        let before = t.samples::<f32>().unwrap().to_vec();
        tone_extra(&mut t, &s).unwrap();
        assert!(
            t.samples::<f32>()
                .unwrap()
                .iter()
                .zip(before)
                .all(|(a, b)| (a - b).abs() < 1e-6)
        );
    }
    #[test]
    fn dehaze_removes_or_adds_haze_and_protects_white_scene() {
        let v: Vec<_> = (0..128).map(|i| 0.35 + 0.4 * i as f32 / 127.0).collect();
        let mut t = tile(&v);
        let mut s = ToneSettings {
            dehaze: 75.0,
            ..ToneSettings::default()
        };
        tone_extra(&mut t, &s).unwrap();
        assert!(t.plane::<f32>(0).unwrap()[32] < v[32]);
        let mut t = tile(&v);
        s.dehaze = -75.0;
        tone_extra(&mut t, &s).unwrap();
        assert!(t.plane::<f32>(0).unwrap()[32] > v[32]);
        let snow: Vec<_> = (0..128)
            .map(|i| 0.95 + 0.01 * (i as f32 * 0.1).sin())
            .collect();
        let mut t = tile(&snow);
        s.dehaze = 100.0;
        tone_extra(&mut t, &s).unwrap();
        assert!(
            t.plane::<f32>(0)
                .unwrap()
                .iter()
                .zip(&snow)
                .all(|(a, b)| (a - b).abs() < 1e-6)
        );
    }
    #[test]
    fn airlight_rejects_isolated_hdr_outlier() {
        let v: Vec<_> = (0..128).map(|i| 0.35 + 0.4 * i as f32 / 127.0).collect();
        let mut t = tile(&v);
        let mut t2 = t.clone();
        for c in 0..3 {
            t2.plane_mut::<f32>(c).unwrap()[127] = 1000.0;
        }
        let s = ToneSettings {
            dehaze: 60.0,
            ..ToneSettings::default()
        };
        tone_extra(&mut t, &s).unwrap();
        tone_extra(&mut t2, &s).unwrap();
        assert!(t.plane::<f32>(0).unwrap()[32] < v[32]);
        assert!((t.plane::<f32>(0).unwrap()[32] - t2.plane::<f32>(0).unwrap()[32]).abs() < 0.02);
    }
    #[test]
    fn neutral_is_bit_exact_and_keeps_shared_storage() {
        let mut t = tile(&[-0.0, -0.1, 0.0, 1e-20, 0.18, 4.0, f32::MAX]);
        let other = t.clone();
        tone_extra(&mut t, &ToneSettings::default()).unwrap();
        assert!(t.shares_buffer_with(&other));
        assert!(
            t.samples::<f32>()
                .unwrap()
                .iter()
                .zip(other.samples::<f32>().unwrap())
                .all(|(a, b)| a.to_bits() == b.to_bits())
        );
    }
    #[test]
    fn invalid_parameters_reject_before_mutation() {
        let mut t = tile(&[0.2, 0.3]);
        let other = t.clone();
        let mut s = ToneSettings {
            texture: f32::NAN,
            ..ToneSettings::default()
        };
        assert!(tone_extra(&mut t, &s).is_err());
        assert!(t.shares_buffer_with(&other));
        s.texture = 100.0;
        s.curves.blue = Curve(vec![
            CurvePoint { x: 0.5, y: 0.4 },
            CurvePoint { x: 0.5, y: 0.6 },
        ]);
        assert!(tone_extra(&mut t, &s).is_err());
        assert!(t.shares_buffer_with(&other));
        s.curves.blue = Curve(vec![
            CurvePoint { x: 0.0, y: 0.8 },
            CurvePoint { x: 1.0, y: 0.2 },
        ]);
        assert!(tone_extra(&mut t, &s).is_err());
        let mut bad = tile(&[f32::INFINITY]);
        assert!(tone_extra(&mut bad, &ToneSettings::default()).is_err());
        let mut bad = Tile::zeroed(
            TileCoord::new(0, 0, 0),
            engine_api::tile::TileFormat::U8,
            TileLayout {
                extent: Extent::new(1, 1),
                halo: 0,
                channels: 3,
            },
        )
        .unwrap();
        assert!(tone_extra(&mut bad, &ToneSettings::default()).is_err());
    }
    #[test]
    fn channel_and_luminance_curves_have_distinct_semantics() {
        let mut t = tile(&[0.1]);
        t.plane_mut::<f32>(1).unwrap()[0] = 0.2;
        t.plane_mut::<f32>(2).unwrap()[0] = 0.3;
        let original = t.clone();
        let mut s = ToneSettings::default();
        s.curves.red = lift();
        tone_extra(&mut t, &s).unwrap();
        assert!(t.plane::<f32>(0).unwrap()[0] > 0.1);
        assert_eq!(t.plane::<f32>(1).unwrap()[0], 0.2);
        assert_eq!(t.plane::<f32>(2).unwrap()[0], 0.3);
        let mut t = original;
        s.curves.red = Curve::default();
        s.curves.luminance = lift();
        tone_extra(&mut t, &s).unwrap();
        let v = t.samples::<f32>().unwrap();
        assert!((v[0] / v[1] - 0.5).abs() < 1e-6);
        assert!((v[2] / v[1] - 1.5).abs() < 1e-6);
    }
    #[test]
    fn spline_is_monotone_including_flat_and_steep_segments() {
        let c = Curve(vec![
            CurvePoint { x: 0.0, y: 0.0 },
            CurvePoint { x: 0.01, y: 0.4 },
            CurvePoint { x: 0.02, y: 0.4 },
            CurvePoint { x: 0.9, y: 0.41 },
            CurvePoint { x: 1.0, y: 1.0 },
        ]);
        let s = Spline::new(&c).unwrap();
        let mut prev = 0.0;
        for i in 0..=10000 {
            let v = s.eval(i as f32 / 10000.0);
            assert!(v + f32::EPSILON >= prev, "{v} < {prev}");
            assert!((0.0..=1.0).contains(&v));
            prev = v;
        }
        for p in &c.0 {
            assert!((s.eval(p.x) - p.y).abs() < 1e-12);
        }
    }
    #[test]
    fn presence_halo_matches_overlapping_reference() {
        fn region(start: i32, width: u32, halo: u16) -> Tile {
            let layout = TileLayout {
                extent: Extent::new(width, 9),
                halo,
                channels: 3,
            };
            let mut v = Vec::new();
            for _c in 0..3 {
                for y in 0..layout.rows() {
                    for x in 0..layout.stride() {
                        let xx = start + x as i32 - halo as i32;
                        let yy = y as i32 - halo as i32;
                        v.push(
                            0.3 + 0.015 * (xx as f32 * 0.2).sin() + 0.01 * (yy as f32 * 0.9).cos(),
                        );
                    }
                }
            }
            Tile::from_samples(TileCoord::new(0, 0, 0), layout, v).unwrap()
        }
        let mut full = region(0, 192, 16);
        let mut a = region(64, 64, 16);
        let s = ToneSettings {
            texture: 70.0,
            clarity: 90.0,
            ..ToneSettings::default()
        };
        tone_extra(&mut full, &s).unwrap();
        tone_extra(&mut a, &s).unwrap();
        for y in 0..9 {
            for x in 0..64 {
                let i = a.layout().index(0, x, y).unwrap();
                let j = full.layout().index(0, x + 64, y).unwrap();
                assert!(
                    (a.samples::<f32>().unwrap()[i] - full.samples::<f32>().unwrap()[j]).abs()
                        < 1e-6
                );
            }
        }
    }
    #[test]
    fn negative_presence_reduces_contrast_and_extremes_remain_finite() {
        let v: Vec<_> = (0..128)
            .map(|i| 0.3 + 0.01 * (i as f32 * 0.4).sin())
            .collect();
        let mut t = tile(&v);
        let mut s = ToneSettings {
            texture: -100.0,
            clarity: -100.0,
            ..ToneSettings::default()
        };
        tone_extra(&mut t, &s).unwrap();
        let energy = |v: &[f32]| v[20..100].iter().map(|x| (x - 0.3).powi(2)).sum::<f32>();
        assert!(energy(t.plane::<f32>(0).unwrap()) < energy(&v));
        let mut t = tile(&[-1.0, 0.0, 1e-30, 1.0, 1e10, f32::MAX]);
        s.dehaze = 100.0;
        s.curves.luminance = lift();
        tone_extra(&mut t, &s).unwrap();
        assert!(t.samples::<f32>().unwrap().iter().all(|v| v.is_finite()));
    }
    #[test]
    fn luminance_curve_can_lift_absolute_black() {
        let mut t = tile(&[0.0]);
        let mut s = ToneSettings::default();
        s.curves.luminance = Curve(vec![
            CurvePoint { x: 0.0, y: 0.1 },
            CurvePoint { x: 1.0, y: 1.0 },
        ]);
        tone_extra(&mut t, &s).unwrap();
        let v = t.samples::<f32>().unwrap();
        assert!(v[0] > 0.0);
        assert_eq!(v[0], v[1]);
        assert_eq!(v[1], v[2]);
    }
    fn lift() -> Curve {
        Curve(vec![
            CurvePoint { x: 0.0, y: 0.0 },
            CurvePoint { x: 0.5, y: 0.7 },
            CurvePoint { x: 1.0, y: 1.0 },
        ])
    }
    #[test]
    fn parametric_regions_lift_on_log_axis_and_are_monotone() {
        let mut s = ToneSettings::default();
        s.curves.parametric.shadows = 100.0;
        s.curves.parametric.darks = 100.0;
        s.curves.parametric.lights = 100.0;
        s.curves.parametric.highlights = 100.0;
        let values: Vec<_> = (0..=200).map(|i| decode(i as f32 / 200.0)).collect();
        let mut t = tile(&values);
        tone_extra(&mut t, &s).unwrap();
        let out = t.plane::<f32>(0).unwrap();
        for i in [25, 75, 125, 175] {
            assert!(out[i] > values[i]);
        }
        assert!(out.windows(2).all(|p| p[0] <= p[1]));
        s.curves.parametric.shadow_split = 80.0;
        assert!(tone_extra(&mut t, &s).is_err());
    }
    #[test]
    fn master_curve_lifts_and_preserves_hdr() {
        let mut t = tile(&[0.0, 0.1, 0.3, 1.0, 4.0]);
        let mut s = ToneSettings::default();
        s.curves.rgb = lift();
        tone_extra(&mut t, &s).unwrap();
        let p = t.plane::<f32>(0).unwrap();
        assert!(p[1] > 0.1 && p[2] > 0.3);
        assert_eq!(p[0], 0.0);
        assert!((p[4] - 4.0).abs() < 1e-5);
        assert!(p.windows(2).all(|v| v[1] >= v[0]));
    }
}
