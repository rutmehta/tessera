// Ordered zeroing for recycled buffers within the single resident compute pass.
@group(0) @binding(0) var<storage, read_write> dst: array<u32>;
@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) id: vec3<u32>) {
    if id.x < arrayLength(&dst) {
        dst[id.x] = 0u;
    }
}
