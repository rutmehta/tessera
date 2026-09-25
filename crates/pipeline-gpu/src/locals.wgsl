@group(0) @binding(0) var<storage, read> base: array<f32>;
@group(0) @binding(1) var<storage, read> adjusted: array<f32>;
@group(0) @binding(2) var<storage, read> mask: array<f32>;
@group(0) @binding(3) var<storage, read_write> output: array<f32>;

@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) id: vec3<u32>) {
    let i = id.x;
    if i >= arrayLength(&base) { return; }
    let alpha = mask[i % arrayLength(&mask)];
    // Endpoint branches preserve identity and avoid needless cancellation.
    if alpha == 0.0 { output[i] = base[i]; }
    else if alpha == 1.0 { output[i] = adjusted[i]; }
    else { output[i] = base[i] + alpha * (adjusted[i] - base[i]); }
}
