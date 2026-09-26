// The resident document program: one invocation per level pixel runs the
// whole flattened layer tree (render::exec semantics, COMPOSITOR.md §2–4)
// reading layer and mask texels straight from the page pool through
// per-raster page tables. Prepended with blend.wgsl and pages.wgsl.

struct Frame {
    lw: u32,          // level extent
    lh: u32,
    cols: u32,        // tile grid width at this level
    nsteps: u32,
    blocks: u32,      // 0: every 16² block of the level, else blocks[] count
    bcols: u32,
    brows: u32,
    clamp: u32,       // 1: clamp adjustment outputs to [0, 1]
    level: u32,
    _p0: u32,
    _p1: u32,
    _p2: u32,
}

// Grouped into 16-byte words so the hot fields are one load each.
struct Step {
    // kind (0 blend, 1 adjust, 2 push isolated/clip, 3 push pass, 4 pop
    // blend, 5 pop pass, 6 snapshot), blend mode, flags (bit0 atop, bits1-2
    // knockout, bit3 blend-if, bit4 mask, bit5 skip absent source tiles,
    // bit6 plain raster blend: none of the others and not Dissolve),
    // source (0 raster, 1 solid, 2 linear, 3 radial, 4 pattern, 5 constant,
    // 6 smart)
    h: vec4<u32>,
    // table, mask table, adjustment kind, aux offset
    t: vec4<u32>,
    // aux count, dissolve seed
    u: vec4<u32>,
    // opacity, fill, source default, mask density
    f: vec4<f32>,
    // effective mask where the mask has no tile
    g: vec4<f32>,
    p: array<vec4<f32>, 3>,
    bi: array<vec4<f32>, 8>,
}

@group(0) @binding(8) var<storage, read> smart: array<f32>;
@group(0) @binding(9) var<uniform> pool: Pool;
@group(0) @binding(10) var<uniform> frame: Frame;
@group(0) @binding(11) var<storage, read> steps: array<Step>;
@group(0) @binding(12) var<storage, read> tables: array<u32>;
@group(0) @binding(13) var<storage, read> aux: array<f32>;
@group(0) @binding(14) var<storage, read> blocks: array<u32>;
@group(0) @binding(15) var<storage, read_write> outp: array<vec4<f32>>;

// Normalized sample k of a planar one-channel (mask) page (aux[0..256] is
// the CPU's 8-bit LUT).
fn norm(page: u32, k: u32) -> f32 {
    let c = page_code(page, k);
    if (pool.depth == 0u) { return aux[c]; }
    if (pool.depth == 1u) { return f32(c) * pool.inv16; }
    return bitcast<f32>(c);
}

fn lut(offset: u32, v: f32) -> f32 {
    let x = clamp(v, 0.0, 1.0) * 4095.0;
    let i = min(u32(x), 4094u);
    let f = x - f32(i);
    return aux[offset + i] + (aux[offset + i + 1u] - aux[offset + i]) * f;
}

fn hsl_hue(p: f32, q: f32, t0: f32) -> f32 {
    let t = t0 - floor(t0);
    if (t < 1.0 / 6.0) { return p + (q - p) * 6.0 * t; }
    if (t < 0.5) { return q; }
    if (t < 2.0 / 3.0) { return p + (q - p) * (2.0 / 3.0 - t) * 6.0; }
    return p;
}

fn y601(c: vec3<f32>) -> f32 {
    return 0.299 * c.x + 0.587 * c.y + 0.114 * c.z;
}

