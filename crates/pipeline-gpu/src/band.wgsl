// Full-width export row bands (see Batch::gather_rows / finish_rows).
@group(0) @binding(0) var<storage, read> src: array<u32>;
@group(0) @binding(1) var<storage, read_write> dst: array<u32>;
@group(0) @binding(2) var<storage, read> p: array<u32>;

// The nearest in-range position with the same CFA phase (resample.rs
// clamp_phase): period 2 Bayer, 6 X-Trans, 1 RGB.
fn fold(v: i32, n: i32, period: i32) -> i32 {
    if v >= 0 && v < n { return v; }
    let phase = ((v % period) + period) % period;
    if phase >= n { return clamp(v, 0, n - 1); }
    if v < 0 { return phase; }
    return phase + (n - 1 - phase) / period * period;
}

// p: stride, rows (both including the halo), channels, halo, first interior
// row, frame width, frame height, period, source first row, source rows.
// The source is a halo-free band of the full frame width.
@compute @workgroup_size(64)
fn gather(@builtin(global_invocation_id) grid: vec3<u32>, @builtin(num_workgroups) groups: vec3<u32>) {
    let i = grid.x + grid.y * groups.x * 64u;
    let plane = p[0] * p[1];
    if i >= plane * p[2] { return; }
    let c = i / plane;
    let xy = i % plane;
    let halo = i32(p[3]);
    let x = fold(i32(xy % p[0]) - halo, i32(p[5]), i32(p[7]));
    let y = fold(i32(p[4]) + i32(xy / p[0]) - halo, i32(p[6]), i32(p[7])) - i32(p[8]);
    dst[i] = src[c * p[5] * p[9] + u32(y) * p[5] + u32(x)];
}

// Planar RGB (p[0] pixels per plane) to interleaved RGB rows.
@compute @workgroup_size(64)
fn interleave(@builtin(global_invocation_id) grid: vec3<u32>, @builtin(num_workgroups) groups: vec3<u32>) {
    let i = grid.x + grid.y * groups.x * 64u;
    let n = p[0];
    if i >= n * 3u { return; }
    dst[i] = src[(i % 3u) * n + i / 3u];
}
