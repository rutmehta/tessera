// Appended to presence.wgsl: exact order statistics, no histogram quantisation.
// Merge sorted runs independently in each vec4 lane. One writer per output
// vector, with binary-search merge partitions; handles non-power-of-two levels.
@compute @workgroup_size(64)
fn stats_sort(@builtin(global_invocation_id) grid: vec3<u32>,
              @builtin(num_workgroups) groups: vec3<u32>) {
    let i = grid.x + grid.y * groups.x * 64u;
    let n = p[0] * p[1];
    if i >= n { return; }
    let width = p[11];
    let base = (i / (2u * width)) * (2u * width);
    let mid = min(base + width, n);
    let end = min(base + 2u * width, n);
    let a = mid - base;
    let b = end - mid;
    let k = i - base + 1u;
    var result: vec4<f32>;
    for (var c = 0u; c < 4u; c++) {
        var lo = k - min(k, b);
        var hi = min(k, a);
        while lo < hi {
            let take = (lo + hi) / 2u;
            let other = k - take;
            if other > 0u && take < a && ina[mid + other - 1u][c] > ina[base + take][c] {
                lo = take + 1u;
            } else { hi = take; }
        }
        let other = k - lo;
        var value = 0.0;
        if lo > 0u { value = ina[base + lo - 1u][c]; }
        if other > 0u { value = max(value, ina[mid + other - 1u][c]); }
        result[c] = value;
    }
    outa[i] = result;
}

// Rust f32::round rounds positive half-way cases upwards, unlike WGSL round.
fn quantile(n: u32, q: f32) -> u32 {
    let position = f32(n - 1u) * q;
    let lower = floor(position);
    // Do not add 0.5 before floor: above 2^23 that changes integral f32s.
    return min(u32(lower) + select(0u, 1u, position - lower >= 0.5), n - 1u);
}

// ina: sorted luminance/dark, inb: original luminance/dark. Invalid candidates
// sort after all finite RGB, while lane w gives the exact candidate count.
@compute @workgroup_size(64)
fn stats_candidates(@builtin(global_invocation_id) grid: vec3<u32>,
                    @builtin(num_workgroups) groups: vec3<u32>) {
    let i = grid.x + grid.y * groups.x * 64u;
    let n = p[0] * p[1];
    if i >= n { return; }
    let threshold = ina[quantile(n, 0.90)].y;
    let ceiling = ina[quantile(n, 0.99)].x;
    var v = vec4<f32>(3.402823466e38, 3.402823466e38, 3.402823466e38, 1.0);
    if inb[i].y >= threshold && inb[i].x <= ceiling {
        v = vec4<f32>(max(rgb_at(i), vec3<f32>(0.0)), 0.0);
    }
    outa[i] = v;
    if i == 0u {
        outb[0] = vec4<f32>(ina[quantile(n, 0.90)].x, ina[quantile(n, 0.10)].x, 0.0, 0.0);
    }
}

// One invocation only reads quantiles / binary searches candidate count.
// outa[0] is (airlight.rgb, confidence); confidence=0 is an exact identity.
@compute @workgroup_size(1)
fn stats_finish() {
    let n = p[0] * p[1];
    var lo = 0u;
    var hi = n;
    while lo < hi {
        let mid = (lo + hi) / 2u;
        if ina[mid].w == 0.0 { lo = mid + 1u; } else { hi = mid; }
    }
    outa[0] = vec4<f32>(1.0, 1.0, 1.0, 0.0);
    if lo == 0u { return; }
    let air = ina[quantile(lo, 0.5)].xyz;
    let ay = luma(air);
    if ay < 1e-8 { return; }
    let adjusted = max(clamp(air, vec3<f32>(0.75 * ay), vec3<f32>(finite(1.25 * ay))), vec3<f32>(1e-8));
    let t = clamp(((inb[0].x - inb[0].y) / ay - 0.05) / 0.20, 0.0, 1.0);
    outa[0] = vec4<f32>(adjusted, t * t * (3.0 - 2.0 * t));
}
