// Scalar padded-mask operations and full-canvas straight-RGBA plane emission.
// All transcendental constants (Gaussian weights, light direction) come from
// the CPU reference's f32 arithmetic. Compile with precise_compute_pipeline.
struct Params {
    dims: vec4<u32>,       // padded width/height, canvas width/height
    control: vec4<u32>,    // padding, operation, integer radius, vertical
    flags: vec4<u32>,      // dilation / sampled fill, plane kind, option, highlight
    geometry: vec4<f32>,   // fractional morphology radius, dx, dy, unused
    color: vec4<f32>,
    lighting: vec4<f32>,   // lx, ly, sin(elevation), strength
    relief: vec4<f32>,     // direction, unused
    paint: vec4<f32>,
    origin: vec4<u32>,     // crop origin, stop count, unused
}
@group(0) @binding(0) var<storage, read> a: array<f32>;
@group(0) @binding(1) var<storage, read> b: array<f32>;
@group(0) @binding(2) var<storage, read> c: array<f32>;
@group(0) @binding(3) var<storage, read> auxiliary: array<f32>;
@group(0) @binding(4) var<storage, read_write> output: array<f32>;
@group(0) @binding(5) var<uniform> p: Params;

// This helper is replaced by gpu_core's correctly rounded Metal division.
fn pdiv(numerator: f32, denominator: f32) -> f32 { return numerator / denominator; }

fn paint_rgba(o: u32) -> vec4<f32> {
    return vec4<f32>(auxiliary[o], auxiliary[o + 1u], auxiliary[o + 2u], auxiliary[o + 3u]);
}
fn paint_sample(pos: vec2<u32>) -> vec4<f32> {
    let x = f32(pos.x + p.origin.x) + 0.5;
    let y = f32(pos.y + p.origin.y) + 0.5;
    let g = p.paint;
    if p.flags.x == 1u {
        let w = i32(g.x); let h = i32(g.y);
        let px = ((i32(floor(x - g.z)) % w) + w) % w;
        let py = ((i32(floor(y - g.w)) % h) + h) % h;
        return paint_rgba(u32(py * w + px) * 4u);
    }
    let dx = g.z - g.x; let dy = g.w - g.y;
    let len2 = dx * dx + dy * dy;
    var t = 0.0;
    if len2 > 0.0 {
        if p.flags.x == 2u { t = pdiv((x-g.x)*dx + (y-g.y)*dy, len2); }
        else { let ex = x-g.x; let ey = y-g.y; t = pdiv(sqrt(ex*ex+ey*ey), sqrt(len2)); }
    }
    t = clamp(t, 0.0, 1.0);
    if t <= auxiliary[0] { return paint_rgba(1u); }
    for (var i = 0u; i + 1u < p.origin.z; i++) {
        let a = i * 5u; let b = a + 5u;
        if t <= auxiliary[b] {
            let span = auxiliary[b] - auxiliary[a];
            var f = 1.0;
            if span > 0.0 { f = pdiv(t - auxiliary[a], span); }
            let c0 = paint_rgba(a + 1u); let c1 = paint_rgba(b + 1u);
            return c0 + (c1 - c0) * f;
        }
    }
    return paint_rgba((p.origin.z - 1u) * 5u + 1u);
}

fn in_mask(x: i32, y: i32) -> bool {
    return x >= 0 && y >= 0 && x < i32(p.dims.x) && y < i32(p.dims.y);
}
fn at_a(x: i32, y: i32) -> f32 {
    if (!in_mask(x, y)) { return 0.0; }
    return a[u32(y) * p.dims.x + u32(x)];
}
fn at_b(x: i32, y: i32) -> f32 {
    if (!in_mask(x, y)) { return 0.0; }
    return b[u32(y) * p.dims.x + u32(x)];
}
fn sample_b(x: f32, y: f32) -> f32 {
    let ix = i32(floor(x)) - i32(p.origin.x);
    let iy = i32(floor(y)) - i32(p.origin.y);
    let fx = x - floor(x);
    let fy = y - floor(y);
    let top = at_b(ix, iy) * (1.0 - fx) + at_b(ix + 1, iy) * fx;
    let bottom = at_b(ix, iy + 1) * (1.0 - fx) + at_b(ix + 1, iy + 1) * fx;
    return top * (1.0 - fy) + bottom * fy;
}

