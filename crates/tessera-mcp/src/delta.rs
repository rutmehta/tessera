//! CIEDE2000, shared algorithm with the pipeline GPU regression suite.

/// CIEDE2000 for CIELab values, with unit parametric weights.
pub fn delta_e_2000(a: [f64; 3], b: [f64; 3]) -> f64 {
    let [l1, a1, b1] = a;
    let [l2, a2, b2] = b;
    let c_bar = (a1.hypot(b1) + a2.hypot(b2)) / 2.0;
    let chroma_ratio = |c: f64| c.powi(7) / (c.powi(7) + 25_f64.powi(7));
    let g = 0.5 * (1.0 - chroma_ratio(c_bar).sqrt());
    let ap1 = (1.0 + g) * a1;
    let ap2 = (1.0 + g) * a2;
    let cp1 = ap1.hypot(b1);
    let cp2 = ap2.hypot(b2);
    let hue = |ap: f64, bv: f64, cp: f64| {
        if cp == 0.0 {
            0.0
        } else {
            bv.atan2(ap).to_degrees().rem_euclid(360.0)
        }
    };
    let h1 = hue(ap1, b1, cp1);
    let h2 = hue(ap2, b2, cp2);
    let achromatic = cp1 == 0.0 || cp2 == 0.0;
    let mut dh = h2 - h1;
    if achromatic {
        dh = 0.0;
    } else if dh > 180.0 {
        dh -= 360.0;
    } else if dh < -180.0 {
        dh += 360.0;
    }
    let dl = l2 - l1;
    let dc = cp2 - cp1;
    let d_h = 2.0 * (cp1 * cp2).sqrt() * (dh / 2.0).to_radians().sin();
    let l_bar = (l1 + l2) / 2.0;
    let cp_bar = (cp1 + cp2) / 2.0;
    let h_bar = if achromatic {
        h1 + h2
    } else if (h1 - h2).abs() <= 180.0 {
        (h1 + h2) / 2.0
    } else if h1 + h2 < 360.0 {
        (h1 + h2 + 360.0) / 2.0
    } else {
        (h1 + h2 - 360.0) / 2.0
    };
    let cos = |degrees: f64| degrees.to_radians().cos();
    let t =
        1.0 - 0.17 * cos(h_bar - 30.0) + 0.24 * cos(2.0 * h_bar) + 0.32 * cos(3.0 * h_bar + 6.0)
            - 0.20 * cos(4.0 * h_bar - 63.0);
    let l_offset_sq = (l_bar - 50.0).powi(2);
    let sl = 1.0 + 0.015 * l_offset_sq / (20.0 + l_offset_sq).sqrt();
    let sc = 1.0 + 0.045 * cp_bar;
    let sh = 1.0 + 0.015 * cp_bar * t;
    let theta = 30.0 * (-((h_bar - 275.0) / 25.0).powi(2)).exp();
    let rt = -2.0 * chroma_ratio(cp_bar).sqrt() * (2.0 * theta).to_radians().sin();
    let (vl, vc, vh) = (dl / sl, dc / sc, d_h / sh);
    (vl * vl + vc * vc + vh * vh + rt * vc * vh).sqrt()
}
