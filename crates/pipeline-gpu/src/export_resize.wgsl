// Separable Lanczos-3. Coefficients are normalized in host f64/f32,
// matching the scalar exporter; all pixel filtering remains on the GPU.
@group(0) @binding(0) var<storage, read> src: array<f32>;
@group(0) @binding(1) var<storage, read_write> dst: array<f32>;
@group(0) @binding(2) var<storage, read> p: array<u32>;
@group(0) @binding(3) var<storage, read> offsets: array<u32>;
@group(0) @binding(4) var<storage, read> taps: array<u32>;
@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let n = p[2] * p[3];
    let i = gid.x + gid.y * 65535u * 64u;
    if (i >= n * 3u) { return; }
    let c = i / n;
    let x = (i % n) % p[2];
    let y = (i % n) / p[2];
    let axis = p[4];
    let a = select(x, y, axis == 1u);
    var value = 0.0;
    for (var t = offsets[a]; t < offsets[a + 1u]; t++) {
        let pos = taps[t * 2u];
        let weight = bitcast<f32>(taps[t * 2u + 1u]);
        let index = select(y * p[0] + pos, pos * p[0] + x, axis == 1u);
        value += src[c * p[0] * p[1] + index] * weight;
    }
    dst[i] = select(value, clamp(value, 0.0, 1.0), axis == 1u);
}
