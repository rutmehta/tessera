// Self-guided filter, radius R, eps EPS, per channel on RGBA f32, built from
// separable clipped-window box means. Four entry points = four dispatches:
//   hbox_in:   I          -> t1 = hmean(I),  t2 = hmean(I*I)
//   vbox_coef: t1, t2     -> a, b   (var = m2 - m*m, a = var/(var+eps), b = m - a m)
//   hbox_ab:   a, b       -> t1 = hmean(a), t2 = hmean(b)
//   vbox_out:  t1, t2, I  -> out = vmean(t1) * I + vmean(t2)
const R: i32 = 8;
const EPS: f32 = 1e-3;

@group(0) @binding(0) var<storage, read> s0: array<vec4<f32>>;
@group(0) @binding(1) var<storage, read> s1: array<vec4<f32>>;
@group(0) @binding(2) var<storage, read> s2: array<vec4<f32>>;
@group(0) @binding(3) var<storage, read_write> d0: array<vec4<f32>>;
@group(0) @binding(4) var<storage, read_write> d1: array<vec4<f32>>;

@compute @workgroup_size(16, 16, 1)
fn hbox_in(@builtin(global_invocation_id) gid: vec3<u32>) {
    let x = i32(gid.x);
    let y = i32(gid.y);
    if (x >= W || y >= H) { return; }
    let x0 = max(x - R, 0);
    let x1 = min(x + R, W - 1);
    var s = vec4<f32>(0.0);
    var s2v = vec4<f32>(0.0);
    for (var k = x0; k <= x1; k++) {
        let v = s0[y * W + k];
        s += v;
        s2v += v * v;
    }
    let inv = 1.0 / f32(x1 - x0 + 1);
    d0[y * W + x] = s * inv;
    d1[y * W + x] = s2v * inv;
}

@compute @workgroup_size(16, 16, 1)
fn vbox_coef(@builtin(global_invocation_id) gid: vec3<u32>) {
    let x = i32(gid.x);
    let y = i32(gid.y);
    if (x >= W || y >= H) { return; }
    let y0 = max(y - R, 0);
    let y1 = min(y + R, H - 1);
    var m = vec4<f32>(0.0);
    var m2 = vec4<f32>(0.0);
    for (var k = y0; k <= y1; k++) {
        m += s0[k * W + x];
        m2 += s1[k * W + x];
    }
    let inv = 1.0 / f32(y1 - y0 + 1);
    m = m * inv;
    m2 = m2 * inv;
    let v = max(m2 - m * m, vec4<f32>(0.0));
    let a = v / (v + vec4<f32>(EPS));
    d0[y * W + x] = a;
    d1[y * W + x] = m - a * m;
}

@compute @workgroup_size(16, 16, 1)
fn hbox_ab(@builtin(global_invocation_id) gid: vec3<u32>) {
    let x = i32(gid.x);
    let y = i32(gid.y);
    if (x >= W || y >= H) { return; }
    let x0 = max(x - R, 0);
    let x1 = min(x + R, W - 1);
    var sa = vec4<f32>(0.0);
    var sb = vec4<f32>(0.0);
    for (var k = x0; k <= x1; k++) {
        sa += s0[y * W + k];
        sb += s1[y * W + k];
    }
    let inv = 1.0 / f32(x1 - x0 + 1);
    d0[y * W + x] = sa * inv;
    d1[y * W + x] = sb * inv;
}

@compute @workgroup_size(16, 16, 1)
fn vbox_out(@builtin(global_invocation_id) gid: vec3<u32>) {
    let x = i32(gid.x);
    let y = i32(gid.y);
    if (x >= W || y >= H) { return; }
    let y0 = max(y - R, 0);
    let y1 = min(y + R, H - 1);
    var ma = vec4<f32>(0.0);
    var mb = vec4<f32>(0.0);
    for (var k = y0; k <= y1; k++) {
        ma += s0[k * W + x];
        mb += s1[k * W + x];
    }
    let inv = 1.0 / f32(y1 - y0 + 1);
    d0[y * W + x] = (ma * inv) * s2[y * W + x] + mb * inv;
}
