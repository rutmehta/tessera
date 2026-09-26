//! Image-processing primitives used by the selection tools.

/// Separable Gaussian blur with clamp-to-edge addressing (`sigma` ≤ 0 is a
/// copy).
pub fn gaussian(data: &[f32], w: usize, h: usize, sigma: f32) -> Vec<f32> {
    if sigma <= 0.0 || w == 0 || h == 0 {
        return data.to_vec();
    }
    let r = (3.0 * sigma).ceil() as i64;
    let k: Vec<f32> = (-r..=r)
        .map(|i| (-(i * i) as f32 / (2.0 * sigma * sigma)).exp())
        .collect();
    let s: f32 = k.iter().sum();
    let k: Vec<f32> = k.iter().map(|v| v / s).collect();
    let mut tmp = vec![0.0f32; w * h];
    for y in 0..h {
        let row = &data[y * w..(y + 1) * w];
        for x in 0..w {
            let mut acc = 0.0;
            for (j, kv) in k.iter().enumerate() {
                let xx = (x as i64 + j as i64 - r).clamp(0, w as i64 - 1) as usize;
                acc += kv * row[xx];
            }
            tmp[y * w + x] = acc;
        }
    }
    let mut out = vec![0.0f32; w * h];
    for y in 0..h {
        for x in 0..w {
            let mut acc = 0.0;
            for (j, kv) in k.iter().enumerate() {
                let yy = (y as i64 + j as i64 - r).clamp(0, h as i64 - 1) as usize;
                acc += kv * tmp[yy * w + x];
            }
            out[y * w + x] = acc;
        }
    }
    out
}

/// Mean over the `(2r+1)²` window truncated at the borders (integral
/// image in f64).
pub fn box_mean(data: &[f32], w: usize, h: usize, r: usize) -> Vec<f32> {
    let mut sat = vec![0.0f64; (w + 1) * (h + 1)];
    for y in 0..h {
        let mut row = 0.0f64;
        for x in 0..w {
            row += f64::from(data[y * w + x]);
            sat[(y + 1) * (w + 1) + x + 1] = sat[y * (w + 1) + x + 1] + row;
        }
    }
    let mut out = vec![0.0f32; w * h];
    for y in 0..h {
        let (y0, y1) = (y.saturating_sub(r), (y + r + 1).min(h));
        for x in 0..w {
            let (x0, x1) = (x.saturating_sub(r), (x + r + 1).min(w));
            let s = sat[y1 * (w + 1) + x1] - sat[y0 * (w + 1) + x1] - sat[y1 * (w + 1) + x0]
                + sat[y0 * (w + 1) + x0];
            out[y * w + x] = (s / ((y1 - y0) * (x1 - x0)) as f64) as f32;
        }
    }
    out
}

/// 1-D squared distance transform of sampled function `f` (infinite =
/// no site), lower envelope of parabolas, f64 internally.
fn edt_1d(f: &[f64], out: &mut [f64], v: &mut [usize], z: &mut [f64]) {
    let n = f.len();
    let mut k: isize = -1;
    for q in 0..n {
        if !f[q].is_finite() {
            continue;
        }
        loop {
            if k < 0 {
                k = 0;
                v[0] = q;
                z[0] = f64::NEG_INFINITY;
                z[1] = f64::INFINITY;
                break;
            }
            let p = v[k as usize];
            let (qf, pf) = (q as f64, p as f64);
            let s = ((f[q] + qf * qf) - (f[p] + pf * pf)) / (2.0 * (qf - pf));
            if s <= z[k as usize] {
                k -= 1;
                continue;
            }
            k += 1;
            v[k as usize] = q;
            z[k as usize] = s;
            z[k as usize + 1] = f64::INFINITY;
            break;
        }
    }
    if k < 0 {
        out.fill(f64::INFINITY);
        return;
    }
    let mut j = 0usize;
    for q in 0..n {
        while z[j + 1] < q as f64 {
            j += 1;
        }
        let p = v[j];
        out[q] = (q as f64 - p as f64).powi(2) + f[p];
    }
}

/// Squared Euclidean distance from every pixel to the nearest `site`
/// (Felzenszwalb–Huttenlocher); infinite when there is none.
pub fn edt_sq(site: &[bool], w: usize, h: usize) -> Vec<f32> {
    let mut g: Vec<f64> = site
        .iter()
        .map(|s| if *s { 0.0 } else { f64::INFINITY })
        .collect();
    let n = w.max(h);
    let (mut f, mut o) = (vec![0.0f64; n], vec![0.0f64; n]);
    let (mut v, mut z) = (vec![0usize; n], vec![0.0f64; n + 1]);
    for x in 0..w {
        for y in 0..h {
            f[y] = g[y * w + x];
        }
        edt_1d(&f[..h], &mut o[..h], &mut v, &mut z);
        for y in 0..h {
            g[y * w + x] = o[y];
        }
    }
    for y in 0..h {
        f[..w].copy_from_slice(&g[y * w..(y + 1) * w]);
        edt_1d(&f[..w], &mut o[..w], &mut v, &mut z);
        g[y * w..(y + 1) * w].copy_from_slice(&o[..w]);
    }
    g.into_iter().map(|v| v as f32).collect()
}

