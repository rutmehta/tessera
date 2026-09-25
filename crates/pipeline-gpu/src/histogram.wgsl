@group(0) @binding(0) var<storage,read> src: array<f32>;
@group(0) @binding(1) var<storage,read_write> result: array<atomic<u32>>;
@group(0) @binding(2) var<storage,read> p: array<u32>;
@group(0) @binding(3) var surface_out: texture_storage_2d<rgba8unorm,write>;
var<workgroup> bins: array<atomic<u32>,1024>;
@compute @workgroup_size(256)
fn main(@builtin(workgroup_id) group:vec3<u32>,@builtin(local_invocation_index) lane:u32) {
    for (var j=lane;j<1024u;j+=256u) {atomicStore(&bins[j],0u);}
    workgroupBarrier();
    let n=p[0]*p[1];
    for (var i=group.x*4096u+lane;i<min((group.x+1u)*4096u,n);i+=256u) {
        let r=u32(clamp(src[i],0.0,255.0));
        let g=u32(clamp(src[n+i],0.0,255.0));
        let b=u32(clamp(src[2u*n+i],0.0,255.0));
        let xy=vec2<u32>(p[2]+i%p[0],p[3]+i/p[0]);
        textureStore(surface_out,xy,vec4<f32>(f32(r)/255.0,f32(g)/255.0,f32(b)/255.0,1.0));
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
