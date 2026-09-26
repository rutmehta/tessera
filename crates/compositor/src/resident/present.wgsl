// Resident presentation: premultiplied level composite → an RGBA8 storage
// texture (an IOSurface on the app's device), either flattened over an
// opaque background or left premultiplied with alpha.

struct Present {
    lw: u32,
    sx: u32,
    sy: u32,
    dx: u32,
    dy: u32,
    w: u32,
    h: u32,
    flatten: u32,
    bg: vec4<f32>,
}

@group(0) @binding(0) var<storage, read> comp: array<vec4<f32>>;
@group(0) @binding(1) var<uniform> pr: Present;
@group(0) @binding(2) var dest: texture_storage_2d<rgba8unorm, write>;

@compute @workgroup_size(16, 16)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    if (gid.x >= pr.w || gid.y >= pr.h) { return; }
    let c = comp[(pr.sy + gid.y) * pr.lw + pr.sx + gid.x];
    var o = c;
    if (pr.flatten != 0u) { o = vec4<f32>(c.xyz + (1.0 - c.w) * pr.bg.xyz, 1.0); }
    textureStore(dest, vec2<u32>(pr.dx + gid.x, pr.dy + gid.y), clamp(o, vec4<f32>(0.0), vec4<f32>(1.0)));
}
