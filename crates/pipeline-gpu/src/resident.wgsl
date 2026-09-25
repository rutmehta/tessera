// f16 cache storage uses packed pairs, so no optional shader-f16 capability is required.
@group(0) @binding(0) var<storage, read> src: array<u32>;
@group(0) @binding(1) var<storage, read_write> dst: array<u32>;
@group(0) @binding(2) var<storage, read> p: array<u32>;
// X-Trans parameters: opcode, width, height, halo, stride, origin phase x/y,
// followed by the row-major 6x6 CFA. Coordinates here are interior-relative.
fn xtrans_channel(x: i32, y: i32) -> u32 {
    let px = u32((x + i32(p[5]) + 6) % 6);
    let py = u32((y + i32(p[6]) + 6) % 6);
    return p[7u + py * 6u + px];
}
fn xtrans_sample(x: i32, y: i32) -> f32 {
    return bitcast<f32>(src[u32(y + i32(p[3])) * p[4] + u32(x + i32(p[3]))]);
}
fn xtrans_mean(x: i32, y: i32, channel: u32, radius: i32) -> vec2<f32> {
    var sum = 0.0;
    var count = 0.0;
    // Preserve the reference's row-major accumulation order.
    for (var dy = -radius; dy <= radius; dy++) {
        for (var dx = -radius; dx <= radius; dx++) {
            if xtrans_channel(x + dx, y + dy) == channel {
                sum += xtrans_sample(x + dx, y + dy);
                count += 1.0;
            }
        }
    }
    return vec2<f32>(sum, count);
}
fn xtrans_proxy(x: i32, y: i32, channel: u32) -> f32 {
    var sum = 0.0;
    var count = 0.0;
    for (var dy = -1; dy <= 1; dy++) {
        for (var dx = -1; dx <= 1; dx++) {
            let v = xtrans_sample(x + dx, y + dy);
            if xtrans_channel(x + dx, y + dy) != channel && v > 0.0 && v < 1.0 {
                sum += v;
                count += 1.0;
            }
        }
    }
    if count > 0.0 { return sum / count; }
    return 0.0;
}
fn xtrans_highlight(x: i32, y: i32) -> f32 {
    let v = xtrans_sample(x, y);
    if p[0] == 5u || v < 1.0 { return min(v, 1.0); }
    let channel = xtrans_channel(x, y);
    let target_proxy = xtrans_proxy(x, y, channel);
    var ratios = 0.0;
    var count = 0.0;
    for (var dy = -3; dy <= 3; dy++) {
        for (var dx = -3; dx <= 3; dx++) {
            let donor = xtrans_sample(x + dx, y + dy);
            if xtrans_channel(x + dx, y + dy) == channel && donor > 0.0 && donor < 1.0 {
                let proxy = xtrans_proxy(x + dx, y + dy, channel);
                if proxy > 1e-6 {
                    ratios += donor / proxy;
                    count += 1.0;
                }
            }
        }
    }
    if count > 0.0 && target_proxy > 0.0 { return clamp(target_proxy * ratios / count, 1.0, 4.0); }
    return 1.0;
}
@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) id_grid: vec3<u32>, @builtin(num_workgroups) id_groups: vec3<u32>) {
    // Rows of at most 65535 workgroups (see Batch::record).
    let id = vec3<u32>(id_grid.x + id_grid.y * id_groups.x * 64u, 0u, 0u);
    let i = id.x;
    if p[0] == 0u {
        if i >= (p[1]+1u)/2u { return; }
        let a = clamp(bitcast<f32>(src[2u*i]), -65504.0, 65504.0);
        var b = 0.0;
        if 2u*i+1u < p[1] { b = clamp(bitcast<f32>(src[2u*i+1u]), -65504.0, 65504.0); }
        dst[i] = pack2x16float(vec2<f32>(a,b));
    } else if p[0] == 1u {
        if i >= p[1] { return; }
        dst[i] = bitcast<u32>(unpack2x16float(src[i/2u])[i%2u]);
    } else if p[0] == 5u || p[0] == 6u {
        if i >= p[1] * p[2] { return; }
        dst[i] = bitcast<u32>(xtrans_highlight(i32(i % p[1]), i32(i / p[1])));
    } else if p[0] == 4u {
        let area = p[1] * p[2];
        if i >= area { return; }
        let x = i32(i % p[1]);
        let y = i32(i / p[1]);
        let known = xtrans_channel(x, y);
        let sample = xtrans_sample(x, y);
        for (var c = 0u; c < 3u; c++) {
            var value = sample;
            if c != known {
                var mean = xtrans_mean(x, y, c, 1);
                if mean.y == 0.0 { mean = xtrans_mean(x, y, c, 3); }
                if mean.y > 0.0 { value = mean.x / mean.y; }
            }
            dst[c * area + i] = bitcast<u32>(value);
        }
    } else if p[0] == 7u {
        // Sub-rectangle of a halo-free planar tile (whole-level split).
        if i >= p[1] { return; }
        let area = p[2]*p[3]; let c = i/area; let xy = i%area;
        dst[i] = src[c*p[6] + (xy/p[2]+p[8])*p[5] + xy%p[2]+p[7]];
    } else if p[0] == 3u {
        // Strip the immutable neighbour halo after Detail, before memoization.
        if i >= p[1] { return; }
        let area = p[2]*p[3]; let c = i/area; let xy = i%area;
        dst[i] = src[c*p[6] + (xy/p[2]+p[4])*p[5] + xy%p[2]+p[4]];
    } else {
        // One source tile contributes to each overlapping output block.
        // Dispatches are ordered; dst begins zeroed. No atomics or giant atlas.
        let ow=p[1]; let oh=p[2]; let sw=p[3]; let sh=p[4]; let scale=p[5];
        let rw=p[16];let rh=p[17];
        if i>=rw*rh*3u {return;}
        let c=i/(rw*rh);let x=p[14]+i%rw;let y=p[15]+(i/rw)%rh;
        let out_index=c*ow*oh+y*ow+x;
        let bx=p[6]+(p[8]+x)*scale;let by=p[7]+(p[9]+y)*scale;
        let ex=min(bx+scale,p[6]+p[10]);let ey=min(by+scale,p[7]+p[11]);
        let x0=max(bx,p[12]);let y0=max(by,p[13]);
        let x1=min(ex,p[12]+sw);let y1=min(ey,p[13]+sh);
        if x0>=x1 || y0>=y1 {return;}
        var sum=0.0;
        for (var sy=y0;sy<y1;sy++) {
            for (var sx=x0;sx<x1;sx++) {
                sum+=bitcast<f32>(src[c*sw*sh+(sy-p[13])*sw+sx-p[12]]);
            }
        }
        dst[out_index]=bitcast<u32>(bitcast<f32>(dst[out_index])+sum/f32((ex-bx)*(ey-by)));
    }
}
