// Mip pages: level n+1 is the 2×2 box of level n, clipped at odd edges,
// in the page layout of the level below: RGBA pages interleaved (8-bit: one
// word per texel, 16-bit: two, float: four), one-channel (mask) pages
// planar with 8/16-bit samples packed four/two to a word.
// colour alpha-weighted (COMPOSITOR.md §5). 8- and 16-bit pages use the
// exact integer form of render::mip_exact (bit-identical to the CPU, with
// 64-bit sums emulated in two words); float pages mirror
// Compositor::mip_float in f32. Prepended with pages.wgsl. One invocation
// writes one output word.

struct Job {
    dst: u32,
    k0: u32,
    k1: u32,
    k2: u32,
    k3: u32,
    cw: u32,       // child level extent
    ch: u32,
    tx: u32,       // output tile
    ty: u32,
    chans: u32,
    def_code: u32, // absent children, 8-bit code
    def_f: f32,    // absent children, normalized (16-bit / float)
}

@group(0) @binding(8) var<storage, read> jobs: array<Job>;
@group(0) @binding(9) var<uniform> pool: Pool;

fn kid(j: Job, k: u32) -> u32 {
    switch k {
        case 0u: { return j.k0; }
        case 1u: { return j.k1; }
        case 2u: { return j.k2; }
        default: { return j.k3; }
    }
}

fn page_store(page: u32, word: u32, v: u32) {
    let w = (page & 0x1fffffffu) * pool.page_words + word;
    switch page >> 29u {
        case 0u: { slab0[w] = v; }
        case 1u: { slab1[w] = v; }
        case 2u: { slab2[w] = v; }
        case 3u: { slab3[w] = v; }
        case 4u: { slab4[w] = v; }
        case 5u: { slab5[w] = v; }
        case 6u: { slab6[w] = v; }
        default: { slab7[w] = v; }
    }
}

struct U64 {
    hi: u32,
    lo: u32,
}

fn add64(a: U64, v: u32) -> U64 {
    let lo = a.lo + v;
    return U64(a.hi + select(0u, 1u, lo < a.lo), lo);
}

// floor((2·num + d) / (2·d)): round(num / d) with halves up, for num < 2^36
// and 0 < d < 2^19.
fn round_div(num: U64, d: u32) -> u32 {
    let n = add64(U64((num.hi << 1u) | (num.lo >> 31u), num.lo << 1u), d);
    let dd = 2u * d;
    var r = 0u;
    var q = 0u;
    for (var b = 39i; b >= 0i; b--) {
        var bit = 0u;
        if (b >= 32i) { bit = (n.hi >> u32(b - 32i)) & 1u; } else { bit = (n.lo >> u32(b)) & 1u; }
        r = (r << 1u) | bit;
        if (r >= dd) {
            r = r - dd;
            if (b < 32i) { q = q | (1u << u32(b)); }
        }
    }
    return q;
}

// Code (8/16-bit) or bits (float) of channel c of texel i.
fn child_code(page: u32, chans: u32, i: u32, c: u32) -> u32 {
    if (chans == 1u) { return page_code(page, i); }
    switch pool.depth {
        case 0u: { return (page_load(page, i) >> (8u * c)) & 255u; }
        case 1u: { return (page_load(page, 2u * i + (c >> 1u)) >> (16u * (c & 1u))) & 65535u; }
        default: { return page_load(page, 4u * i + c); }
    }
}

fn child_norm(page: u32, chans: u32, i: u32, c: u32, def_f: f32) -> f32 {
    if (page == NONE) { return def_f; }
    let v = child_code(page, chans, i, c);
    if (pool.depth == 1u) { return f32(v) * pool.inv16; }
    return bitcast<f32>(v);
}

