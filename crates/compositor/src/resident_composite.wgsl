// Appended to the shared blend functions in composite.wgsl.
@compute @workgroup_size(8, 8)
fn resident_main(@builtin(global_invocation_id) gid: vec3<u32>) {
    if (gid.x >= hdr.p1 || gid.y >= hdr.p2) { return; }
    let x = hdr.ox + gid.x;
    let y = hdr.oy + gid.y;
    let i = y * hdr.w + x;
    let n = hdr.n;
    var b = vec4<f32>(0.0);
    if (hdr.p0 != 0u) { b = vec4<f32>(outp[i], outp[n+i], outp[2u*n+i], outp[3u*n+i]); }
    if (ops[0].kind == 7u) {
        let c = unpremul(b);
        let rgb = c + ops[0].opacity * (adjustment(c, ops[0]) - c);
        outp[i] = rgb.x*b.w; outp[n+i] = rgb.y*b.w; outp[2u*n+i] = rgb.z*b.w;
        return;
    }
    let s = vec4<f32>(srcs[i], srcs[n+i], srcs[2u*n+i], srcs[3u*n+i]);
    let r = composite(b, s, ops[0], 0u, false, vec4<f32>(0.0), x, y);
    outp[i] = r.x; outp[n+i] = r.y; outp[2u*n+i] = r.z; outp[3u*n+i] = r.w;
}
