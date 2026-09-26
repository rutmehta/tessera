// Resident whole-level Texture/Clarity/Dehaze (see resident_tone.rs).
//
// Separable clipped box means keep the scalar reference's accumulation order:
// a horizontal pass sums x0..x1 in order and divides by the clipped count, a
// vertical pass sums those means y0..y1 in order; no prefix sums (they
// cancel in f32).
// Min filters are order independent and therefore also separable.
//
// Every vec4 carries two (value, value) pairs with independent radii:
// a.xy, a.zw, b.xy, b.zw use radii p[3..7]; INACTIVE disables a pair.
// Planar level RGB (W*H per plane): the developed level, or a packed copy.
@group(0) @binding(0) var<storage, read> src: array<f32>;
@group(0) @binding(1) var<storage, read> ina: array<vec4<f32>>;
@group(0) @binding(2) var<storage, read> inb: array<vec4<f32>>;
@group(0) @binding(3) var<storage, read> low: array<vec4<f32>>;
@group(0) @binding(4) var<storage, read_write> outa: array<vec4<f32>>;
@group(0) @binding(5) var<storage, read_write> outb: array<vec4<f32>>;
@group(0) @binding(6) var<storage, read_write> zbuf: array<f32>;
// Planar output: the rect (tile or whole level), plane length p[23].
@group(0) @binding(7) var<storage, read_write> dst: array<f32>;
// 0 width, 1 height, 2 mode, 3..6 radii (6: slot roles in the fused kernels),
// 7 ox, 8 oy, 9 rect width, 10 rect height, 12 texture, 13 clarity,
// 17 strength (before confidence), 18 dehaze amount, 19 low width, 20 low height,
// Dehaze kernels bind (airlight.rgb, confidence) in low[0].
// 21 low factor, 22 flags, 23 output plane length.
@group(0) @binding(8) var<storage, read> p: array<u32>;

const INACTIVE: u32 = 0xffffffffu;
const EPS: f32 = 0.001;
const F_WIDE_LOW: u32 = 4u;
const F_PACK_Z: u32 = 16u;

fn pf(i: u32) -> f32 { return bitcast<f32>(p[i]); }
fn finite(v: f32) -> f32 { return clamp(v, -3.402823466e38, 3.402823466e38); }
fn luma(v: vec3<f32>) -> f32 { return finite(0.2627 * v.x + 0.678 * v.y + 0.0593 * v.z); }
fn encode(v: f32) -> f32 {
    if v > 6.125082e37 { return (log(v) - log(0.18)) / log(1.0 + 1.0 / 0.18); }
    return log(1.0 + v / 0.18) / log(1.0 + 1.0 / 0.18);
}
fn decode(v: f32) -> f32 {
    let x = v * log(1.0 + 1.0 / 0.18);
    if x >= 80.0 { return finite(exp(x + log(0.18))); }
    return 0.18 * (exp(x) - 1.0);
}
fn z_of(v: vec3<f32>) -> f32 { return encode(max(luma(v), 0.0)); }
fn rgb_at(i: u32) -> vec3<f32> {
    let n = p[0] * p[1];
    return vec3<f32>(src[i], src[n + i], src[2u * n + i]);
}

// Load modes shared by the horizontal kernel.
const LOAD_BUFFERS: u32 = 0u;
const LOAD_DARK: u32 = 2u;    // raw dark channel (min filter on .x)
const LOAD_NORM: u32 = 3u;    // airlight-normalised dark channel (min on .x)


// Pairs 2/3 (the second vec4) carry data: otherwise inb/outb are untouched.
fn b_active() -> bool { return p[5] != INACTIVE || p[6] != INACTIVE; }

