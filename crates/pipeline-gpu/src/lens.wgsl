// Resident export lens stages, ported from pipeline-cpu optics.rs,
// embedded_lens.rs, lens_resolve.rs and geometry_effects.rs. The reference
// evaluates coordinates in f64; here they are f32 (≤ 1e-3 px at 8K).
@group(0) @binding(0) var<storage, read> src: array<f32>;
@group(0) @binding(1) var<storage, read_write> dst: array<f32>;
@group(0) @binding(2) var<storage, read> p: array<u32>;

fn f(i: u32) -> f32 { return bitcast<f32>(p[i]); }
fn index(gid: vec3<u32>) -> u32 { return gid.x + gid.y * 65535u * 64u; }

// ── lateral CA: optics::lateral_ca + ResolvedLens::ca_map (sample) ──
// p: w, h, halo, ox, oy, sensor W, H | crop[4], centre[2], scale[2], amount,
//    red[3], blue[3] (f32 from 7).
@compute @workgroup_size(64)
fn lateral_ca(@builtin(global_invocation_id) gid: vec3<u32>) {
    let w = p[0]; let h = p[1]; let halo = p[2];
    let n = w * h;
    let i = index(gid);
    if i >= n * 3u { return; }
    let c = i / n;
    let px = (i % n) % w;
    let py = (i % n) / w;
    let stride = w + 2u * halo;
    let plane = stride * (h + 2u * halo);
    if c == 1u {
        dst[i] = src[c * plane + (py + halo) * stride + px + halo];
        return;
    }
    let x = f32(p[3] + px);
    let y = f32(p[4] + py);
    let crop = vec4<f32>(f(7u), f(8u), f(9u), f(10u));
    let center = vec2<f32>(f(11u), f(12u));
    let scale = vec2<f32>(f(13u), f(14u));
    let amount = f(15u);
    let k = select(vec3<f32>(f(19u), f(20u), f(21u)), vec3<f32>(f(16u), f(17u), f(18u)), c == 0u);
    let pn = vec2<f32>(2. * (x + 0.5 - crop.x) / crop.z - 1., 2. * (y + 0.5 - crop.y) / crop.w - 1.);
    let m = (pn - center) * scale;
    let r = m.x * m.x + m.y * m.y;
    let s = 1. + (k.x - 1. + r * (k.y + r * k.z)) * amount;
    let q = center + (pn - center) * s;
    let sx = (q.x + 1.) * crop.z / 2. + crop.x - 0.5;
    let sy = (q.y + 1.) * crop.w / 2. + crop.y - 0.5;
    let u = clamp(sx, 0., f32(p[5] - 1u));
    let v = clamp(sy, 0., f32(p[6] - 1u));
    let a = u32(floor(u));
    let b = u32(floor(v));
    let fu = u - floor(u);
    let fv = v - floor(v);
    let x0 = i32(p[3]) - i32(halo);
    let y0 = i32(p[4]) - i32(halo);
    let at = array<f32, 4>(
        src[c * plane + u32(clamp(i32(b) - y0, 0, i32(h + 2u * halo) - 1)) * stride
            + u32(clamp(i32(a) - x0, 0, i32(stride) - 1))],
        src[c * plane + u32(clamp(i32(b) - y0, 0, i32(h + 2u * halo) - 1)) * stride
            + u32(clamp(i32(min(a + 1u, p[5] - 1u)) - x0, 0, i32(stride) - 1))],
        src[c * plane + u32(clamp(i32(min(b + 1u, p[6] - 1u)) - y0, 0, i32(h + 2u * halo) - 1)) * stride
            + u32(clamp(i32(a) - x0, 0, i32(stride) - 1))],
        src[c * plane + u32(clamp(i32(min(b + 1u, p[6] - 1u)) - y0, 0, i32(h + 2u * halo) - 1)) * stride
            + u32(clamp(i32(min(a + 1u, p[5] - 1u)) - x0, 0, i32(stride) - 1))],
    );
    let value = (at[0] * (1. - fu) + at[1] * fu) * (1. - fv) + (at[2] * (1. - fu) + at[3] * fu) * fv;
    dst[i] = clamp(value, -3.4028234663852886e38f, 3.4028234663852886e38f);
}