@compute @workgroup_size(8, 8, 1)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let op = p.control.y;
    let emitting = op == 4u;
    let width = select(p.dims.x, p.dims.z, emitting);
    let height = select(p.dims.y, p.dims.w, emitting);
    if (gid.x >= width || gid.y >= height) { return; }
    let idx = gid.y * width + gid.x;
    if (op == 0u) {
        // Source is straight interleaved RGBA; RGB must not affect geometry.
        let x = i32(gid.x) - i32(p.control.x);
        let y = i32(gid.y) - i32(p.control.x);
        var value = 0.0;
        if (x >= 0 && y >= 0 && x < i32(p.dims.z) && y < i32(p.dims.w)) {
            value = clamp(a[(u32(y) * p.dims.z + u32(x)) * 4u + 3u], 0.0, 1.0);
        }
        output[idx] = value;
        return;
    }
    if (op == 1u || op == 2u) {
        let radius = i32(p.control.z);
        var value = 0.0;
        if (op == 1u && p.flags.x == 0u) { value = 1.0; }
        for (var k = -radius; k <= radius; k += 1) {
            let x = i32(gid.x) + select(k, 0, p.control.w != 0u);
            let y = i32(gid.y) + select(0, k, p.control.w != 0u);
            let v = at_a(x, y);
            if (op == 2u) {
                value = value + auxiliary[u32(k + radius)] * v;
            } else if (p.flags.x != 0u) {
                value = max(value, v);
            } else {
                value = min(value, v);
            }
        }
        output[idx] = value;
        return;
    }
    if (op == 3u) {
        // Interpolate COMPLETE integer square footprints, not individual axes.
        output[idx] = a[idx] + (b[idx] - a[idx]) * p.geometry.x;
        return;
    }
    let x = gid.x + p.control.x;
    let y = gid.y + p.control.x;
    let i = y * p.dims.x + x;
    let alpha = a[i];
    let kind = p.flags.y;
    var coverage = 0.0;
    if (kind == 0u || kind == 1u) {
        let shifted = sample_b(f32(x + p.origin.x) - p.geometry.y, f32(y + p.origin.y) - p.geometry.z);
        coverage = shifted;
        if (kind == 1u) { coverage = alpha * (1.0 - shifted); }
    } else if (kind == 2u) {
        coverage = b[i];
    } else if (kind == 3u) {
        coverage = alpha * (1.0 - b[i]);
    } else if (kind == 4u) {
        coverage = alpha * b[i];
    } else if (kind == 5u) {
        coverage = alpha;
    } else if (kind == 6u) {
        coverage = max(b[i] - c[i], 0.0);
    } else if (kind == 7u) {
        let d = abs(sample_b(f32(x + p.origin.x) - p.geometry.y, f32(y + p.origin.y) - p.geometry.z)
                  - sample_b(f32(x + p.origin.x) + p.geometry.y, f32(y + p.origin.y) + p.geometry.z));
        var interior = d;
        if (p.flags.z != 0u) { interior = 1.0 - d; }
        coverage = alpha * interior;
    } else if (kind == 8u) {
        let gx = (at_b(i32(x) + 1, i32(y)) - at_b(i32(x) - 1, i32(y))) * 0.5;
        let gy = (at_b(i32(x), i32(y) + 1) - at_b(i32(x), i32(y) - 1)) * 0.5;
        let nx = -gx * p.lighting.w * p.relief.x;
        let ny = -gy * p.lighting.w * p.relief.x;
        let illumination = pdiv(nx * p.lighting.x + ny * p.lighting.y + p.lighting.z,
                                sqrt(nx * nx + ny * ny + 1.0)) - p.lighting.z;
        var band = max(alpha - c[i], 0.0);
        if (p.flags.z != 0u) { band = max(c[i] - alpha, 0.0); }
        var shading = max(-illumination, 0.0);
        if (p.flags.w != 0u) { shading = max(illumination, 0.0); }
        coverage = band * shading;
    }
    var color = p.color;
    if (p.flags.x != 0u) {
        color = paint_sample(gid.xy);
    }
    output[idx * 4u] = color.r;
    output[idx * 4u + 1u] = color.g;
    output[idx * 4u + 2u] = color.b;
    output[idx * 4u + 3u] = clamp(color.a * coverage, 0.0, 1.0);
}