fn load_h(mode: u32, i: u32) -> array<vec4<f32>, 2> {
    var r: array<vec4<f32>, 2>;
    if mode == LOAD_BUFFERS {
        r[0] = ina[i];
        if b_active() { r[1] = inb[i]; }
    } else if mode == LOAD_DARK {
        let v = rgb_at(i);
        r[0] = vec4<f32>(max(min(v.x, min(v.y, v.z)), 0.0), 0.0, 0.0, 0.0);
    } else {
        let v = rgb_at(i) / low[0].xyz;
        r[0] = vec4<f32>(max(min(v.x, min(v.y, v.z)), 0.0), 0.0, 0.0, 0.0);
    }
    return r;
}


// Vertical modes.
const V_MEAN: u32 = 0u;
const V_SELF: u32 = 1u;       // (mean z, mean z^2) pairs -> guided (a, b)
const V_CROSS: u32 = 2u;      // a = (mg, mp, mgg, mgp) -> guided (a, b)
const V_DARK: u32 = 3u;       // vertical min -> (y, dark) statistics
const V_TRANS: u32 = 4u;      // vertical min -> guide/transmission moments
const V_DEHAZE: u32 = 6u;     // coefficient means -> haze removal output


fn self_guided(m: vec2<f32>) -> vec2<f32> {
    // The reference evaluates guide == input: mi = mp and mii = mip.
    let a = (m.y - m.x * m.x) / (max(m.y - m.x * m.x, 0.0) + EPS);
    return vec2<f32>(a, m.x - a * m.x);
}

fn bilinear(x: u32, y: u32) -> vec2<f32> {
    let lw = p[19]; let lh = p[20]; let f = f32(p[21]);
    let half = (f - 1.0) * 0.5;
    let u = clamp((f32(x) - half) / f, 0.0, f32(lw - 1u));
    let v = clamp((f32(y) - half) / f, 0.0, f32(lh - 1u));
    let x0 = u32(floor(u)); let y0 = u32(floor(v));
    let x1 = min(x0 + 1u, lw - 1u); let y1 = min(y0 + 1u, lh - 1u);
    let tx = u - f32(x0); let ty = v - f32(y0);
    let top = mix(low[y0 * lw + x0].xy, low[y0 * lw + x1].xy, tx);
    let bottom = mix(low[y1 * lw + x0].xy, low[y1 * lw + x1].xy, tx);
    return mix(top, bottom, ty);
}

// Texture/Clarity with no-new-extrema protection (the reference's range(z, 1)).
fn presence_from(v: vec3<f32>, z: f32, lo: f32, hi: f32, fine: f32, mid: f32, wide: f32) -> vec3<f32> {
    let t = clamp(z, 0.0, 1.0);
    let weight = 4.0 * t * (1.0 - t);
    let delta = pf(12u) * (fine - mid) + pf(13u) * weight * (mid - wide);
    let adjusted = clamp(z + delta, lo, hi);
    let lum = luma(v);
    if lum > 0.0 && adjusted != z {
        let gain = finite(decode(adjusted) / lum);
        return vec3<f32>(finite(v.x * gain), finite(v.y * gain), finite(v.z * gain));
    }
    return v;
}

fn write_out(x: u32, y: u32, v: vec3<f32>) {
    let n = p[23];
    let j = (y - p[8]) * p[9] + (x - p[7]);
    dst[j] = v.x;
    dst[n + j] = v.y;
    dst[2u * n + j] = v.z;
}


fn vertical_out(x: u32, y: u32, i: u32, mode: u32, ma: vec4<f32>, mb: vec4<f32>) {
    if mode == V_MEAN {
        outa[i] = ma;
        if b_active() { outb[i] = mb; }
    } else if mode == V_SELF {
        outa[i] = vec4<f32>(self_guided(ma.xy), self_guided(ma.zw));
        if b_active() { outb[i] = vec4<f32>(self_guided(mb.xy), 0.0, 0.0); }
    } else if mode == V_CROSS {
        let a = (ma.w - ma.x * ma.y) / (max(ma.z - ma.x * ma.x, 0.0) + EPS);
        outa[i] = vec4<f32>(a, ma.y - a * ma.x, 0.0, 0.0);
    } else {
        var v = rgb_at(i);
        if low[0].w == 0.0 { write_out(x, y, v); return; }
        let g = encode(max(luma(v), 0.0));
        let t = clamp(ma.x * g + ma.y, 0.15, 1.0);
        let air = low[0].xyz;
        let amount = pf(18u);
        for (var c = 0u; c < 3u; c++) {
            if v[c] >= 0.0 {
                if amount > 0.0 { v[c] = finite(max(air[c] + (v[c] - air[c]) / t, 0.0)); }
                else { v[c] = finite(v[c] * t + air[c] * (1.0 - t)); }
            }
        }
        write_out(x, y, v);
    }
}