/// Signed distance (pixels) to the `0.5` iso-contour of a soft mask:
/// negative inside. Anti-aliased pixels use their coverage for sub-pixel
/// placement (`0.5 − a`).
pub fn signed_distance(mask: &[f32], w: usize, h: usize) -> Vec<f32> {
    let inside: Vec<bool> = mask.iter().map(|v| *v >= 0.5).collect();
    let outside: Vec<bool> = inside.iter().map(|v| !v).collect();
    let d_in = edt_sq(&inside, w, h);
    let d_out = edt_sq(&outside, w, h);
    const FAR: f32 = 1e6;
    mask.iter()
        .enumerate()
        .map(|(i, &a)| {
            if a > 0.0 && a < 1.0 {
                // Clamp so that fractional pixels agree with their neighbours.
                if a >= 0.5 {
                    (0.5 - a).max(-(d_out[i].sqrt() - 0.5).min(FAR))
                } else {
                    (0.5 - a).min((d_in[i].sqrt() - 0.5).min(FAR))
                }
            } else if inside[i] {
                -(d_out[i].sqrt().min(FAR) - 0.5)
            } else {
                d_in[i].sqrt().min(FAR) - 0.5
            }
        })
        .collect()
}

/// Sobel gradient magnitude of a single-channel image.
pub fn sobel(data: &[f32], w: usize, h: usize) -> Vec<f32> {
    let at = |x: i64, y: i64| {
        data[y.clamp(0, h as i64 - 1) as usize * w + x.clamp(0, w as i64 - 1) as usize]
    };
    let mut out = vec![0.0f32; w * h];
    for y in 0..h as i64 {
        for x in 0..w as i64 {
            let gx = at(x + 1, y - 1) + 2.0 * at(x + 1, y) + at(x + 1, y + 1)
                - at(x - 1, y - 1)
                - 2.0 * at(x - 1, y)
                - at(x - 1, y + 1);
            let gy = at(x - 1, y + 1) + 2.0 * at(x, y + 1) + at(x + 1, y + 1)
                - at(x - 1, y - 1)
                - 2.0 * at(x, y - 1)
                - at(x + 1, y - 1);
            out[y as usize * w + x as usize] = 0.125 * gx.hypot(gy);
        }
    }
    out
}

/// Gray-guide guided filter (He et al.): edge-preserving smoothing of `p`
/// that follows the structure of `guide`.
pub fn guided_filter(p: &[f32], guide: &[f32], w: usize, h: usize, r: usize, eps: f32) -> Vec<f32> {
    let mean_i = box_mean(guide, w, h, r);
    let mean_p = box_mean(p, w, h, r);
    let ii: Vec<f32> = guide.iter().map(|v| v * v).collect();
    let ip: Vec<f32> = guide.iter().zip(p).map(|(a, b)| a * b).collect();
    let corr_i = box_mean(&ii, w, h, r);
    let corr_ip = box_mean(&ip, w, h, r);
    let n = w * h;
    let mut a = vec![0.0f32; n];
    let mut b = vec![0.0f32; n];
    for i in 0..n {
        let var = (corr_i[i] - mean_i[i] * mean_i[i]).max(0.0);
        let cov = corr_ip[i] - mean_i[i] * mean_p[i];
        a[i] = cov / (var + eps);
        b[i] = mean_p[i] - a[i] * mean_i[i];
    }
    let ma = box_mean(&a, w, h, r);
    let mb = box_mean(&b, w, h, r);
    (0..n).map(|i| ma[i] * guide[i] + mb[i]).collect()
}

/// Otsu threshold of `values` over `bins` histogram bins.
pub fn otsu(values: &[f32], bins: usize) -> f32 {
    let (lo, hi) = values
        .iter()
        .fold((f32::INFINITY, f32::NEG_INFINITY), |(a, b), v| {
            (a.min(*v), b.max(*v))
        });
    if hi.partial_cmp(&lo) != Some(std::cmp::Ordering::Greater) {
        return lo;
    }
    let mut hist = vec![0u64; bins];
    for v in values {
        let i = (((v - lo) / (hi - lo)) * (bins - 1) as f32).round() as usize;
        hist[i.min(bins - 1)] += 1;
    }
    let total = values.len() as f64;
    let sum: f64 = hist
        .iter()
        .enumerate()
        .map(|(i, c)| i as f64 * *c as f64)
        .sum();
    let (mut w0, mut sum0, mut best, mut arg) = (0.0f64, 0.0f64, -1.0f64, 0usize);
    for (i, &c) in hist.iter().enumerate() {
        w0 += c as f64;
        if w0 == 0.0 {
            continue;
        }
        let w1 = total - w0;
        if w1 == 0.0 {
            break;
        }
        sum0 += i as f64 * c as f64;
        let (m0, m1) = (sum0 / w0, (sum - sum0) / w1);
        let between = w0 * w1 * (m0 - m1) * (m0 - m1);
        if between > best {
            best = between;
            arg = i;
        }
    }
    lo + (arg as f32 + 0.5) / (bins - 1) as f32 * (hi - lo)
}