// ── vignetting: optics::profile_vignette then optics::point_corrections ──
// p: w, h, ox, oy, frame W, H, flags (1 profile, 2 manual), gains |
//    v[3], centre[2], scale[2], amount, manual amount, exponent, crop[4],
//    per gain: centre[2], radius, k[5] (f32 from 8).
@compute @workgroup_size(64)
fn vignette(@builtin(global_invocation_id) gid: vec3<u32>) {
    let w = p[0]; let h = p[1];
    let n = w * h;
    let i = index(gid);
    if i >= n * 3u { return; }
    let px = (i % n) % w;
    let py = (i % n) / w;
    let x = f32(p[2] + px);
    let y = f32(p[3] + py);
    let pn = vec2<f32>(2. * (x + 0.5) / f32(p[4]) - 1., 2. * (y + 0.5) / f32(p[5]) - 1.);
    var value = src[i];
    if (p[6] & 1u) != 0u {
        let v = vec3<f32>(f(8u), f(9u), f(10u));
        let m = (pn - vec2<f32>(f(11u), f(12u))) * vec2<f32>(f(13u), f(14u));
        let r = m.x * m.x + m.y * m.y;
        let illumination = clamp(1. + r * (v.x + r * (v.y + r * v.z)), 0.125, 8.);
        let crop = vec4<f32>(f(18u), f(19u), f(20u), f(21u));
        var embedded = 1.;
        for (var g = 0u; g < p[7]; g++) {
            let b = 22u + g * 8u;
            let pixel = crop.xy + (pn + 1.) * crop.zw / 2.;
            let q = (pixel - vec2<f32>(f(b), f(b + 1u))) / f(b + 2u);
            let rr = q.x * q.x + q.y * q.y;
            let poly = (((f(b + 7u) * rr + f(b + 6u)) * rr + f(b + 5u)) * rr + f(b + 4u)) * rr + f(b + 3u);
            embedded = clamp(embedded * (1. + rr * poly), 0.125, 8.);
        }
        let gain = clamp(1. + (embedded / illumination - 1.) * f(15u), 0.125, 8.);
        value = clamp(value * gain, -3.4028234663852886e38f, 3.4028234663852886e38f);
    }
    if (p[6] & 2u) != 0u {
        let r = (pn.x * pn.x + pn.y * pn.y) / 2.;
        let powered = select(0., pow(r, f(17u)), r > 0.);
        value = clamp(value * exp2(f(16u) * powered), -3.4028234663852886e38f, 3.4028234663852886e38f);
    }
    dst[i] = value;
}

