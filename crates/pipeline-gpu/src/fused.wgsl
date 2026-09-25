// Amount-independent vignette mask / grain value of the effects block, per
// global pixel of its extent; a placeholder unless p[base + 27] is set.
@group(0) @binding(3) var<storage, read> effect_map: array<vec2<f32>>;
fn effects_mapped(rgb: vec3<f32>, i: u32) -> vec3<f32> {
    if p[base + 27u] == 0.0 { return effects_pixel(rgb, i); }
    if p[base + 26u] != 0.0 { return rgb; }
    let xy = effects_xy(i);
    let m = effect_map[u32(xy.y) * bitcast<u32>(p[base + 5u]) + u32(xy.x)];
    return effects_apply(rgb, m.x, m.y);
}
// Header offsets are absolute; scalar functions use the selected block base.
@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) id_grid: vec3<u32>, @builtin(num_workgroups) id_groups: vec3<u32>) {
    // Rows of at most 65535 workgroups (see Batch::record).
    let id = vec3<u32>(id_grid.x + id_grid.y * id_groups.x * 64u, 0u, 0u);
    base = 0u;
    let i = id.x;
    if i >= u32(p[6]) { return; }
    let stride = u32(p[1]) + 2u * u32(p[4]);
    let x = i32(i % stride) - i32(p[4]);
    let y = i32(i / stride) - i32(p[4]);
    let source_index = input_index(x,y);
    var rgb = read_rgb(source_index);
    base = u32(p[33]); if base != 0u { rgb = tone(rgb); }
    base = u32(p[34]); if base != 0u { rgb = curves(rgb); }
    base = u32(p[35]); if base != 0u { rgb = creative_color(rgb); }
    base = u32(p[36]); if base != 0u { rgb = effects_mapped(rgb,source_index); }
    base = u32(p[37]); if base != 0u { rgb = display(rgb,u32(x),u32(y)); }
    base = 0u;
    write_rgb(i,rgb);
}
