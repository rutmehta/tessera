// No surface or full pixel readback. One workgroup owns one 16K-pixel band.
// All bins/counts are integers, each band's payload is overwritten, not added
// to recycled storage. The host merges bands using u64.
@group(0) @binding(0) var<storage,read> src: array<f32>;
@group(0) @binding(1) var<storage,read_write> result: array<u32>;
@group(0) @binding(2) var<storage,read> p: array<u32>;
// Four shards bound contention on clipped/flat regions. 21 KiB shared memory.
var<workgroup> bins: array<atomic<u32>,5128>;
var<workgroup> decode: array<f32,256>;
fn linear(v: u32) -> f32 {
    let x=f32(v)/255.0;
    if x<=0.04045 { return x/12.92; }
    return pow((x+0.055)/1.055,2.4);
}
@compute @workgroup_size(256)
fn main(@builtin(workgroup_id) group:vec3<u32>,@builtin(local_invocation_index) lane:u32) {
    for(var j=lane;j<5128u;j+=256u) { atomicStore(&bins[j],0u); }
    decode[lane]=linear(lane);
    workgroupBarrier();
    let n=p[0];
    let band=group.x+group.y*65535u;
    let shard=(lane%4u)*1282u;
    var shadows=0u;
    var highlights=0u;
    for(var i=band*16384u+lane;i<min((band+1u)*16384u,n);i+=256u) {
        let r=u32(clamp(src[i],0.0,255.0));
        let g=u32(clamp(src[n+i],0.0,255.0));
        let b=u32(clamp(src[2u*n+i],0.0,255.0));
        let y=min(((2126u*r+7152u*g+722u*b)*256u)/2550000u,255u);
        let l=min(u32((0.2126*decode[r]+0.7152*decode[g]+0.0722*decode[b])*256.0),255u);
        atomicAdd(&bins[shard+r],1u); atomicAdd(&bins[shard+256u+g],1u);
        atomicAdd(&bins[shard+512u+b],1u); atomicAdd(&bins[shard+768u+y],1u);
        atomicAdd(&bins[shard+1024u+l],1u);
        if min(r,min(g,b))==0u { shadows+=1u; }
        if max(r,max(g,b))==255u { highlights+=1u; }
    }
    atomicAdd(&bins[shard+1280u],shadows);
    atomicAdd(&bins[shard+1281u],highlights);
    workgroupBarrier();
    for(var j=lane;j<1282u;j+=256u) {
        result[(p[1]+band)*1282u+j]=atomicLoad(&bins[j])+atomicLoad(&bins[1282u+j])+atomicLoad(&bins[2564u+j])+atomicLoad(&bins[3846u+j]);
    }
}
