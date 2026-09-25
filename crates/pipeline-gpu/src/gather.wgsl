@group(0) @binding(0) var<storage,read> src: array<u32>;
@group(0) @binding(1) var<storage,read_write> dst: array<u32>;
// Each workgroup copies one disjoint row span: source offset, destination offset, length.
@group(0) @binding(2) var<storage,read> spans: array<u32>;
@compute @workgroup_size(64)
fn main(@builtin(workgroup_id) group:vec3<u32>, @builtin(local_invocation_index) lane:u32) {
    let j=group.x*3u;
    for (var x=lane; x<spans[j+2u]; x+=64u) {dst[spans[j+1u]+x]=src[spans[j]+x];}
}
