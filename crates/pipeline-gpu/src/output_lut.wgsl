// Planar f32 RGB output transform. Red is the fastest LUT axis.
@group(0) @binding(0) var<storage, read> src: array<f32>;
@group(0) @binding(1) var<storage, read> lut: array<f32>;
@group(0) @binding(2) var<storage, read_write> dst: array<f32>;
fn node(p: vec3<u32>) -> vec3<f32> {
    let i = ((p.z * 33u + p.y) * 33u + p.x) * 3u;
    return vec3(lut[i], lut[i+1u], lut[i+2u]);
}
@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) id: vec3<u32>) {
    let n = arrayLength(&src) / 3u;
    let i = id.x;
    if (i >= n) { return; }
    let p = clamp(vec3(src[i], src[n+i], src[2u*n+i]), vec3(0.0), vec3(1.0)) * 32.0;
    let lo = min(vec3<u32>(floor(p)), vec3(31u));
    let t = p - vec3<f32>(lo);
    let c00 = mix(node(lo), node(lo+vec3(1u,0u,0u)), t.x);
    let c10 = mix(node(lo+vec3(0u,1u,0u)), node(lo+vec3(1u,1u,0u)), t.x);
    let c01 = mix(node(lo+vec3(0u,0u,1u)), node(lo+vec3(1u,0u,1u)), t.x);
    let c11 = mix(node(lo+vec3(0u,1u,1u)), node(lo+vec3(1u,1u,1u)), t.x);
    let out = mix(mix(c00,c10,t.y), mix(c01,c11,t.y), t.z);
    dst[i] = out.x;
    dst[n+i] = out.y;
    dst[2u*n+i] = out.z;
}
