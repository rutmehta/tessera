struct Params {
    width: u32, sx: u32, sy: u32, dx: u32,
    dy: u32, w: u32, h: u32, flatten: u32,
    bg: vec3<f32>, managed: u32,
    headroom: f32, mode: u32, pad0: u32, pad1: u32,
}
@group(0) @binding(0) var<storage, read> source: array<vec4<f32>>;
@group(0) @binding(1) var<uniform> p: Params;
@group(0) @binding(2) var surface_out: texture_storage_2d<OUTPUT_FORMAT, write>;
// Red-fastest lattice, identical interpolation/domain to pipeline-gpu output_lut.
@group(0) @binding(3) var<storage, read> lut: array<f32>;
fn node(q: vec3<u32>) -> vec3<f32> {
    let i = ((q.z * 33u + q.y) * 33u + q.x) * 3u;
    return vec3(lut[i], lut[i+1u], lut[i+2u]);
}
fn transform(rgb: vec3<f32>) -> vec3<f32> {
    if p.mode == 1u {
        // Linear matrix-shaper ICC -> linear display. Basis evaluation is
        // unbounded: highlights >1 never enter the bounded LUT sampler.
        let black = node(vec3(0u));
        return black + rgb.x * (node(vec3(32u,0u,0u))-black)
            + rgb.y * (node(vec3(0u,32u,0u))-black)
            + rgb.z * (node(vec3(0u,0u,32u))-black);
    }
    let q = clamp(rgb, vec3(0.0), vec3(1.0)) * 32.0;
    let lo = min(vec3<u32>(floor(q)), vec3(31u));
    let t = q - vec3<f32>(lo);
    let c00 = mix(node(lo), node(lo+vec3(1u,0u,0u)), t.x);
    let c10 = mix(node(lo+vec3(0u,1u,0u)), node(lo+vec3(1u,1u,0u)), t.x);
    let c01 = mix(node(lo+vec3(0u,0u,1u)), node(lo+vec3(1u,0u,1u)), t.x);
    let c11 = mix(node(lo+vec3(0u,1u,1u)), node(lo+vec3(1u,1u,1u)), t.x);
    return mix(mix(c00,c10,t.y), mix(c01,c11,t.y), t.z);
}
@compute @workgroup_size(16,16)
fn main(@builtin(global_invocation_id) id: vec3<u32>) {
    if id.x >= p.w || id.y >= p.h { return; }
    var rgba = source[(id.y + p.sy) * p.width + id.x + p.sx];
    if rgba.a <= 0.0 { rgba = vec4(0.0); }
    else if p.managed != 0u { rgba = vec4(transform(rgba.rgb / rgba.a) * rgba.a, rgba.a); }
    if p.headroom > 0.0 { rgba = vec4(clamp(rgba.rgb, vec3(0.0), vec3(p.headroom * rgba.a)), rgba.a); }
    if p.flatten != 0u { rgba = vec4(rgba.rgb + p.bg * (1.0 - rgba.a), 1.0); }
    textureStore(surface_out, vec2<u32>(p.dx + id.x, p.dy + id.y), rgba);
}
