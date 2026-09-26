// Straight RGBA stack operations; blend.wgsl supplies exact arithmetic helpers.
@group(0) @binding(0) var<storage, read> input_pixels: array<vec4<f32>>;
@group(0) @binding(1) var<storage, read> next_pixels: array<vec4<f32>>;
@group(0) @binding(2) var<storage, read_write> output_pixels: array<vec4<f32>>;
@group(0) @binding(3) var<uniform> p: array<vec4<u32>, 2>;
@group(0) @binding(4) var<storage, read> mask_pixels: array<f32>;

@compute @workgroup_size(8, 8)
fn main(@builtin(global_invocation_id) id: vec3<u32>) {
    let op = p[0].x;
    let w = p[0].y;
    let h = p[0].z;
    let ow = p[1].y;
    let oh = p[1].z;
    if id.x >= ow || id.y >= oh { return; }
    let i = id.y * ow + id.x;
    if op == 4u {
        var acc = vec4<f32>(0.0);
        var count = 0.0;
        for(var y = 0u; y < 2u; y++) {
            for(var x = 0u; x < 2u; x++) {
                let q = id.xy * 2u + vec2<u32>(x,y);
                if q.x < w && q.y < h {
                    let a = input_pixels[q.y*w+q.x];
                    acc += vec4<f32>(a.a*a.rgb,a.a);
                    count += 1.0;
                }
            }
        }
        var inv = 0.0;
        if acc.a > 0.0 { inv = pdiv(1.0,acc.a); }
        output_pixels[i] = vec4<f32>(acc.rgb*inv,pdiv(acc.a,max(count,1.0)));
        return;
    }
    let a = input_pixels[i];
    if op == 0u { output_pixels[i] = vec4<f32>(unpremul(a),a.a); return; }
    if op == 1u { output_pixels[i] = vec4<f32>(a.rgb*a.a,a.a); return; }
    if op == 2u { output_pixels[i] = vec4<f32>(vec3<f32>(1.0)-a.rgb,a.a); return; }
    if op == 5u {
        var rgb = vec3<f32>(0.0);
        if a.a > 0.0 { rgb = pdiv3(a.rgb,vec3<f32>(a.a)); }
        output_pixels[i] = vec4<f32>(rgb,a.a); return;
    }
    let b = next_pixels[i];
    let mode = p[0].w;
    var t = bitcast<f32>(p[1].x);
    if op == 6u { t = 1.0-t*(1.0-mask_pixels[i]); }
    if mode == 1u { t = select(0.0,1.0,dissolve_threshold(id.x,id.y,0u)<t); }
    let alpha = a.a+t*(b.a-a.a);
    let v = a.rgb*a.a*(1.0-t)+blend_px(mode,a.rgb,b.rgb)*b.a*t;
    var rgb = vec3<f32>(0.0);
    if alpha > 0.0 { rgb = pdiv3(v,vec3<f32>(alpha)); }
    output_pixels[i] = vec4<f32>(rgb,alpha);
}
