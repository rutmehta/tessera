// Brush dab rasterizer: one invocation per pixel of the dirty rect loops
// over the batch's dabs in order, so per-pixel accumulation matches the CPU
// path exactly (alpha darken: flow builds up towards the dab opacity).
//
// params[0..16]: x0, y0, w, h, dab count, tip kind (0 round, 1 sampled),
//                tip w, tip h, hardness, wet edges, unused...
// params[16 + 10k ..]: x, y, size, angle, roundness, flow, opacity,
//                      flip_x, flip_y, half extent

@group(0) @binding(0) var<storage, read> params: array<f32>;
@group(0) @binding(1) var<storage, read> tip: array<f32>;
@group(0) @binding(2) var<storage, read_write> mask: array<f32>;

fn texel(x: i32, y: i32, w: i32, h: i32) -> f32 {
    if (x < 0 || y < 0 || x >= w || y >= h) {
        return 0.0;
    }
    return tip[u32(y * w + x)];
}

fn bilinear(x: f32, y: f32, w: i32, h: i32) -> f32 {
    let fx = x - 0.5;
    let fy = y - 0.5;
    let x0 = floor(fx);
    let y0 = floor(fy);
    let ax = fx - x0;
    let ay = fy - y0;
    let ix = i32(x0);
    let iy = i32(y0);
    let t00 = texel(ix, iy, w, h);
    let t10 = texel(ix + 1, iy, w, h);
    let t01 = texel(ix, iy + 1, w, h);
    let t11 = texel(ix + 1, iy + 1, w, h);
    let a = t00 + (t10 - t00) * ax;
    let b = t01 + (t11 - t01) * ax;
    return a + (b - a) * ay;
}

fn round_coverage(d: f32, radius: f32, hardness: f32) -> f32 {
    let h = clamp(hardness, 0.0, 1.0);
    let w = (1.0 - h) * radius + 1.0;
    let t = clamp((radius + 0.5 - d) / w, 0.0, 1.0);
    return t * t * (3.0 - 2.0 * t);
}

fn wet_edges(c: f32, rn: f32) -> f32 {
    let t = clamp((rn - 0.5) / 0.5, 0.0, 1.0);
    return c * (0.5 + 0.5 * t * t * (3.0 - 2.0 * t));
}

@compute @workgroup_size(8, 8)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let w = u32(params[2]);
    let h = u32(params[3]);
    if (gid.x >= w || gid.y >= h) {
        return;
    }
    let px = params[0] + f32(gid.x) + 0.5;
    let py = params[1] + f32(gid.y) + 0.5;
    let i = gid.y * w + gid.x;
    var m = mask[i];
    let n = u32(params[4]);
    let kind = u32(params[5]);
    let tw = i32(params[6]);
    let th = i32(params[7]);
    let hardness = params[8];
    let wet = params[9] > 0.5;
    for (var k = 0u; k < n; k = k + 1u) {
        let b = 16u + k * 10u;
        let dx = px - params[b];
        let dy = py - params[b + 1u];
        let e = params[b + 9u];
        if (abs(dx) > e || abs(dy) > e) {
            continue;
        }
        let size = params[b + 2u];
        let s = sin(params[b + 3u]);
        let c = cos(params[b + 3u]);
        var u = dx * c + dy * s;
        var v = -dx * s + dy * c;
        if (params[b + 7u] > 0.5) {
            u = -u;
        }
        if (params[b + 8u] > 0.5) {
            v = -v;
        }
        v = v / clamp(params[b + 4u], 0.01, 1.0);
        let radius = max(size * 0.5, 0.001);
        let d = sqrt(u * u + v * v);
        var cov = 0.0;
        if (kind == 0u) {
            cov = round_coverage(d, radius, hardness);
        } else {
            let scale = max(size, 0.001) / f32(max(tw, th));
            cov = bilinear(u / scale + f32(tw) * 0.5, v / scale + f32(th) * 0.5, tw, th);
        }
        if (cov <= 0.0) {
            continue;
        }
        if (wet) {
            cov = wet_edges(cov, d / radius);
        }
        let flow = params[b + 5u];
        let op = params[b + 6u];
        if (m < op) {
            m = m + (op - m) * min(flow * cov, 1.0);
        }
    }
    mask[i] = m;
}
