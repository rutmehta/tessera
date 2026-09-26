// Layer compositor tile program interpreter. Mirrors src/render/pixel.rs and
// src/blend.rs exactly (same formulas, same op order); see COMPOSITOR.md §6.

struct Header {
    n: u32,
    w: u32,
    ox: u32,
    oy: u32,
    count: u32,
    p0: u32,
    p1: u32,
    p2: u32,
}

struct Op {
    kind: u32,     // 0 blend, 2 push isolated/clip, 3 push pass, 4 pop blend, 5 pop pass, 6 snapshot bg
    mode: u32,     // BlendMode::index
    src: u32,      // offset of straight RGBA planes in srcs
    mask: u32,     // offset of a mask plane in srcs, or 0xffffffff
    flags: u32,    // bit0 atop, bits1-2 knockout (1 shallow, 2 deep), bit3 blend-if
    opacity: f32,
    fill: f32,
    seed: u32,
    bi: array<vec4<f32>, 8>,
}

@group(0) @binding(0) var<storage, read> hdr: Header;
@group(0) @binding(1) var<storage, read> ops: array<Op>;
@group(0) @binding(2) var<storage, read> srcs: array<f32>;
@group(0) @binding(3) var<storage, read_write> outp: array<f32>;

const NO_MASK: u32 = 0xffffffffu;

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
        if (d > 0.0) { o = vec3<f32>(l) + (o - vec3<f32>(l)) * l / d; } else { o = vec3<f32>(l); }
    }
    if (x > 1.0) {
        let d = x - l;
        if (d > 0.0) { o = vec3<f32>(l) + (o - vec3<f32>(l)) * (1.0 - l) / d; } else { o = vec3<f32>(l); }
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
    if (r > 0.0) { return (c - vec3<f32>(mn)) * s / r; }
    return vec3<f32>(0.0);
}

fn color_burn(b: f32, s: f32) -> f32 {
    if (b >= 1.0) { return 1.0; }
    if (s <= 0.0) { return 0.0; }
    return 1.0 - min((1.0 - b) / s, 1.0);
}

fn color_dodge(b: f32, s: f32) -> f32 {
    if (b <= 0.0) { return 0.0; }
    if (s >= 1.0) { return 1.0; }
    return min(b / (1.0 - s), 1.0);
}

fn hard_light(b: f32, s: f32) -> f32 {
    if (s <= 0.5) { return b * 2.0 * s; }
    let t = 2.0 * s - 1.0;
    return b + t - b * t;
}

fn blend_ch(mode: u32, b: f32, s: f32) -> f32 {
    switch mode {
        case 2u: { return min(b, s); }
        case 3u: { return b * s; }
        case 4u: { return color_burn(b, s); }
        case 5u: { return max(b + s - 1.0, 0.0); }
        case 7u: { return max(b, s); }
        case 8u: { return b + s - b * s; }
        case 9u: { return color_dodge(b, s); }
        case 10u: { return min(b + s, 1.0); }
        case 12u: { return hard_light(s, b); }
        case 13u: {
            if (s <= 0.5) { return 2.0 * b * s + b * b * (1.0 - 2.0 * s); }
            return 2.0 * b * (1.0 - s) + sqrt(max(b, 0.0)) * (2.0 * s - 1.0);
        }
        case 14u: { return hard_light(b, s); }
        case 15u: {
            if (s <= 0.5) { return color_burn(b, 2.0 * s); }
            return color_dodge(b, 2.0 * s - 1.0);
        }
        case 16u: { return clamp(b + 2.0 * s - 1.0, 0.0, 1.0); }
        case 17u: {
            if (s <= 0.5) { return min(b, 2.0 * s); }
            return max(b, 2.0 * s - 1.0);
        }
        case 18u: { if (b + s >= 1.0) { return 1.0; } return 0.0; }
        case 19u: { return abs(b - s); }
        case 20u: { return b + s - 2.0 * b * s; }
        case 21u: { return max(b - s, 0.0); }
        case 22u: {
            if (s <= 0.0) { if (b <= 0.0) { return 0.0; } return 1.0; }
            return min(b / s, 1.0);
        }
        default: { return s; }
    }
}

fn blend_px(mode: u32, b: vec3<f32>, s: vec3<f32>) -> vec3<f32> {
    switch mode {
        case 6u: { if (s.x + s.y + s.z < b.x + b.y + b.z) { return s; } return b; }
        case 11u: { if (s.x + s.y + s.z > b.x + b.y + b.z) { return s; } return b; }
        case 23u: { return set_lum(set_sat(s, sat(b)), lum(b)); }
        case 24u: { return set_lum(set_sat(b, sat(s)), lum(b)); }
        case 25u: { return set_lum(s, lum(b)); }
        case 26u: { return set_lum(b, lum(s)); }
        default: {
            return vec3<f32>(blend_ch(mode, b.x, s.x), blend_ch(mode, b.y, s.y), blend_ch(mode, b.z, s.z));
        }
    }
}

