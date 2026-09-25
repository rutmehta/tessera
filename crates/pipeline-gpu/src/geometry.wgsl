// Scalar f32 operation order follows pipeline-cpu geometry_effects.rs.
struct Params {
    iw: u32, ih: u32, w: u32, h: u32,
    cw: f32, ch: f32, cx: f32, cy: f32,
    sine: f32, cosine: f32,
}
@group(0) @binding(0) var<storage, read> src: array<f32>;
@group(0) @binding(1) var<storage, read_write> dst: array<f32>;
@group(0) @binding(2) var<storage, read> p: Params;
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
@compute @workgroup_size(8, 8, 1)
fn main(@builtin(global_invocation_id) id: vec3<u32>) {
    if id.x >= p.w || id.y >= p.h { return; }
    let index = id.z * p.w * p.h + id.y * p.w + id.x;
    // Explicit fma(..., 0) rounds products before subsequent additions;
    // Metal must not contract the CPU reference's separate operations.
    let dx = fma(divide(fma(f32(id.x) + 0.5, p.cw, 0.), f32(p.w)), 1., -p.cw / 2.);
    let dy = fma(divide(fma(f32(id.y) + 0.5, p.ch, 0.), f32(p.h)), 1., -p.ch / 2.);
    let sx = fma(fma(p.cosine, dx, 0.), 1., p.cx) + fma(p.sine, dy, 0.) - 0.5;
    let sy = fma(-fma(p.sine, dx, 0.), 1., p.cy) + fma(p.cosine, dy, 0.) - 0.5;
    if sx < -0.5 || sy < -0.5 || sx >= f32(p.iw) - 0.5 || sy >= f32(p.ih) - 0.5 {
        dst[index] = 0.;
        return;
    }
    var sum = 0.;
    var weights = 0.;
    let x0 = i32(floor(sx));
    let y0 = i32(floor(sy));
    for (var ky = y0 - 2; ky <= y0 + 3; ky++) {
        for (var kx = x0 - 2; kx <= x0 + 3; kx++) {
            let weight = lanczos3(sx - f32(kx)) * lanczos3(sy - f32(ky));
            let ix = u32(clamp(kx, 0, i32(p.iw) - 1));
            let iy = u32(clamp(ky, 0, i32(p.ih) - 1));
            sum += src[id.z * p.iw * p.ih + iy * p.iw + ix] * weight;
            weights += weight;
        }
    }
    dst[index] = clamp(sum / weights, -3.4028234663852886e38f, 3.4028234663852886e38f);
}
