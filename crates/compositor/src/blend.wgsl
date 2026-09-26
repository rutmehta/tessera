// Blend-mode maths shared by every compositor shader. Mirrors src/blend.rs
// and src/render/pixel.rs exactly (same formulas, same op order); see
// COMPOSITOR.md §2–3.

// Every run-time f32 division goes through these. gpu_core's precise
// pipelines compile them as metal::precise::divide (correctly rounded, as
// on the CPU) while the rest of the shader may use relaxed maths.
fn pdiv(a: f32, b: f32) -> f32 {
    return a / b;
}

fn pdiv3(a: vec3<f32>, b: vec3<f32>) -> vec3<f32> {
    return a / b;
}

fn lum(c: vec3<f32>) -> f32 {
    return 0.3 * c.x + 0.59 * c.y + 0.11 * c.z;
}

fn clip_color(c: vec3<f32>) -> vec3<f32> {
    let l = lum(c);
    let n = min(min(c.x, c.y), c.z);
    let x = max(max(c.x, c.y), c.z);
    var o = c;
    if (n < 0.0) {
        let d = l - n;
        if (d > 0.0) { o = vec3<f32>(l) + pdiv3((o - vec3<f32>(l)) * l, vec3<f32>(d)); } else { o = vec3<f32>(l); }
    }
    if (x > 1.0) {
        let d = x - l;
        if (d > 0.0) { o = vec3<f32>(l) + pdiv3((o - vec3<f32>(l)) * (1.0 - l), vec3<f32>(d)); } else { o = vec3<f32>(l); }
    }
    return o;
}

fn set_lum(c: vec3<f32>, l: f32) -> vec3<f32> {
    let d = l - lum(c);
    return clip_color(c + vec3<f32>(d));
}

fn sat(c: vec3<f32>) -> f32 {
    return max(max(c.x, c.y), c.z) - min(min(c.x, c.y), c.z);
}

fn set_sat(c: vec3<f32>, s: f32) -> vec3<f32> {
    let mx = max(max(c.x, c.y), c.z);
    let mn = min(min(c.x, c.y), c.z);
    let r = mx - mn;
    if (r > 0.0) { return pdiv3((c - vec3<f32>(mn)) * s, vec3<f32>(r)); }
    return vec3<f32>(0.0);
}

// Separable modes are evaluated on all three channels at once (one switch
// per pixel); every component follows the scalar formula of blend.rs.
fn color_burn(b: vec3<f32>, s: vec3<f32>) -> vec3<f32> {
    let r = vec3<f32>(1.0) - min(pdiv3(vec3<f32>(1.0) - b, s), vec3<f32>(1.0));
    return select(select(r, vec3<f32>(0.0), s <= vec3<f32>(0.0)), vec3<f32>(1.0), b >= vec3<f32>(1.0));
}

fn color_dodge(b: vec3<f32>, s: vec3<f32>) -> vec3<f32> {
    let r = min(pdiv3(b, vec3<f32>(1.0) - s), vec3<f32>(1.0));
    return select(select(r, vec3<f32>(1.0), s >= vec3<f32>(1.0)), vec3<f32>(0.0), b <= vec3<f32>(0.0));
}

fn hard_light(b: vec3<f32>, s: vec3<f32>) -> vec3<f32> {
    let t = 2.0 * s - vec3<f32>(1.0);
    return select(b + t - b * t, b * 2.0 * s, s <= vec3<f32>(0.5));
}

