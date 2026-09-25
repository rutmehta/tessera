// Header offsets are absolute; scalar functions use the selected block base.
@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) id: vec3<u32>) {
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
    base = u32(p[36]); if base != 0u { rgb = effects_pixel(rgb,source_index); }
    base = u32(p[37]); if base != 0u { rgb = display(rgb,u32(x),u32(y)); }
    base = 0u;
    write_rgb(i,rgb);
}
