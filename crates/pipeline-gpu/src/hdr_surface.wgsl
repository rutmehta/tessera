// EDR surface writer: display-linear extended sRGB (Op::Display with a
// headroom) into an RGBA16F IOSurface, fused with the display histogram.
// The histogram bins the display-encoded sRGB of each value clamped to SDR
// white, so EDR highlights land in the top bin (as on the RGBA8 path).
@group(0) @binding(0) var<storage,read> src: array<f32>;
@group(0) @binding(1) var<storage,read_write> result: array<atomic<u32>>;
@group(0) @binding(2) var<storage,read> p: array<u32>;
@group(0) @binding(3) var surface_out: texture_storage_2d<rgba16float,write>;
var<workgroup> bins: array<atomic<u32>,1024>;
fn encode(v: f32) -> u32 {
    let l = clamp(v, 0.0, 1.0);
    var e = 12.92 * l;
    if l > 0.0031308 { e = 1.055 * pow(l, 1.0 / 2.4) - 0.055; }
    return u32(clamp(floor(e * 255.0 + 0.5), 0.0, 255.0));
}
@compute @workgroup_size(256)
fn main(@builtin(workgroup_id) group:vec3<u32>,@builtin(local_invocation_index) lane:u32) {
    for (var j=lane;j<1024u;j+=256u) {atomicStore(&bins[j],0u);}
    workgroupBarrier();
    let n=p[0]*p[1];
    for (var i=group.x*4096u+lane;i<min((group.x+1u)*4096u,n);i+=256u) {
        let v=vec3<f32>(src[i],src[n+i],src[2u*n+i]);
        let xy=vec2<u32>(p[2]+i%p[0],p[3]+i/p[0]);
        textureStore(surface_out,xy,vec4<f32>(v,1.0));
        let r=encode(v.x);let g=encode(v.y);let b=encode(v.z);
        let y=(54u*r+183u*g+19u*b)>>8u;
        atomicAdd(&bins[r],1u);atomicAdd(&bins[256u+g],1u);
        atomicAdd(&bins[512u+b],1u);atomicAdd(&bins[768u+y],1u);
    }
    workgroupBarrier();
    for (var j=lane;j<1024u;j+=256u) {
        let count=atomicLoad(&bins[j]);
        if count!=0u {atomicAdd(&result[j],count);}
    }
}