fn blend_px(mode: u32, b: vec3<f32>, s: vec3<f32>) -> vec3<f32> {
    if (mode <= 1u) { return s; }
    let one = vec3<f32>(1.0);
    let zero = vec3<f32>(0.0);
    let half = vec3<f32>(0.5);
    switch mode {
        case 2u: { return min(b, s); }
        case 3u: { return b * s; }
        case 4u: { return color_burn(b, s); }
        case 5u: { return max(b + s - one, zero); }
        case 6u: { if (s.x + s.y + s.z < b.x + b.y + b.z) { return s; } return b; }
        case 7u: { return max(b, s); }
        case 8u: { return b + s - b * s; }
        case 9u: { return color_dodge(b, s); }
        case 10u: { return min(b + s, one); }
        case 11u: { if (s.x + s.y + s.z > b.x + b.y + b.z) { return s; } return b; }
        case 12u: { return hard_light(s, b); }
        case 13u: {
            let lo = 2.0 * b * s + b * b * (one - 2.0 * s);
            let hi = 2.0 * b * (one - s) + sqrt(max(b, zero)) * (2.0 * s - one);
            return select(hi, lo, s <= half);
        }
        case 14u: { return hard_light(b, s); }
        case 15u: {
            return select(color_dodge(b, 2.0 * s - one), color_burn(b, 2.0 * s), s <= half);
        }
        case 16u: { return clamp(b + 2.0 * s - one, zero, one); }
        case 17u: { return select(max(b, 2.0 * s - one), min(b, 2.0 * s), s <= half); }
        case 18u: { return select(zero, one, b + s >= one); }
        case 19u: { return abs(b - s); }
        case 20u: { return b + s - 2.0 * b * s; }
        case 21u: { return max(b - s, zero); }
        case 22u: {
            let q = min(pdiv3(b, s), one);
            return select(q, select(one, zero, b <= zero), s <= zero);
        }
        case 23u: { return set_lum(set_sat(s, sat(b)), lum(b)); }
        case 24u: { return set_lum(set_sat(b, sat(s)), lum(b)); }
        case 25u: { return set_lum(s, lum(b)); }
        case 26u: { return set_lum(b, lum(s)); }
        default: { return s; }
    }
}

fn slider(v: f32, r: vec4<f32>) -> f32 {
    var lo = 1.0;
    if (r.y > 0.0 && v < r.y) {
        if (v < r.x) { lo = 0.0; } else { lo = pdiv(v - r.x, r.y - r.x); }
    }
    var hi = 1.0;
    if (r.z < 1.0 && v > r.z) {
        if (v > r.w) { hi = 0.0; } else { hi = pdiv(r.w - v, r.w - r.z); }
    }
    return min(lo, hi);
}

fn blend_if(bi: array<vec4<f32>, 8>, s: vec3<f32>, b: vec3<f32>) -> f32 {
    var w = slider(lum(s), bi[0]) * slider(lum(b), bi[1]);
    w = w * slider(s.x, bi[2]) * slider(b.x, bi[3]);
    w = w * slider(s.y, bi[4]) * slider(b.y, bi[5]);
    w = w * slider(s.z, bi[6]) * slider(b.z, bi[7]);
    return w;
}

fn dissolve_threshold(x: u32, y: u32, seed: u32) -> f32 {
    var h = (x * 0x8da6b343u) ^ (y * 0xd8163841u) ^ (seed * 0xcb1ab31fu);
    h = h ^ (h >> 16u);
    h = h * 0x7feb352du;
    h = h ^ (h >> 15u);
    h = h * 0x846ca68bu;
    h = h ^ (h >> 16u);
    return f32(h >> 8u) * (1.0 / 16777216.0);
}

fn unpremul(p: vec4<f32>) -> vec3<f32> {
    if (p.w > 0.0) { return p.xyz * pdiv(1.0, p.w); }
    return vec3<f32>(0.0);
}

// Composites straight source colour `cs` with shape `sigma` (content α ×
// mask × Blend-If weight) onto premultiplied backdrop `b` (straight `cb`).
// Mirrors render/pixel.rs::blend_px. flags: bit0 atop.
fn composite_core(b: vec4<f32>, cb: vec3<f32>, cs: vec3<f32>, sigma: f32, mode: u32, flags: u32,
                  opacity: f32, fill: f32, seed: u32, kb_on: bool, k: vec4<f32>, x: u32, y: u32) -> vec4<f32> {
    let ab = b.w;
    if (sigma <= 0.0) { return b; }
    let atop = (flags & 1u) != 0u;
    if (mode == 1u) {
        if (dissolve_threshold(x, y, seed) >= sigma * opacity * fill) { return b; }
        if (atop) { return vec4<f32>(ab * cs, ab); }
        return vec4<f32>(cs, 1.0);
    }
    if (!kb_on) {
        let a = sigma * opacity * fill;
        let bl = blend_px(mode, cb, cs);
        if (atop) {
            let kk = a * ab;
            return vec4<f32>(b.xyz + kk * (bl - cb), ab);
        }
        let u = a * (1.0 - ab);
        let v = a * ab;
        let w = 1.0 - a;
        return vec4<f32>(u * cs + v * bl + w * b.xyz, a + w * ab);
    }
    let fo = fill;
    let ka = k.w;
    let ck = unpremul(k);
    let bl = blend_px(mode, ck, cs);
    let u = fo * (1.0 - ka);
    let v = fo * ka;
    let w = 1.0 - fo;
    let r = vec4<f32>(u * cs + v * bl + w * k.xyz, fo + w * ka);
    let t = sigma * opacity;
    return b + t * (r - b);
}