// adjust.rs::Compiled::apply.
fn adjustment(k: u32, c: vec3<f32>) -> vec3<f32> {
    let a = steps[k].p[0];
    switch steps[k].t.z {
        case 0u: { return vec3<f32>(1.0) - c; }
        case 1u: { return pow(max(c * a.x + vec3<f32>(a.y), vec3<f32>(0.0)), vec3<f32>(a.z)); }
        case 2u: { return vec3<f32>(select(0.0, 1.0, y601(c) >= a.x)); }
        case 3u: {
            return min(floor(clamp(c, vec3<f32>(0.0), vec3<f32>(1.0)) * a.x), vec3<f32>(a.x - 1.0)) / (a.x - 1.0);
        }
        case 4u: {
            let o = steps[k].t.w;
            return vec3<f32>(lut(o + 12288u, lut(o, c.x)), lut(o + 12288u, lut(o + 4096u, c.y)),
                             lut(o + 12288u, lut(o + 8192u, c.z)));
        }
        case 5u: {
            let r0 = steps[k].p[0];
            let r1 = steps[k].p[1];
            let r2 = steps[k].p[2];
            return vec3<f32>(r0.x * c.x + r0.y * c.y + r0.z * c.z + r0.w,
                             r1.x * c.x + r1.y * c.y + r1.z * c.z + r1.w,
                             r2.x * c.x + r2.y * c.y + r2.z * c.z + r2.w);
        }
        case 6u: {
            let mx = max(max(c.x, c.y), c.z);
            let mn = min(min(c.x, c.y), c.z);
            var h = 0.0; var s = 0.0; var l = (mx + mn) / 2.0;
            if (mx != mn) {
                let d = mx - mn;
                if (l > 0.5) { s = d / (2.0 - mx - mn); } else { s = d / (mx + mn); }
                if (mx == c.x) { h = (c.y - c.z) / d + select(0.0, 6.0, c.y < c.z); }
                else if (mx == c.y) { h = (c.z - c.x) / d + 2.0; }
                else { h = (c.x - c.y) / d + 4.0; }
                h = h / 6.0;
            }
            if (a.w != 0.0) {
                let t = a.x;
                h = t - floor(t);
                s = max(a.y, 0.0);
                l = y601(c);
            } else {
                let t = h + a.x;
                h = t - floor(t);
                if (a.y < 0.0) { s = s * (1.0 + a.y); } else { s = s + (1.0 - s) * a.y; }
            }
            var rgb = vec3<f32>(l);
            if (s > 0.0) {
                var q = l + s - l * s;
                if (l < 0.5) { q = l * (1.0 + s); }
                let p = 2.0 * l - q;
                rgb = vec3<f32>(hsl_hue(p, q, h + 1.0 / 3.0), hsl_hue(p, q, h), hsl_hue(p, q, h - 1.0 / 3.0));
            }
            if (a.z < 0.0) { return rgb * (1.0 + a.z); }
            return rgb + (vec3<f32>(1.0) - rgb) * a.z;
        }
        default: { return c; }
    }
}

// Document::Fill::sample at level-0 point (fx, fy).
fn fill_sample(k: u32, fx: f32, fy: f32) -> vec4<f32> {
    let p = steps[k].p[0];
    let src = steps[k].h.w;
    if (src == 1u) { return vec4<f32>(p.xyz, 1.0); }
    if (src == 5u) { return p; }
    if (src == 4u) {
        let w = i32(p.x);
        let h = i32(p.y);
        let px = ((i32(floor(fx - p.z)) % w) + w) % w;
        let py = ((i32(floor(fy - p.w)) % h) + h) % h;
        let o = steps[k].t.w + u32(py * w + px) * 4u;
        return vec4<f32>(aux[o], aux[o + 1u], aux[o + 2u], aux[o + 3u]);
    }
    // Gradients: p = (start.x, start.y, end.x, end.y); stops (pos, rgba).
    let dx = p.z - p.x;
    let dy = p.w - p.y;
    let len2 = dx * dx + dy * dy;
    var t = 0.0;
    if (len2 > 0.0) {
        if (src == 2u) {
            t = ((fx - p.x) * dx + (fy - p.y) * dy) / len2;
        } else {
            let ex = fx - p.x;
            let ey = fy - p.y;
            t = sqrt(ex * ex + ey * ey) / sqrt(len2);
        }
    }
    t = clamp(t, 0.0, 1.0);
    let o = steps[k].t.w;
    let n = steps[k].u.x;
    if (n == 0u) { return vec4<f32>(0.0); }
    if (t <= aux[o]) { return vec4<f32>(aux[o + 1u], aux[o + 2u], aux[o + 3u], aux[o + 4u]); }
    for (var i = 0u; i + 1u < n; i++) {
        let a = o + 5u * i;
        let b = a + 5u;
        if (t <= aux[b]) {
            let span = aux[b] - aux[a];
            var f = 1.0;
            if (span > 0.0) { f = (t - aux[a]) / span; }
            let c0 = vec4<f32>(aux[a + 1u], aux[a + 2u], aux[a + 3u], aux[a + 4u]);
            let c1 = vec4<f32>(aux[b + 1u], aux[b + 2u], aux[b + 3u], aux[b + 4u]);
            return c0 + (c1 - c0) * f;
        }
    }
    let l = o + 5u * (n - 1u);
    return vec4<f32>(aux[l + 1u], aux[l + 2u], aux[l + 3u], aux[l + 4u]);
}