// Separable passes over the rect. Neighbours are cached global reads:
// workgroup-memory strips measured slower on M4.
@compute @workgroup_size(64, 4)
fn box_h(@builtin(global_invocation_id) id: vec3<u32>) {
    let w = p[0]; let h = p[1]; let mode = p[2];
    let x = p[7] + id.x;
    let y = p[8] + id.y;
    if x >= p[7] + p[9] || y >= p[8] + p[10] || x >= w || y >= h { return; }
    var oa = vec4<f32>(0.0);
    var ob = vec4<f32>(0.0);
    if mode == LOAD_DARK || mode == LOAD_NORM {
        let r = i32(p[3]);
        var m = 3.402823466e38;
        for (var k = max(0, i32(x) - r); k < min(i32(w), i32(x) + r + 1); k++) {
            m = min(m, load_h(mode, y * w + u32(k))[0].x);
        }
        oa = vec4<f32>(m, 0.0, 0.0, 0.0);
    } else {
        for (var pair = 0u; pair < 4u; pair++) {
            let r = p[3u + pair];
            if r == INACTIVE { continue; }
            let lo = max(0, i32(x) - i32(r));
            let hi = min(i32(w), i32(x) + i32(r) + 1);
            var s = vec2<f32>(0.0);
            for (var k = lo; k < hi; k++) {
                var v: vec4<f32>;
                if pair >= 2u { v = inb[y * w + u32(k)]; } else { v = ina[y * w + u32(k)]; }
                s += select(v.xy, v.zw, (pair & 1u) == 1u);
            }
            let m = s / f32(hi - lo);
            if pair == 0u { oa = vec4<f32>(m, oa.zw); }
            else if pair == 1u { oa = vec4<f32>(oa.xy, m); }
            else if pair == 2u { ob = vec4<f32>(m, ob.zw); }
            else { ob = vec4<f32>(ob.xy, m); }
        }
    }
    let i = y * w + x;
    outa[i] = oa;
    if b_active() { outb[i] = ob; }
}

@compute @workgroup_size(16, 16)
fn box_v(@builtin(global_invocation_id) id: vec3<u32>) {
    let w = p[0]; let h = p[1]; let mode = p[2];
    let x = p[7] + id.x;
    let y = p[8] + id.y;
    if x >= p[7] + p[9] || y >= p[8] + p[10] || x >= w || y >= h { return; }
    let i = y * w + x;
    if mode == V_DARK || mode == V_TRANS {
        let r = i32(p[3]);
        var m = 3.402823466e38;
        for (var k = max(0, i32(y) - r); k < min(i32(h), i32(y) + r + 1); k++) {
            m = min(m, ina[u32(k) * w + x].x);
        }
        let v = rgb_at(i);
        let lum = max(luma(v), 0.0);
        if mode == V_DARK {
            outa[i] = vec4<f32>(lum, m, 0.0, 0.0);
        } else {
            let t = clamp(1.0 - 0.85 * (pf(17u) * low[0].w) * clamp(m, 0.0, 1.0), 0.15, 1.0);
            let g = encode(lum);
            outa[i] = vec4<f32>(g, t, g * g, g * t);
        }
        return;
    }
    var ma = vec4<f32>(0.0);
    var mb = vec4<f32>(0.0);
    for (var pair = 0u; pair < 4u; pair++) {
        let r = p[3u + pair];
        if r == INACTIVE { continue; }
        let lo = max(0, i32(y) - i32(r));
        let hi = min(i32(h), i32(y) + i32(r) + 1);
        var s = vec2<f32>(0.0);
        for (var k = lo; k < hi; k++) {
            var v: vec4<f32>;
            if pair >= 2u { v = inb[u32(k) * w + x]; } else { v = ina[u32(k) * w + x]; }
            s += select(v.xy, v.zw, (pair & 1u) == 1u);
        }
        let m = s / f32(hi - lo);
        if pair == 0u { ma = vec4<f32>(m, ma.zw); }
        else if pair == 1u { ma = vec4<f32>(ma.xy, m); }
        else if pair == 2u { mb = vec4<f32>(m, mb.zw); }
        else { mb = vec4<f32>(mb.xy, m); }
    }
    vertical_out(x, y, i, mode, ma, mb);
}

