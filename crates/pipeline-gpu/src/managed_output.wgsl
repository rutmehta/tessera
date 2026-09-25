@group(0) @binding(0) var<storage, read> src: array<f32>;
@group(0) @binding(1) var<storage, read> lut: array<f32>;
@group(0) @binding(2) var<storage, read> warnings: array<f32>;
@group(0) @binding(3) var<storage, read_write> dst: array<f32>;
@group(0) @binding(4) var<storage, read_write> flags: array<u32>;
@group(0) @binding(5) var<storage, read> params: array<u32>;
@group(0) @binding(6) var<storage, read> transfer: array<f32>;

fn node(p: vec3<u32>, warning: bool) -> vec3<f32> {
    let i = ((p.z * 33u + p.y) * 33u + p.x) * 3u;
    if warning { return vec3(warnings[i], warnings[i+1u], warnings[i+2u]); }
    return vec3(lut[i], lut[i+1u], lut[i+2u]);
}
fn sample_lut(rgb: vec3<f32>, warning: bool) -> vec3<f32> {
    // Extrapolate edge cells rather than clipping scene chroma before the CMM.
    var p = rgb * 32.0;
    if warning { p = clamp((rgb + vec3(0.5)) * 8.0, vec3(0.0), vec3(32.0)); }
    let lo = vec3<u32>(clamp(floor(p), vec3(0.0), vec3(31.0)));
    let t = p - vec3<f32>(lo);
    if warning {
        // Tetrahedra preserve the neutral diagonal. Trilinear interpolation of
        // gamut distances mixes saturated corners into neutral shadow pixels.
        var order = vec3(0u, 1u, 2u);
        if t[order.x] < t[order.y] { order = order.yxz; }
        if t[order.y] < t[order.z] { order = order.xzy; }
        if t[order.x] < t[order.y] { order = order.yxz; }
        var first = vec3(0u); first[order.x] = 1u;
        var second = first; second[order.y] = 1u;
        return node(lo, true) * (1.0 - t[order.x])
            + node(lo + first, true) * (t[order.x] - t[order.y])
            + node(lo + second, true) * (t[order.y] - t[order.z])
            + node(lo + vec3(1u), true) * t[order.z];
    }
    let c00 = mix(node(lo, warning), node(lo+vec3(1u,0u,0u), warning), t.x);
    let c10 = mix(node(lo+vec3(0u,1u,0u), warning), node(lo+vec3(1u,1u,0u), warning), t.x);
    let c01 = mix(node(lo+vec3(0u,0u,1u), warning), node(lo+vec3(1u,0u,1u), warning), t.x);
    let c11 = mix(node(lo+vec3(0u,1u,1u), warning), node(lo+vec3(1u,1u,1u), warning), t.x);
    let value = mix(mix(c00,c10,t.y), mix(c01,c11,t.y), t.z);
    if warning { return value; }
    if params[6] == 1u {
        return vec3(encode_channel(value.x, 0u), encode_channel(value.y, 1u), encode_channel(value.z, 2u));
    }
    return vec3(unshape(value.x), unshape(value.y), unshape(value.z));
}
fn encode_channel(v: f32, c: u32) -> f32 {
    if v < 0.0 { return mix(transfer[3u+c], transfer[c], -v); }
    if v > 1.0 { return mix(transfer[4097u*3u+c], transfer[4098u*3u+c], v-1.0); }
    let p = v * 4096.0;
    let lo = min(u32(floor(p)), 4095u);
    return mix(transfer[(lo+1u)*3u+c], transfer[(lo+2u)*3u+c], p-f32(lo));
}
fn unshape(v: f32) -> f32 {
    if v <= 0.0031308 { return v * 12.92; }
    return 1.055 * pow(v, 1.0 / 2.4) - 0.055;
}
fn in_gamut(rgb: vec3<f32>) -> bool {
    return all(rgb >= vec3(0.0)) && all(rgb <= vec3(1.0));
}
@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) id_grid: vec3<u32>, @builtin(num_workgroups) id_groups: vec3<u32>) {
    // Rows of at most 65535 workgroups (see Batch::record).
    let id = vec3<u32>(id_grid.x + id_grid.y * id_groups.x * 64u, 0u, 0u);
    let w = params[0]; let h = params[1]; let halo = params[2];
    let n = w * h; let i = id.x;
    if i >= n { return; }
    let stride = w + 2u * halo;
    let plane = stride * (h + 2u * halo);
    let j = (i / w + halo) * stride + i % w + halo;
    let v = vec3(src[j], src[plane+j], src[2u*plane+j]);
    let y = dot(v, vec3(0.2627, 0.6780, 0.0593));
    var toned = vec3(0.0);
    if y > 0.0 {
        let s = 1.0 / (1.0 + exp(1.5 * (bitcast<f32>(params[5]) - log(y))));
        toned = v * s / y;
    }
    let warning = sample_lut(toned, true);
    let threshold = bitcast<f32>(params[7]);
    flags[i] = select(0u, 1u, warning.x > threshold) | select(0u, 2u, warning.y > threshold);
    var encoded = sample_lut(toned, false);
    if params[3] == 1u && !in_gamut(encoded) {
        let grey = clamp(dot(toned, vec3(0.2627, 0.6780, 0.0593)), 0.0, 1.0);
        var lo = 0.0; var hi = 1.0;
        encoded = sample_lut(vec3(grey), false);
        for (var step = 0u; step < 18u; step++) {
            let chroma = (lo + hi) * 0.5;
            let candidate = sample_lut(vec3(grey) + chroma * (toned - vec3(grey)), false);
            if in_gamut(candidate) { lo = chroma; encoded = candidate; }
            else { hi = chroma; }
        }
    }
    encoded = clamp(encoded, vec3(0.0), vec3(1.0));
    if params[4] == 1u { encoded = floor(encoded * 255.0 + 0.5); }
    dst[i] = encoded.x; dst[n+i] = encoded.y; dst[2u*n+i] = encoded.z;
}