// ── composed inverse map + normalized Lanczos-3: geometry_mapped ──
// p: iw, ih, first, source rows, w, h, row0, rows, flags (1 transform,
//    2 lens, 4 sample), warps | crop cw, ch, cx, cy, sin, cos (10..15),
//    transform offset[2], rotate sin/cos, scale[2], perspective h/v (16..23),
//    manual k1 (24), sample k[3], p[2], centre[2], distortion scale,
//    odd[2], coordinate scale[2], amount (25..37), embedded distortion (38),
//    embedded crop[4] (39..42), per warp k[6], centre[2], radius (43 + 9i).
fn lanczos3(x: f32) -> f32 {
    if abs(x) < 1e-12 { return 1.; }
    if abs(x) >= 3. { return 0.; }
    let v = 3.14159265358979323846f * x;
    return sin(v) / v * sin(v / 3.) / (v / 3.);
}
// Correct the reciprocal-based GPU division before pixel-coordinate rounding.
fn divide(a: f32, b: f32) -> f32 {
    let q = a / b;
    return fma(fma(-q, b, a), 1. / b, q);
}
fn lens_map(p0: vec2<f32>) -> vec2<f32> {
    // Manual Brown-Conrady k1 around the centre.
    let r0 = dot(p0, p0);
    var q = p0 * (1. + r0 * f(24u));
    if (p[8] & 4u) != 0u {
        let c = vec2<f32>(f(30u), f(31u));
        let cs = vec2<f32>(f(35u), f(36u));
        let m = (q - c) * cs;
        let r = m.x * m.x + m.y * m.y;
        let s = 1. + r * (f(25u) + r * (f(26u) + r * f(27u)));
        let bc = vec2<f32>(
            m.x * s + 2. * f(28u) * m.x * m.y + f(29u) * (r + 2. * m.x * m.x),
            m.y * s + f(28u) * (r + 2. * m.y * m.y) + 2. * f(29u) * m.x * m.y,
        );
        let radius = length(m);
        let extra = f(32u) - 1. + f(33u) * radius + f(34u) * radius * radius * radius;
        let d = c + (bc + m * extra) / cs;
        q = q + f(37u) * (d - q);
    }
    let crop = vec4<f32>(f(39u), f(40u), f(41u), f(42u));
    for (var k = 0u; k < p[9]; k++) {
        let b = 43u + k * 9u;
        let centre = vec2<f32>(f(b + 6u), f(b + 7u));
        let radius = f(b + 8u);
        let pixel = crop.xy + (q + 1.) * crop.zw / 2.;
        let m = (pixel - centre) / radius;
        let r = m.x * m.x + m.y * m.y;
        let radial = f(b) + r * (f(b + 1u) + r * (f(b + 2u) + r * f(b + 3u)));
        let x = m.x * radial + 2. * f(b + 4u) * m.x * m.y + f(b + 5u) * (r + 2. * m.x * m.x);
        let y = m.y * radial + f(b + 4u) * (r + 2. * m.y * m.y) + 2. * f(b + 5u) * m.x * m.y;
        let green = vec2<f32>(
            2. * (centre.x + radius * x - crop.x) / crop.z - 1.,
            2. * (centre.y + radius * y - crop.y) / crop.w - 1.,
        );
        q = q + f(38u) * (green - q);
    }
    return q;
}
@compute @workgroup_size(64)
fn remap(@builtin(global_invocation_id) gid: vec3<u32>) {
    let iw = p[0]; let ih = p[1]; let first = p[2]; let rows_in = p[3];
    let w = p[4]; let h = p[5];
    let n = w * p[7];
    let i = index(gid);
    if i >= n { return; }
    let ox = i % w;
    let oy = p[6] + i / w;
    let cw = f(10u); let ch = f(11u);
    // Explicit fma(..., 0) rounds products before subsequent additions,
    // matching the reference's separate f32 operations (see geometry.wgsl).
    let dx = fma(divide(fma(f32(ox) + 0.5, cw, 0.), f32(w)), 1., -cw / 2.);
    let dy = fma(divide(fma(f32(oy) + 0.5, ch, 0.), f32(h)), 1., -ch / 2.);
    let sine = f(14u); let cosine = f(15u);
    var sx = fma(fma(cosine, dx, 0.), 1., f(12u)) + fma(sine, dy, 0.) - 0.5;
    var sy = fma(-fma(sine, dx, 0.), 1., f(13u)) + fma(cosine, dy, 0.) - 0.5;
    var valid = true;
    if (p[8] & 3u) != 0u {
        var q = vec2<f32>(2. * (sx + 0.5) / f32(iw) - 1., 2. * (sy + 0.5) / f32(ih) - 1.);
        if (p[8] & 1u) != 0u {
            q = q - vec2<f32>(f(16u), f(17u));
            let rs = f(18u); let rc = f(19u);
            q = vec2<f32>(
                rc * q.x + rs * q.y * f32(ih) / f32(iw),
                -rs * q.x * f32(iw) / f32(ih) + rc * q.y,
            );
            q = q / vec2<f32>(f(20u), f(21u));
            let d = 1. - f(22u) * q.x - f(23u) * q.y;
            if abs(d) < 1e-8 { valid = false; }
            q = q / d;
        }
        if (p[8] & 2u) != 0u {
            q = lens_map(q);
        }
        sx = (q.x + 1.) * f32(iw) / 2. - 0.5;
        sy = (q.y + 1.) * f32(ih) / 2. - 0.5;
    }
    let plane_out = n;
    let j = i;
    let plane_in = iw * rows_in;
    if !valid || !(abs(sx) < 3.4e38) || !(abs(sy) < 3.4e38) || sx < -0.5 || sy < -0.5
        || sx >= f32(iw) - 0.5 || sy >= f32(ih) - 0.5 {
        for (var c = 0u; c < 3u; c++) { dst[c * plane_out + j] = 0.; }
        return;
    }
    let x0 = i32(floor(sx));
    let y0 = i32(floor(sy));
    var sums = vec3<f32>(0.);
    var weights = 0.;
    for (var ky = y0 - 2; ky <= y0 + 3; ky++) {
        let wy = lanczos3(sy - f32(ky));
        let iy = clamp(clamp(ky, 0, i32(ih) - 1) - i32(first), 0, i32(rows_in) - 1);
        for (var kx = x0 - 2; kx <= x0 + 3; kx++) {
            let weight = lanczos3(sx - f32(kx)) * wy;
            let ix = u32(clamp(kx, 0, i32(iw) - 1));
            let at = u32(iy) * iw + ix;
            sums += vec3<f32>(src[at], src[plane_in + at], src[2u * plane_in + at]) * weight;
            weights += weight;
        }
    }
    let out = clamp(sums / weights, vec3<f32>(-3.4028234663852886e38f), vec3<f32>(3.4028234663852886e38f));
    dst[j] = out.x;
    dst[plane_out + j] = out.y;
    dst[2u * plane_out + j] = out.z;
}
