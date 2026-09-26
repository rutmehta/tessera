// Exact f32 source-center reconstruction, matching transform::sample.
struct Params { sw: u32, sh: u32, dw: u32, dh: u32, kernel: u32, pad0: u32, pad1: u32, pad2: u32 }
@group(0) @binding(0) var<storage, read> source: array<vec4<f32>>;
@group(0) @binding(1) var field: texture_2d<f32>;
@group(0) @binding(2) var<storage, read_write> destination: array<vec4<f32>>;
@group(0) @binding(3) var<uniform> params: Params;
fn pdiv(a: f32, b: f32) -> f32 { return a / b; }
fn texel(x: i32, y: i32) -> vec4<f32> {
    if x < 0 || y < 0 || x >= i32(params.sw) || y >= i32(params.sh) { return vec4<f32>(0.); }
    return source[u32(y) * params.sw + u32(x)];
}
fn weight(v: f32) -> f32 {
    let x = abs(v);
    if params.kernel == 1u { return max(1. - x, 0.); }
    if params.kernel == 2u {
        if x < 1. { return ((1.5 * x - 2.5) * x) * x + 1.; }
        if x < 2. { return ((-0.5 * x + 2.5) * x - 4.) * x + 2.; }
        return 0.;
    }
    if x == 0. { return 1.; }
    if x >= 3. { return 0.; }
    let p = 3.1415927410125732421875 * x;
    let q = pdiv(p, 3.);
    return pdiv(sin(p), p) * pdiv(sin(q), q);
}
fn sample_center(center: vec2<f32>) -> vec4<f32> {
    let p = center - vec2<f32>(0.5);
    // Negated conjunction rejects NaN as well as the invalid -1e20 sentinel.
    if !(p.x >= -3. && p.y >= -3. && p.x <= f32(params.sw) + 2. && p.y <= f32(params.sh) + 2.) {
        return vec4<f32>(0.);
    }
    if params.kernel == 0u { return texel(i32(floor(p.x + 0.5)), i32(floor(p.y + 0.5))); }
    var n = 4;
    if params.kernel == 1u { n = 2; }
    if params.kernel == 3u { n = 6; }
    let base = vec2<i32>(floor(p)) - vec2<i32>(n / 2 - 1);
    var wx: array<f32, 6>;
    var wy: array<f32, 6>;
    var sx = 0.;
    var sy = 0.;
    for (var i = 0; i < n; i++) {
        wx[i] = weight(p.x - f32(base.x + i));
        wy[i] = weight(p.y - f32(base.y + i));
        sx += wx[i]; sy += wy[i];
    }
    if n == 6 {
        for (var i = 0; i < n; i++) { wx[i] = pdiv(wx[i], sx); wy[i] = pdiv(wy[i], sy); }
    }
    var result = vec4<f32>(0.);
    for (var j = 0; j < n; j++) {
        for (var i = 0; i < n; i++) {
            let w = wx[i] * wy[j];
            result += texel(base.x + i, base.y + j) * w;
        }
    }
    return result;
}
@compute @workgroup_size(8, 8, 1)
fn main(@builtin(global_invocation_id) id: vec3<u32>) {
    if id.x >= params.dw || id.y >= params.dh { return; }
    let center = textureLoad(field, vec2<i32>(id.xy), 0).xy;
    destination[id.y * params.dw + id.x] = sample_center(center);
}
