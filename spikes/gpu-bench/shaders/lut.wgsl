// Linear sRGB -> Oklab -> 33^3 trilinear LUT (indexed by L, a+0.5, b+0.5) -> Oklab -> linear sRGB.
const N: i32 = 33;
@group(0) @binding(0) var<storage, read> src: array<vec4<f32>>;
@group(0) @binding(1) var<storage, read> lut: array<vec4<f32>>;
@group(0) @binding(2) var<storage, read_write> dst: array<vec4<f32>>;

fn cbrt1(v: f32) -> f32 { return sign(v) * pow(abs(v), 1.0 / 3.0); }

fn to_oklab(c: vec3<f32>) -> vec3<f32> {
    let l = 0.4122214708 * c.x + 0.5363325363 * c.y + 0.0514459929 * c.z;
    let m = 0.2119034982 * c.x + 0.6806995451 * c.y + 0.1073969566 * c.z;
    let s = 0.0883024619 * c.x + 0.2817188376 * c.y + 0.6299787005 * c.z;
    let l_ = cbrt1(l);
    let m_ = cbrt1(m);
    let s_ = cbrt1(s);
    return vec3<f32>(
        0.2104542553 * l_ + 0.7936177850 * m_ - 0.0040720468 * s_,
        1.9779984951 * l_ - 2.4285922050 * m_ + 0.4505937099 * s_,
        0.0259040371 * l_ + 0.7827717662 * m_ - 0.8086757660 * s_);
}

fn from_oklab(c: vec3<f32>) -> vec3<f32> {
    let l_ = c.x + 0.3963377774 * c.y + 0.2158037573 * c.z;
    let m_ = c.x - 0.1055613458 * c.y - 0.0638541728 * c.z;
    let s_ = c.x - 0.0894841775 * c.y - 1.2914855480 * c.z;
    let l = l_ * l_ * l_;
    let m = m_ * m_ * m_;
    let s = s_ * s_ * s_;
    return vec3<f32>(
        4.0767416621 * l - 3.3077115913 * m + 0.2309699292 * s,
        -1.2684380046 * l + 2.6097574011 * m - 0.3413193965 * s,
        -0.0041960863 * l - 0.7034186147 * m + 1.7076147010 * s);
}

fn node(i: i32, j: i32, k: i32) -> vec3<f32> {
    return lut[(k * N + j) * N + i].xyz;
}

fn lerp3(a: vec3<f32>, b: vec3<f32>, t: f32) -> vec3<f32> { return a + (b - a) * t; }

@compute @workgroup_size(16, 16, 1)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let x = i32(gid.x);
    let y = i32(gid.y);
    if (x >= W || y >= H) { return; }
    let p = src[y * W + x];
    let lab = to_oklab(p.xyz);
    let t = clamp(vec3<f32>(lab.x, lab.y + 0.5, lab.z + 0.5), vec3<f32>(0.0), vec3<f32>(1.0)) * 32.0;
    let i0 = min(vec3<i32>(floor(t)), vec3<i32>(31));
    let fr = t - vec3<f32>(i0);
    let c00 = lerp3(node(i0.x, i0.y, i0.z), node(i0.x + 1, i0.y, i0.z), fr.x);
    let c10 = lerp3(node(i0.x, i0.y + 1, i0.z), node(i0.x + 1, i0.y + 1, i0.z), fr.x);
    let c01 = lerp3(node(i0.x, i0.y, i0.z + 1), node(i0.x + 1, i0.y, i0.z + 1), fr.x);
    let c11 = lerp3(node(i0.x, i0.y + 1, i0.z + 1), node(i0.x + 1, i0.y + 1, i0.z + 1), fr.x);
    let c0 = lerp3(c00, c10, fr.y);
    let c1 = lerp3(c01, c11, fr.y);
    let o = lerp3(c0, c1, fr.z);
    dst[y * W + x] = vec4<f32>(from_oklab(o), p.w);
}
