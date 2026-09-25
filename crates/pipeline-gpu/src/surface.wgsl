@group(0) @binding(0) var<storage,read> src: array<f32>;
@group(0) @binding(1) var surface_out: texture_storage_2d<rgba8unorm,write>;
@group(0) @binding(2) var<storage,read> p: array<u32>;
@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) id_grid: vec3<u32>, @builtin(num_workgroups) id_groups: vec3<u32>) {
    // Rows of at most 65535 workgroups (see Batch::record).
    let id = vec3<u32>(id_grid.x + id_grid.y * id_groups.x * 64u, 0u, 0u);
    let i=id.x;let n=p[0]*p[1];if i>=n {return;}
    let xy=vec2<u32>(p[2]+i%p[0],p[3]+i/p[0]);
    textureStore(surface_out,xy,vec4<f32>(src[i]/255.0,src[n+i]/255.0,src[2u*n+i]/255.0,1.0));
}