struct Px {
    tidx: u32,  // tile index in the level grid
    i: u32,     // index inside the tile plane
    plane: u32, // tile plane length
    x: u32,
    y: u32,
}

// Effective mask 1 − d·(1 − m) of step k at a pixel (`page`: its mask
// page in this tile).
fn mask_at(k: u32, page: u32, density: f32, q: Px) -> f32 {
    if (page == NONE) { return steps[k].g.x; }
    return 1.0 - density * (1.0 - norm(page, q.i));
}

// An RGBA texel of an interleaved content page, normalized.
fn texel(page: u32, i: u32) -> vec4<f32> {
    switch pool.depth {
        case 0u: {
            let w = page_load(page, i);
            return vec4<f32>(aux[w & 255u], aux[(w >> 8u) & 255u], aux[(w >> 16u) & 255u], aux[w >> 24u]);
        }
        case 1u: {
            let a = page_load(page, 2u * i);
            let b = page_load(page, 2u * i + 1u);
            return vec4<f32>(f32(a & 65535u), f32(a >> 16u), f32(b & 65535u), f32(b >> 16u)) * pool.inv16;
        }
        default: {
            return bitcast<vec4<f32>>(vec4<u32>(page_load(page, 4u * i), page_load(page, 4u * i + 1u),
                                                page_load(page, 4u * i + 2u), page_load(page, 4u * i + 3u)));
        }
    }
}

fn composite_step(k: u32, h: vec4<u32>, f: vec4<f32>, seed: u32, b: vec4<f32>, s: vec4<f32>,
                  kb_on: bool, kb: vec4<f32>, q: Px) -> vec4<f32> {
    let cb = unpremul(b);
    var sigma = s.w;
    if ((h.z & 8u) != 0u) { sigma = sigma * blend_if(steps[k].bi, s.xyz, cb); }
    return composite_core(b, cb, s.xyz, sigma, h.y, h.z, f.x, f.y, seed, kb_on, kb, q.x, q.y);
}

// Steps are staged through workgroup memory CHUNK at a time, together with
// this block's page-table entries, so the per-pixel loop reads them with
// shared-memory latency.
const CHUNK: u32 = 64u;
var<workgroup> sh_h: array<vec4<u32>, 64>;
var<workgroup> sh_f: array<vec4<f32>, 64>;
var<workgroup> sh_pg: array<vec3<u32>, 64>; // content page, mask page, seed
var<workgroup> sh_q: array<vec4<u32>, 64>;  // content page, mode, flags, opacity·fill

