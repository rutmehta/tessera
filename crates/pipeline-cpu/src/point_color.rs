//! Point Color reference operator; see POINT_COLOR.md for explicit approximations.
use crate::color_detail::{from_lab, to_lab};
use engine_api::recipe::settings::PointColor;

fn feather(x: f32, [a, b, c, d]: [f32; 4]) -> f32 {
    let smooth = |t: f32| {
        let t = t.clamp(0., 1.);
        t * t * (3. - 2. * t)
    };
    if x < b {
        if a == b {
            0.
        } else {
            smooth((x - a) / (b - a))
        }
    } else if x <= c {
        1.
    } else if c == d {
        0.
    } else {
        smooth((d - x) / (d - c))
    }
}
fn distance(a: f32, b: f32) -> f32 {
    (a - b + 180.).rem_euclid(360.) - 180.
}
fn hsl([r, g, b]: [f32; 3]) -> [f32; 3] {
    let hi = r.max(g).max(b);
    let lo = r.min(g).min(b);
    let c = hi - lo;
    let l = (hi + lo) * 0.5;
    if c <= 1e-7 {
        return [0., 0., l];
    }
    let h = if hi == r {
        (g - b) / c
    } else if hi == g {
        (b - r) / c + 2.
    } else {
        (r - g) / c + 4.
    };
    [
        60. * h.rem_euclid(6.),
        c / (1. - (2. * l - 1.).abs()).max(1e-7),
        l,
    ]
}
fn rgb([h, s, l]: [f32; 3]) -> [f32; 3] {
    let c = (1. - (2. * l - 1.).abs()) * s;
    let h = h.rem_euclid(360.) / 60.;
    let x = c * (1. - (h.rem_euclid(2.) - 1.).abs());
    let p = match h as u32 {
        0 => [c, x, 0.],
        1 => [x, c, 0.],
        2 => [0., c, x],
        3 => [0., x, c],
        4 => [x, 0., c],
        _ => [c, 0., x],
    };
    p.map(|v| v + l - c * 0.5)
}
/// One point at a time, in recipe order. Excluded/no-op pixels return exactly.
pub(crate) fn apply(input: [f32; 3], p: &PointColor) -> [f32; 3] {
    if p.hue_shift == 0. && p.saturation_shift == 0. && p.luminance_shift == 0. {
        return input;
    }
    if let Some(s) = &p.selection {
        // The Adobe working model is proprietary. This documented approximation
        // operates on SDR linear-working RGB HSL and leaves HDR/signed pixels alone.
        if input.iter().any(|v| !(0.0..=1.0).contains(v)) {
            return input;
        }
        let [h, sat, l] = hsl(input);
        if sat <= 1e-7 {
            return input;
        }
        let width = p.range / 50.;
        let relative = [0.5 + distance(h, s.source_hsl[0]) / 360., sat, l];
        let centers = [0.5, s.source_hsl[1], s.source_hsl[2]];
        let weight = relative
            .into_iter()
            .zip(centers)
            .zip([s.hue, s.saturation, s.luminance])
            .map(|((x, center), range)| {
                if width == 0. {
                    if (x - center).abs() <= 1e-7 { 1. } else { 0. }
                } else {
                    feather(center + (x - center) / width, range)
                }
            })
            .product::<f32>();
        if weight == 0. {
            return input;
        }
        rgb([
            h + weight * p.hue_shift,
            (sat * (1. + weight * p.saturation_shift / 100.)).clamp(0., 1.),
            (l * (1. + weight * p.luminance_shift / 100.)).clamp(0., 1.),
        ])
    } else {
        let [l, a, b] = to_lab(input);
        let c = a.hypot(b);
        if c <= 1e-7 {
            return input;
        }
        let h = b.atan2(a).to_degrees().rem_euclid(360.);
        let [sl, sc, sh] = p.source_lch;
        let d =
            ((l - sl).powi(2) + ((c - sc) * 2.).powi(2) + (distance(h, sh) / 180.).powi(2)).sqrt();
        let weight = if p.range == 0. {
            if d <= 1e-7 { 1. } else { 0. }
        } else {
            feather(d, [0., 0., 0., p.range / 100.])
        };
        if weight == 0. {
            return input;
        }
        let angle = (h + weight * p.hue_shift).to_radians();
        let c = c * (1. + weight * p.saturation_shift / 100.);
        from_lab([
            l * (1. + weight * p.luminance_shift / 100.),
            c * angle.cos(),
            c * angle.sin(),
        ])
    }
}
