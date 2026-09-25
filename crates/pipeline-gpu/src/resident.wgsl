// f16 cache storage uses packed pairs, so no optional shader-f16 capability is required.
@group(0) @binding(0) var<storage, read> src: array<u32>;
@group(0) @binding(1) var<storage, read_write> dst: array<u32>;
@group(0) @binding(2) var<storage, read> p: array<u32>;
@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) id: vec3<u32>) {
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