@compute @workgroup_size(16, 16)
fn main(@builtin(workgroup_id) wg: vec3<u32>, @builtin(num_workgroups) nwg: vec3<u32>,
        @builtin(local_invocation_id) li: vec3<u32>, @builtin(local_invocation_index) lix: u32) {
    let bi = wg.x + wg.y * nwg.x;
    var bx = 0u;
    var by = 0u;
    if (frame.blocks == 0u) {
        if (bi >= frame.bcols * frame.brows) { return; }
        bx = bi % frame.bcols;
        by = bi / frame.bcols;
    } else {
        if (bi >= frame.blocks) { return; }
        let b = blocks[bi];
        bx = b & 0xffffu;
        by = b >> 16u;
    }
    // The block lies in one tile. Threads past the level edge compute a
    // clamped pixel and do not store it (barriers need every thread).
    let inside = bx * 16u + li.x < frame.lw && by * 16u + li.y < frame.lh;
    let x = min(bx * 16u + li.x, frame.lw - 1u);
    let y = min(by * 16u + li.y, frame.lh - 1u);
    let tx = (bx * 16u) >> 8u;
    let ty = (by * 16u) >> 8u;
    let tidx = ty * frame.cols + tx;
    let tw = min(256u, frame.lw - tx * 256u);
    let th = min(256u, frame.lh - ty * 256u);
    let q = Px(tidx, (y & 255u) * tw + (x & 255u), tw * th, x, y);
    let scale = f32(1u << frame.level);
    let fx = (f32(x) + 0.5) * scale;
    let fy = (f32(y) + 0.5) * scale;

    var cur = vec4<f32>(0.0);
    var kind = 0u;              // current frame: 0 root, 1 isolated/clip, 2 pass-through
    // Saved parent frames live in registers (no dynamically indexed
    // private arrays); frame i's accumulator is s_i, its kind k_i.
    var s0 = vec4<f32>(0.0); var s1 = vec4<f32>(0.0); var s2 = vec4<f32>(0.0); var s3 = vec4<f32>(0.0);
    var s4 = vec4<f32>(0.0); var s5 = vec4<f32>(0.0); var s6 = vec4<f32>(0.0);
    var k0 = 0u; var k1 = 0u; var k2 = 0u; var k3 = 0u; var k4 = 0u; var k5 = 0u; var k6 = 0u;
    var sp = 0u;
    var deep = vec4<f32>(0.0);
    for (var c0 = 0u; c0 < frame.nsteps; c0 += CHUNK) {
        let n = min(CHUNK, frame.nsteps - c0);
        workgroupBarrier();
        if (lix < n) {
            let k = c0 + lix;
            let h = steps[k].h;
            let t = steps[k].t;
            sh_h[lix] = h;
            sh_f[lix] = steps[k].f;
            var pg = vec3<u32>(NONE, NONE, steps[k].u.y);
            if (h.x == 0u && (h.w == 0u || h.w == 6u)) { pg.x = tables[t.x + tidx]; }
            if ((h.z & 16u) != 0u) { pg.y = tables[t.y + tidx]; }
            sh_pg[lix] = pg;
            sh_q[lix] = vec4<u32>(pg.x, h.y, h.z, bitcast<u32>(steps[k].f.x * steps[k].f.y));
        }
        workgroupBarrier();
    for (var j = 0u; j < n; j++) {
        let fast = sh_q[j];
        if ((fast.z & 64u) != 0u) {
            // Plain raster blend (no mask, Blend If, knockout, clipping or
            // Dissolve; absent tiles are transparent): §2.1 directly. A
            // zero-alpha texel leaves `cur` exactly unchanged.
            if (fast.x != NONE) {
                let s = texel(fast.x, q.i);
                let ab = cur.w;
                let a = s.w * bitcast<f32>(fast.w);
                var bl = s.xyz;
                if (fast.y > 1u) { bl = blend_px(fast.y, unpremul(cur), s.xyz); }
                let u = a * (1.0 - ab);
                let v = a * ab;
                let w = 1.0 - a;
                // Preserve separately rounded products from the CPU executor.
                // Implicit contraction changes Pin Light by one ulp and can
                // flip a subsequent Hard Mix (see the one-pixel regression).
                // Do not change blend-mode tie rules or add an epsilon.
                let us = fma(vec3<f32>(u), s.xyz, vec3<f32>(0.0));
                let vb = fma(vec3<f32>(v), bl, vec3<f32>(0.0));
                let wc = fma(vec3<f32>(w), cur.xyz, vec3<f32>(0.0));
                cur = vec4<f32>(fma(vec3<f32>(1.0), us + vb, wc), a + fma(w, ab, 0.0));
            }
            continue;
        }
        let k = c0 + j;
        let h = sh_h[j];
        let f = sh_f[j];
        let pg = sh_pg[j];
        let op = h.x;
        if (op == 0u || op == 4u) {
            var s = vec4<f32>(0.0);
            var skip = false;
            if (op == 0u) {
                let src = h.w;
                if (src == 0u || src == 6u) {
                    let page = pg.x;
                    if (page == NONE) {
                        skip = (h.z & 32u) != 0u;
                        s = vec4<f32>(f.z);
                    } else if (src == 0u) {
                        s = texel(page, q.i);
                    } else {
                        let o = (page & 0x1fffffffu) * 262144u + q.i;
                        s = vec4<f32>(smart[o], smart[o + q.plane], smart[o + 2u * q.plane], smart[o + 3u * q.plane]);
                    }
                } else {
                    s = fill_sample(k, fx, fy);
                }
            } else {
                let c = cur;
                sp = sp - 1u;
                switch sp {
                    case 0u: { cur = s0; kind = k0; }
                    case 1u: { cur = s1; kind = k1; }
                    case 2u: { cur = s2; kind = k2; }
                    case 3u: { cur = s3; kind = k3; }
                    case 4u: { cur = s4; kind = k4; }
                    case 5u: { cur = s5; kind = k5; }
                    default: { cur = s6; kind = k6; }
                }
                s = vec4<f32>(unpremul(c), c.w);
            }
            if (!skip) {
                if ((h.z & 16u) != 0u) { s.w = s.w * mask_at(k, pg.y, f.w, q); }
                let knock = (h.z >> 1u) & 3u;
                var kb_on = false;
                var kb = vec4<f32>(0.0);
                if (knock == 2u) {
                    kb_on = true;
                    kb = deep;
                } else if (knock == 1u) {
                    kb_on = true;
                    if (kind == 0u) {
                        kb = deep;
                    } else if (kind == 2u) {
                        switch sp - 1u {
                            case 0u: { kb = s0; }
                            case 1u: { kb = s1; }
                            case 2u: { kb = s2; }
                            case 3u: { kb = s3; }
                            case 4u: { kb = s4; }
                            case 5u: { kb = s5; }
                            default: { kb = s6; }
                        }
                    }
                }
                cur = composite_step(k, h, f, pg.z, cur, s, kb_on, kb, q);
            }
        } else if (op == 1u) {
            if (cur.w > 0.0) {
                let cb = unpremul(cur);
                var a = adjustment(k, cb);
                if (frame.clamp != 0u) { a = clamp(a, vec3<f32>(0.0), vec3<f32>(1.0)); }
                var w = f.x * f.y;
                if ((h.z & 16u) != 0u) { w = w * mask_at(k, pg.y, f.w, q); }
                if ((h.z & 8u) != 0u) { w = w * blend_if(steps[k].bi, a, cb); }
                if (h.y == 1u) {
                    w = select(0.0, 1.0, dissolve_threshold(x, y, pg.z) < w);
                }
                if (w > 0.0) {
                    let bl = blend_px(h.y, cb, a);
                    let kk = w * cur.w;
                    cur = vec4<f32>(cur.xyz + kk * (bl - cb), cur.w);
                }
            }
        } else if (op == 2u || op == 3u) {
            switch sp {
                case 0u: { s0 = cur; k0 = kind; }
                case 1u: { s1 = cur; k1 = kind; }
                case 2u: { s2 = cur; k2 = kind; }
                case 3u: { s3 = cur; k3 = kind; }
                case 4u: { s4 = cur; k4 = kind; }
                case 5u: { s5 = cur; k5 = kind; }
                default: { s6 = cur; k6 = kind; }
            }
            sp = sp + 1u;
            if (op == 2u) { cur = vec4<f32>(0.0); kind = 1u; } else { kind = 2u; }
        } else if (op == 5u) {
            let c = cur;
            sp = sp - 1u;
            switch sp {
                case 0u: { cur = s0; kind = k0; }
                case 1u: { cur = s1; kind = k1; }
                case 2u: { cur = s2; kind = k2; }
                case 3u: { cur = s3; kind = k3; }
                case 4u: { cur = s4; kind = k4; }
                case 5u: { cur = s5; kind = k5; }
                default: { cur = s6; kind = k6; }
            }
            var t = f.x * f.y;
            if ((h.z & 16u) != 0u) { t = t * mask_at(k, pg.y, f.w, q); }
            cur = cur + t * (c - cur);
        } else if (op == 6u) {
            deep = cur;
        }
    }
    }
    if (inside) { outp[y * frame.lw + x] = cur; }
}