fn slider(v: f32, r: vec4<f32>) -> f32 {
    var lo = 1.0;
    if (r.y > 0.0 && v < r.y) {
        if (v < r.x) { lo = 0.0; } else { lo = (v - r.x) / (r.y - r.x); }
    }
    var hi = 1.0;
    if (r.z < 1.0 && v > r.z) {
        if (v > r.w) { hi = 0.0; } else { hi = (r.w - v) / (r.w - r.z); }
    }
    return min(lo, hi);
}

fn blend_if(k: u32, s: vec3<f32>, b: vec3<f32>) -> f32 {
    var w = slider(lum(s), ops[k].bi[0]) * slider(lum(b), ops[k].bi[1]);
    w = w * slider(s.x, ops[k].bi[2]) * slider(b.x, ops[k].bi[3]);
    w = w * slider(s.y, ops[k].bi[4]) * slider(b.y, ops[k].bi[5]);
    w = w * slider(s.z, ops[k].bi[6]) * slider(b.z, ops[k].bi[7]);
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
    if (p.w > 0.0) { return p.xyz * (1.0 / p.w); }
    return vec3<f32>(0.0);
}

fn composite(b: vec4<f32>, s: vec4<f32>, op: Op, oi: u32, kb_on: bool, k: vec4<f32>, x: u32, y: u32) -> vec4<f32> {
    let ab = b.w;
    let cb = unpremul(b);
    let cs = s.xyz;
    var sigma = s.w;
    if ((op.flags & 8u) != 0u) { sigma = sigma * blend_if(oi, cs, cb); }
    if (sigma <= 0.0) { return b; }
    let atop = (op.flags & 1u) != 0u;
    if (op.mode == 1u) {
        if (dissolve_threshold(x, y, op.seed) >= sigma * op.opacity * op.fill) { return b; }
        if (atop) { return vec4<f32>(ab * cs, ab); }
        return vec4<f32>(cs, 1.0);
    }
    if (!kb_on) {
        let a = sigma * op.opacity * op.fill;
        let bl = blend_px(op.mode, cb, cs);
        if (atop) {
            let kk = a * ab;
            return vec4<f32>(b.xyz + kk * (bl - cb), ab);
        }
        let u = a * (1.0 - ab);
        let v = a * ab;
        let w = 1.0 - a;
        return vec4<f32>(u * cs + v * bl + w * b.xyz, a + w * ab);
    }
    let fo = op.fill;
    let ka = k.w;
    let ck = unpremul(k);
    let bl = blend_px(op.mode, ck, cs);
    let u = fo * (1.0 - ka);
    let v = fo * ka;
    let w = 1.0 - fo;
    let r = vec4<f32>(u * cs + v * bl + w * k.xyz, fo + w * ka);
    let t = sigma * op.opacity;
    return b + t * (r - b);
}

@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let i = gid.x;
    let n = hdr.n;
    if (i >= n) { return; }
    let x = hdr.ox + i % hdr.w;
    let y = hdr.oy + i / hdr.w;
    var acc: array<vec4<f32>, 8>;
    var kinds: array<u32, 8>;
    var sp = 0u;
    acc[0] = vec4<f32>(0.0);
    kinds[0] = 0u;
    var deep = vec4<f32>(0.0);
    for (var k = 0u; k < hdr.count; k = k + 1u) {
        let op = ops[k];
        switch op.kind {
            case 0u, 4u: {
                var s: vec4<f32>;
                if (op.kind == 0u) {
                    s = vec4<f32>(srcs[op.src + i], srcs[op.src + n + i], srcs[op.src + 2u * n + i], srcs[op.src + 3u * n + i]);
                } else {
                    let c = acc[sp];
                    sp = sp - 1u;
                    s = vec4<f32>(unpremul(c), c.w);
                    if (op.mask != NO_MASK) { s.w = s.w * srcs[op.mask + i]; }
                }
                let knock = (op.flags >> 1u) & 3u;
                var kb_on = false;
                var kb = vec4<f32>(0.0);
                if (knock == 2u) {
                    kb_on = true;
                    kb = deep;
                } else if (knock == 1u) {
                    kb_on = true;
                    if (kinds[sp] == 0u) { kb = deep; }
                    else if (kinds[sp] == 2u) { kb = acc[sp - 1u]; }
                }
                acc[sp] = composite(acc[sp], s, op, k, kb_on, kb, x, y);
            }
            case 2u: {
                sp = sp + 1u;
                acc[sp] = vec4<f32>(0.0);
                kinds[sp] = 1u;
            }
            case 3u: {
                sp = sp + 1u;
                acc[sp] = acc[sp - 1u];
                kinds[sp] = 2u;
            }
            case 5u: {
                let c = acc[sp];
                sp = sp - 1u;
                var t = op.opacity * op.fill;
                if (op.mask != NO_MASK) { t = t * srcs[op.mask + i]; }
                acc[sp] = acc[sp] + t * (c - acc[sp]);
            }
            case 6u: {
                deep = acc[0];
            }
            default: {}
        }
    }
    let r = acc[0];
    outp[i] = r.x;
    outp[n + i] = r.y;
    outp[2u * n + i] = r.z;
    outp[3u * n + i] = r.w;
}