// Block means of (z, z^2) onto the preview guidance grid (factor p[21]);
// partial edge blocks average real samples only.
@compute @workgroup_size(64)
fn down(@builtin(global_invocation_id) grid: vec3<u32>,
        @builtin(num_workgroups) groups: vec3<u32>) {
    let id = vec3<u32>(grid.x + grid.y * groups.x * 64u, 0u, 0u);
    let lw = p[19]; let lh = p[20]; let f = p[21];
    if id.x >= lw * lh { return; }
    let lx = id.x % lw; let ly = id.x / lw;
    let w = p[0]; let h = p[1];
    var s = vec4<f32>(0.0);
    var n = 0.0;
    for (var y = ly * f; y < min(h, ly * f + f); y++) {
        for (var x = lx * f; x < min(w, lx * f + f); x++) {
            let z = zbuf[y * w + x];
            s += vec4<f32>(z, z * z, 0.0, 0.0);
            n += 1.0;
        }
    }
    outa[id.x] = s / n;
}

// Planar halo-free tile (src, plane p[23]) -> planar level (dst); optional z.
@compute @workgroup_size(64)
fn pack(@builtin(global_invocation_id) id: vec3<u32>) {
    let n = p[23];
    if id.x >= n { return; }
    let level = p[0] * p[1];
    let i = (p[8] + id.x / p[9]) * p[0] + p[7] + id.x % p[9];
    let v = vec3<f32>(src[id.x], src[n + id.x], src[2u * n + id.x]);
    dst[i] = v.x;
    dst[level + i] = v.y;
    dst[2u * level + i] = v.z;
    if (p[22] & F_PACK_Z) != 0u {
        zbuf[i] = z_of(v);
    }
}

// Planar level (src) -> planar rect (dst): identity output.
@compute @workgroup_size(64)
fn unpack(@builtin(global_invocation_id) grid: vec3<u32>,
          @builtin(num_workgroups) groups: vec3<u32>) {
    let id = vec3<u32>(grid.x + grid.y * groups.x * 64u, 0u, 0u);
    let n = p[23];
    if id.x >= n { return; }
    let i = (p[8] + id.x / p[9]) * p[0] + p[7] + id.x % p[9];
    let v = rgb_at(i);
    dst[id.x] = v.x;
    dst[n + id.x] = v.y;
    dst[2u * n + id.x] = v.z;
}

// Guide z of the whole level, when it was not written while packing.
@compute @workgroup_size(16, 16)
fn zpass(@builtin(global_invocation_id) id: vec3<u32>) {
    if id.x >= p[0] || id.y >= p[1] { return; }
    let i = id.y * p[0] + id.x;
    zbuf[i] = z_of(rgb_at(i));
}

