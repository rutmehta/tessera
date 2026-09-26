// Exact host f64 footprints; all pixel work stays on the GPU.
struct Footprint {
    p00: u32, p10: u32, p01: u32, p11: u32,
    ax: f32, ay: f32,
}
struct Params { count: u32, page_word: u32, pad0: u32, pad1: u32 }
@group(0) @binding(0) var<storage, read> child: array<vec4<f32>>;
@group(0) @binding(1) var<storage, read> samples: array<Footprint>;
@group(0) @binding(2) var<storage, read_write> pages: array<f32>;
@group(0) @binding(3) var<uniform> params: Params;

fn fetch(index: u32) -> vec4<f32> {
    if index == 0xffffffffu { return vec4<f32>(0.0); }
    return child[index];
}

@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) id: vec3<u32>) {
    let i = id.x;
    if i >= params.count { return; }
    let s = samples[i];
    let p00 = fetch(s.p00);
    let p10 = fetch(s.p10);
    let p01 = fetch(s.p01);
    let p11 = fetch(s.p11);
    let top = p00 + (p10 - p00) * s.ax;
    let bot = p01 + (p11 - p01) * s.ax;
    let v = top + (bot - top) * s.ay;
    var rgb = vec3<f32>(0.0);
    if v.a > 0.0 { rgb = v.rgb * (1.0 / v.a); }
    let base = params.page_word + i;
    pages[base] = rgb.r;
    pages[base + params.count] = rgb.g;
    pages[base + 2u * params.count] = rgb.b;
    pages[base + 3u * params.count] = v.a;
}
