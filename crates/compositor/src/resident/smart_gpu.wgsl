// Exact host f64 footprints; all pixel work stays on the GPU.
// Bilinear records: 4 indices + 2 weights. Lanczos: 36 indices + 6 X + 6 Y weights.
struct Params { count: u32, page_word: u32, lanczos: u32, pad1: u32 }
@group(0) @binding(0) var<storage, read> child: array<vec4<f32>>;
@group(0) @binding(1) var<storage, read> samples: array<u32>;
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
    var v: vec4<f32>;
    if params.lanczos == 0u {
        let s = i * 6u;
        let p00 = fetch(samples[s]);
        let p10 = fetch(samples[s + 1u]);
        let p01 = fetch(samples[s + 2u]);
        let p11 = fetch(samples[s + 3u]);
        let ax = bitcast<f32>(samples[s + 4u]);
        let ay = bitcast<f32>(samples[s + 5u]);
        // Keep the legacy expression ordering unchanged for pinned RGBA8 output.
        let top = p00 + (p10 - p00) * ax;
        let bot = p01 + (p11 - p01) * ax;
        v = top + (bot - top) * ay;
    } else {
        let s = i * 48u;
        v = vec4<f32>(0.0);
        for (var y = 0u; y < 6u; y++) {
            var row = vec4<f32>(0.0);
            for (var x = 0u; x < 6u; x++) {
                row += fetch(samples[s + y * 6u + x]) * bitcast<f32>(samples[s + 36u + x]);
            }
            v += row * bitcast<f32>(samples[s + 42u + y]);
        }
    }
    var rgb = vec3<f32>(0.0);
    if v.a > 0.0 { rgb = v.rgb * (1.0 / v.a); }
    let base = params.page_word + i;
    pages[base] = rgb.r;
    pages[base + params.count] = rgb.g;
    pages[base + 2u * params.count] = rgb.b;
    pages[base + 3u * params.count] = v.a;
}
