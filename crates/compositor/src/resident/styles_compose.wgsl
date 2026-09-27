// Backdrop-aware counterpart of render::effects::run_styles. All inputs are
// straight, independently mipped effect planes. Metadata is in auxiliary words.
fn style_sample(k: u32, offset: u32, q: Px) -> vec4<f32> {
    let region = vec3<u32>(steps[k].p[0].xyz);
    let i = offset + ((q.y - region.y) * region.z + q.x - region.x) * 4u;
    return vec4<f32>(aux[i], aux[i + 1u], aux[i + 2u], aux[i + 3u]);
}

fn style_plane(k: u32, i: u32, b: vec4<f32>, shape: f32, interior: bool, q: Px) -> vec4<f32> {
    let o = steps[k].t.w + i * 4u;
    var s = style_sample(k, bitcast<u32>(aux[o]), q);
    if interior {
        s.w = 0.0;
        if shape > 0.0 {
            s.w = clamp(pdiv(style_sample(k, bitcast<u32>(aux[o]), q).w, shape), 0.0, 1.0);
        }
    }
    let h = vec4<u32>(0u, bitcast<u32>(aux[o + 1u]), steps[k].h.z & 9u, 0u);
    let f = vec4<f32>(aux[o + 2u], 1.0, 0.0, 0.0);
    return composite_step(k, h, f, steps[k].u.y, b, s, false, vec4<f32>(0.0), q);
}

fn composite_styled(k: u32, before: vec4<f32>, kb_on: bool, kb: vec4<f32>, q: Px) -> vec4<f32> {
    var cur = before;
    let n = steps[k].u.x;
    var source = style_sample(k, steps[k].t.x, q);
    let shape = source.w;
    for (var i = 0u; i < n; i++) {
        let flags = bitcast<u32>(aux[steps[k].t.w + i * 4u + 3u]);
        if flags == 1u { cur = style_plane(k, i, cur, shape, false, q); }
    }
    let interior_before = cur;
    source.w = select(0.0, 1.0, shape > 0.0);
    var f = steps[k].f;
    f.x = 1.0;
    cur = composite_step(k, steps[k].h, f, steps[k].u.y, cur, source, kb_on, kb, q);
    for (var i = 0u; i < n; i++) {
        let flags = bitcast<u32>(aux[steps[k].t.w + i * 4u + 3u]);
        if flags == 0u { cur = style_plane(k, i, cur, shape, true, q); }
    }
    cur = interior_before + shape * (cur - interior_before);
    for (var i = 0u; i < n; i++) {
        let flags = bitcast<u32>(aux[steps[k].t.w + i * 4u + 3u]);
        if (flags & 2u) != 0u { cur = style_plane(k, i, cur, shape, false, q); }
    }
    return before + steps[k].f.x * (cur - before);
}