// ---------------------------------------------------------------------------
// Fused 2D presence kernels: 16x16 threads and outputs. Each scale loads
// only its own halo. (32x32 blocks with 30 KiB of workgroup memory measured
// ~10x slower on M4.) Up to three guided scales ("slots") with radii
// p[3..6]; slot roles are in p[6]: fine | mid << 4 | wide << 8 (15 = absent).
// The horizontal then vertical clipped means keep the reference's
// summation order exactly.
const TILE: u32 = 16u;
var<workgroup> zs: array<f32, 1024>;             // (TILE + 2r)^2 guide samples
var<workgroup> hm: array<vec2<f32>, 512>;        // (TILE + 2r) rows x TILE
var<workgroup> cs: array<vec2<f32>, 1024>;       // (TILE + 2r)^2 coefficients

fn slot_radius(slot: u32) -> u32 { return p[3u + slot]; }
fn max_radius() -> u32 {
    var r = 0u;
    for (var slot = 0u; slot < 3u; slot++) {
        if slot_radius(slot) != INACTIVE { r = max(r, slot_radius(slot)); }
    }
    return r;
}

// Horizontal clipped means of (v, v^2) or (a, b) for one slot. `values`
// selects zs (moments of z) or cs (coefficient pairs); samples are stored
// with halo `stored` around the block at (gx0, gy0).
fn horizontal(lane: u32, gx0: u32, gy0: u32, r: u32, stored: u32, moments: bool) {
    let w = p[0]; let h = p[1];
    let span = TILE + 2u * stored;
    let sx = i32(gx0) - i32(stored);
    for (var k = lane; k < (TILE + 2u * r) * TILE; k += 256u) {
        let row = k / TILE;
        let x = gx0 + k % TILE;
        let gy = i32(gy0) - i32(r) + i32(row);
        var m = vec2<f32>(0.0);
        if x < w && gy >= 0 && gy < i32(h) {
            let base = (row + stored - r) * span;
            let lo = max(0, i32(x) - i32(r));
            let hi = min(i32(w), i32(x) + i32(r) + 1);
            var sum = vec2<f32>(0.0);
            for (var xx = lo; xx < hi; xx++) {
                let j = base + u32(xx - sx);
                if moments {
                    let v = zs[j];
                    sum += vec2<f32>(v, v * v);
                } else {
                    sum += cs[j];
                }
            }
            m = sum / f32(hi - lo);
        }
        hm[k] = m;
    }
}

fn vertical(lid: vec3<u32>, gy0: u32, r: u32) -> vec2<f32> {
    let h = p[1];
    let y = gy0 + lid.y;
    let lo = max(0, i32(y) - i32(r));
    let hi = min(i32(h), i32(y) + i32(r) + 1);
    var sum = vec2<f32>(0.0);
    for (var yy = lo; yy < hi; yy++) {
        sum += hm[u32(yy - (i32(gy0) - i32(r))) * TILE + lid.x];
    }
    return sum / f32(hi - lo);
}

// z -> self-guided coefficients (a, b) for every active slot.
@compute @workgroup_size(16, 16)
fn pres_coef(@builtin(local_invocation_id) lid: vec3<u32>,
             @builtin(local_invocation_index) lane: u32,
             @builtin(workgroup_id) group: vec3<u32>) {
    let w = p[0]; let h = p[1];
    let gx0 = p[7] + group.x * TILE;
    let gy0 = p[8] + group.y * TILE;
    let halo = max_radius();
    let span = TILE + 2u * halo;
    for (var k = lane; k < span * span; k += 256u) {
        let gx = i32(gx0) - i32(halo) + i32(k % span);
        let gy = i32(gy0) - i32(halo) + i32(k / span);
        var v = 0.0;
        if gx >= 0 && gy >= 0 && gx < i32(w) && gy < i32(h) { v = zbuf[u32(gy) * w + u32(gx)]; }
        zs[k] = v;
    }
    workgroupBarrier();
    var coef: array<vec2<f32>, 3>;
    for (var slot = 0u; slot < 3u; slot++) {
        let r = slot_radius(slot);
        let on = r != INACTIVE;
        if on { horizontal(lane, gx0, gy0, r, halo, true); }
        workgroupBarrier();
        if on && gx0 + lid.x < w && gy0 + lid.y < h {
            coef[slot] = self_guided(vertical(lid, gy0, r));
        }
        workgroupBarrier();
    }
    let x = gx0 + lid.x;
    let y = gy0 + lid.y;
    if x < w && y < h && x < p[7] + p[9] && y < p[8] + p[10] {
        let i = y * w + x;
        outa[i] = vec4<f32>(coef[0], coef[1]);
        if slot_radius(2u) != INACTIVE { outb[i] = vec4<f32>(coef[2], 0.0, 0.0); }
    }
}

