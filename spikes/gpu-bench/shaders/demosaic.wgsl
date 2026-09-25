// Bilinear RGGB demosaic. Input: u16 CFA packed two-per-u32 (little endian,
// even index in the low half). Output: RGBA f32 per pixel.
@group(0) @binding(0) var<storage, read> cfa: array<u32>;
@group(0) @binding(1) var<storage, read_write> dst: array<vec4<f32>>;

const INV: f32 = 1.0 / 65535.0;

fn refl(v: i32, n: i32) -> i32 {
    var r = v;
    if (r < 0) { r = -r; }
    if (r >= n) { r = 2 * n - 2 - r; }
    return r;
}

fn f(x: i32, y: i32) -> f32 {
    let i = u32(refl(y, H) * W + refl(x, W));
    let word = cfa[i >> 1u];
    let v = (word >> ((i & 1u) * 16u)) & 0xffffu;
    return f32(v) * INV;
}

@compute @workgroup_size(16, 16, 1)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let x = i32(gid.x);
    let y = i32(gid.y);
    if (x >= W || y >= H) { return; }
    let c = f(x, y);
    let l = f(x - 1, y);
    let r = f(x + 1, y);
    let u = f(x, y - 1);
    let d = f(x, y + 1);
    let cross = (l + r + u + d) * 0.25;
    let diag = (f(x - 1, y - 1) + f(x + 1, y - 1) + f(x - 1, y + 1) + f(x + 1, y + 1)) * 0.25;
    let horiz = (l + r) * 0.5;
    let vert = (u + d) * 0.5;
    let px = x & 1;
    let py = y & 1;
    var o: vec3<f32>;
    if (py == 0 && px == 0) {
        o = vec3<f32>(c, cross, diag);
    } else if (py == 0) {
        o = vec3<f32>(horiz, c, vert);
    } else if (px == 0) {
        o = vec3<f32>(vert, c, horiz);
    } else {
        o = vec3<f32>(diag, cross, c);
    }
    dst[y * W + x] = vec4<f32>(o, 1.0);
}
