@group(0) @binding(0) var<storage, read> dims: array<u32>;
@group(0) @binding(1) var<storage, read> src: array<f32>;
@group(0) @binding(2) var<storage, read_write> dst: array<f32>;
@compute @workgroup_size(8,8)
fn main(@builtin(global_invocation_id) local: vec3<u32>) {
    let p = local.xy + vec2<u32>(dims[2], dims[3]);
    let w = dims[0]; let h = dims[1]; let ow = (w+1u)/2u; let oh = (h+1u)/2u;
    if (p.x >= dims[4] || p.y >= dims[5]) { return; }
    let n = w*h; let on = ow*oh; let oi = p.y*ow+p.x;
    var c = vec4<f32>(0.0); var count = 0.0;
    for(var y=0u;y<2u;y++) { for(var x=0u;x<2u;x++) {
        let sx=2u*p.x+x; let sy=2u*p.y+y;
        if(sx<w && sy<h) {
            let i=sy*w+sx; let a=src[3u*n+i];
            c += vec4<f32>(src[i]*a,src[n+i]*a,src[2u*n+i]*a,a); count+=1.0;
        }
    }}
    var rgb=vec3<f32>(0.0); if(c.w>0.0) { rgb=c.xyz/c.w; }
    dst[oi]=rgb.x; dst[on+oi]=rgb.y; dst[2u*on+oi]=rgb.z; dst[3u*on+oi]=c.w/count;
}