fn coef_at(slot: u32, i: u32) -> vec2<f32> {
    if slot == 0u { return ina[i].xy; }
    if slot == 1u { return ina[i].zw; }
    return inb[i].xy;
}

// Coefficient means -> guided outputs -> Texture/Clarity, written to dst.
@compute @workgroup_size(16, 16)
fn pres_apply(@builtin(local_invocation_id) lid: vec3<u32>,
              @builtin(local_invocation_index) lane: u32,
              @builtin(workgroup_id) group: vec3<u32>) {
    let w = p[0]; let h = p[1];
    let gx0 = p[7] + group.x * TILE;
    let gy0 = p[8] + group.y * TILE;
    // Guide z with a one-pixel halo for the no-new-extrema range.
    let zspan = TILE + 2u;
    for (var k = lane; k < zspan * zspan; k += 256u) {
        let gx = i32(gx0) - 1 + i32(k % zspan);
        let gy = i32(gy0) - 1 + i32(k / zspan);
        var v = 0.0;
        if gx >= 0 && gy >= 0 && gx < i32(w) && gy < i32(h) { v = zbuf[u32(gy) * w + u32(gx)]; }
        zs[k] = v;
    }
    workgroupBarrier();
    let z = zs[(lid.y + 1u) * zspan + lid.x + 1u];
    var q: array<f32, 3>;
    for (var slot = 0u; slot < 3u; slot++) {
        let r = slot_radius(slot);
        let on = r != INACTIVE;
        if on {
            let span = TILE + 2u * r;
            for (var k = lane; k < span * span; k += 256u) {
                let gx = i32(gx0) - i32(r) + i32(k % span);
                let gy = i32(gy0) - i32(r) + i32(k / span);
                var v = vec2<f32>(0.0);
                if gx >= 0 && gy >= 0 && gx < i32(w) && gy < i32(h) {
                    v = coef_at(slot, u32(gy) * w + u32(gx));
                }
                cs[k] = v;
            }
        }
        workgroupBarrier();
        if on { horizontal(lane, gx0, gy0, r, r, false); }
        workgroupBarrier();
        if on && gx0 + lid.x < w && gy0 + lid.y < h {
            let m = vertical(lid, gy0, r);
            q[slot] = m.x * z + m.y;
        }
        workgroupBarrier();
    }
    let x = gx0 + lid.x;
    let y = gy0 + lid.y;
    if x >= w || y >= h || x >= p[7] + p[9] || y >= p[8] + p[10] { return; }
    let roles = p[6];
    let mid = q[(roles >> 4u) & 15u];
    var fine = mid;
    if (roles & 15u) < 3u { fine = q[roles & 15u]; }
    var wide = mid;
    if (p[22] & F_WIDE_LOW) != 0u {
        let c = bilinear(x, y);
        wide = c.x * z + c.y;
    } else if ((roles >> 8u) & 15u) < 3u {
        wide = q[(roles >> 8u) & 15u];
    }
    var lo = z; var hi = z;
    for (var yy = max(0, i32(y) - 1); yy < min(i32(h), i32(y) + 2); yy++) {
        for (var xx = max(0, i32(x) - 1); xx < min(i32(w), i32(x) + 2); xx++) {
            let n = zs[u32(yy - i32(gy0) + 1) * zspan + u32(xx - i32(gx0) + 1)];
            lo = min(lo, n); hi = max(hi, n);
        }
    }
    write_out(x, y, presence_from(rgb_at(y * w + x), z, lo, hi, fine, mid, wide));
}
