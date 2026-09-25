// Ordered zeroing for recycled buffers within the single resident compute pass.
@group(0) @binding(0) var<storage, read_write> dst: array<u32>;
@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) id_grid: vec3<u32>, @builtin(num_workgroups) id_groups: vec3<u32>) {
    // Rows of at most 65535 workgroups (see Batch::record).
    let id = vec3<u32>(id_grid.x + id_grid.y * id_groups.x * 64u, 0u, 0u);
    if id.x < arrayLength(&dst) {
        dst[id.x] = 0u;
    }
}
