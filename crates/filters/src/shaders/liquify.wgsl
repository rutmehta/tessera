// Inverse offsets in image pixels, RG32Float nodes; manual interpolation avoids
// hardware filtering precision loss and works without FLOAT32_FILTERABLE.
struct Params { width: u32, height: u32, cell: u32, mode: u32 }
@group(0) @binding(0) var field: texture_2d<f32>;
@group(0) @binding(1) var<storage, read> source: array<vec4<f32>>;
@group(0) @binding(2) var<storage, read_write> output: array<vec4<f32>>;
@group(0) @binding(3) var<uniform> p: Params;
fn pixel(q: vec2<i32>) -> vec4<f32> {
    let c = clamp(q, vec2<i32>(0), vec2<i32>(i32(p.width)-1,i32(p.height)-1));
    return source[u32(c.y)*p.width+u32(c.x)];
}
fn cubic(x: f32) -> f32 {
    let t = abs(x);
    if t <= 1. { return 1.5*t*t*t-2.5*t*t+1.; }
    if t < 2. { return -0.5*t*t*t+2.5*t*t-4.*t+2.; }
    return 0.;
}
@compute @workgroup_size(8,8)
fn main(@builtin(global_invocation_id) id: vec3<u32>) {
    if id.x >= p.width || id.y >= p.height { return; }
    let g = vec2<f32>(id.xy) / f32(p.cell);
    let base = vec2<i32>(floor(g));
    let f = fract(g);
    let last = vec2<i32>(textureDimensions(field))-vec2<i32>(1);
    var d = vec2<f32>(0);
    for(var x=0; x<=1; x++) { for(var y=0; y<=1; y++) {
        let wx = select(1.-f.x,f.x,x==1);
        let wy = select(1.-f.y,f.y,y==1);
        d += textureLoad(field,min(base+vec2<i32>(x,y),last),0).xy * wx * wy;
    }}
    let q = clamp(vec2<f32>(id.xy)+d,vec2<f32>(0),vec2<f32>(f32(p.width-1),f32(p.height-1)));
    let i = vec2<i32>(floor(q));
    let t = fract(q);
    var value = vec4<f32>(0);
    if p.mode == 1u {
        for(var y=-1; y<=2; y++) { for(var x=-1; x<=2; x++) {
            value += pixel(i+vec2<i32>(x,y))*(cubic(f32(x)-t.x)*cubic(f32(y)-t.y));
        }}
    } else {
        for(var y=0; y<=1; y++) { for(var x=0; x<=1; x++) {
            let weight = select(1.-t.x,t.x,x==1)*select(1.-t.y,t.y,y==1);
            value += pixel(i+vec2<i32>(x,y))*weight;
        }}
    }
    output[id.y*p.width+id.x] = value;
}