// One output sample: channel c at output pixel (ox, oy).
fn mip_sample(j: Job, c: u32, ox: u32, oy: u32) -> u32 {
    let bx = 2u * j.tx * 256u;
    let by = 2u * j.ty * 256u;
    var num = U64(0u, 0u); var den = 0u; var cnt = 0u;
    var acc = 0.0; var acca = 0.0; var fcnt = 0.0;
    for (var dy = 0u; dy < 2u; dy++) {
        let cy = by + 2u * oy + dy;
        if (cy >= j.ch) { continue; }
        for (var dx = 0u; dx < 2u; dx++) {
            let cx = bx + 2u * ox + dx;
            if (cx >= j.cw) { continue; }
            let lx = cx - bx;
            let ly = cy - by;
            let page = kid(j, (ly / 256u) * 2u + lx / 256u);
            let ctx = cx / 256u;
            let ctw = min(256u, j.cw - ctx * 256u);
            let i = (ly % 256u) * ctw + (lx % 256u);
            cnt += 1u;
            fcnt += 1.0;
            if (pool.depth <= 1u) {
                var v = j.def_code;
                var a = j.def_code;
                if (page != NONE) {
                    v = child_code(page, j.chans, i, c);
                    if (j.chans == 4u) { a = child_code(page, 4u, i, 3u); }
                }
                if (j.chans == 4u) {
                    if (c < 3u) { num = add64(num, a * v); }
                    den += a;
                } else {
                    num = add64(num, v);
                }
            } else {
                if (j.chans == 4u) {
                    let a = child_norm(page, 4u, i, 3u, j.def_f);
                    if (c < 3u) { acc += a * child_norm(page, 4u, i, c, j.def_f); }
                    acca += a;
                } else {
                    acc += child_norm(page, 1u, i, 0u, j.def_f);
                }
            }
        }
    }
    cnt = max(cnt, 1u);
    if (pool.depth <= 1u) {
        if (j.chans == 4u) {
            if (c == 3u) { return (2u * den + cnt) / (2u * cnt); }
            if (den == 0u) { return 0u; }
            return round_div(num, den);
        }
        return round_div(num, cnt);
    }
    var v: f32;
    if (j.chans == 4u) {
        if (c == 3u) {
            v = acca / max(fcnt, 1.0);
        } else {
            var inv = 0.0;
            if (acca > 0.0) { inv = 1.0 / acca; }
            v = acc * inv;
        }
    } else {
        v = acc / max(fcnt, 1.0);
    }
    if (pool.depth == 1u) { return u32(clamp(v, 0.0, 1.0) * 65535.0 + 0.5); }
    return bitcast<u32>(v);
}

@compute @workgroup_size(256)
fn main(@builtin(global_invocation_id) gid: vec3<u32>, @builtin(workgroup_id) wg: vec3<u32>) {
    let j = jobs[wg.y];
    let lw = (j.cw + 1u) / 2u;
    let lh = (j.ch + 1u) / 2u;
    let tw = min(256u, lw - j.tx * 256u);
    let th = min(256u, lh - j.ty * 256u);
    let plane = tw * th;
    if (j.chans == 4u) {
        // One texel per invocation, interleaved.
        let i = gid.x;
        if (i >= plane) { return; }
        let ox = i % tw;
        let oy = i / tw;
        let r = mip_sample(j, 0u, ox, oy);
        let g = mip_sample(j, 1u, ox, oy);
        let b = mip_sample(j, 2u, ox, oy);
        let a = mip_sample(j, 3u, ox, oy);
        switch pool.depth {
            case 0u: { page_store(j.dst, i, r | (g << 8u) | (b << 16u) | (a << 24u)); }
            case 1u: {
                page_store(j.dst, 2u * i, r | (g << 16u));
                page_store(j.dst, 2u * i + 1u, b | (a << 16u));
            }
            default: {
                page_store(j.dst, 4u * i, r);
                page_store(j.dst, 4u * i + 1u, g);
                page_store(j.dst, 4u * i + 2u, b);
                page_store(j.dst, 4u * i + 3u, a);
            }
        }
        return;
    }
    // One-channel pages: one output word (4/2/1 samples) per invocation.
    var spw = 1u;
    if (pool.depth == 0u) { spw = 4u; } else if (pool.depth == 1u) { spw = 2u; }
    let word = gid.x;
    if (word * spw >= plane) { return; }
    var packed = 0u;
    for (var s = 0u; s < spw; s++) {
        let k = word * spw + s;
        if (k >= plane) { break; }
        let v = mip_sample(j, 0u, k % tw, k / tw);
        if (pool.depth == 0u) { packed |= v << (8u * s); }
        else if (pool.depth == 1u) { packed |= v << (16u * s); }
        else { packed = v; }
    }
    page_store(j.dst, word, packed);
}
