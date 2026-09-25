// Local tone only. All neighbourhood filtering is GPU compute, with clipped,
// renormalized separable box means matching the CPU's accumulation order.
@group(0) @binding(0) var<storage, read> a: array<vec4<f32>>;
@group(0) @binding(1) var<storage, read> b: array<vec4<f32>>;
@group(0) @binding(2) var<storage, read> c: array<vec4<f32>>;
@group(0) @binding(3) var<storage, read> d: array<vec4<f32>>;
@group(0) @binding(4) var<storage, read> e: array<vec4<f32>>;
@group(0) @binding(5) var<storage, read_write> out: array<vec4<f32>>;
@group(0) @binding(6) var<storage, read> p: array<f32>;
fn finite(v:f32)->f32 { return clamp(v,-3.402823466e38,3.402823466e38); }
fn luma(v:vec3<f32>)->f32 { return finite(0.2627*v.x + 0.678*v.y + 0.0593*v.z); }
fn encode(v:f32)->f32 {
    if v > 6.125082e37 { return (log(v)-log(0.18))/log(1.0+1.0/0.18); }
    return log(1.0+v/0.18)/log(1.0+1.0/0.18);
}
fn decode(v:f32)->f32 {
    let x=v*log(1.0+1.0/0.18);
    if x>=80.0 { return finite(exp(x+log(0.18))); }
    return 0.18*(exp(x)-1.0);
}
// Maximum radius is eight. Each 16x16 output tile cooperatively loads a
// 32x16 or 16x32 strip (512 vec4s / 8 KiB). Smaller radii use a subset.
// Never prefix/subtract sums: retain CPU order.
var<workgroup> strip: array<vec4<f32>,512>;
@compute @workgroup_size(16,16)
fn mean(@builtin(global_invocation_id) id:vec3<u32>,
        @builtin(local_invocation_index) lane:u32,
        @builtin(workgroup_id) group:vec3<u32>) {
    let w=u32(p[0]); let h=u32(p[1]); let r=u32(p[3]);
    let horizontal=u32(p[2])==2u;
    // Fixed power-of-two pitches avoid integer division by a runtime radius.
    let sw=select(16u,32u,horizontal);
    let origin=vec2<i32>(group.xy*16u)-select(vec2<i32>(0,i32(r)),vec2<i32>(i32(r),0),horizontal);
    for(var k=lane;k<512u;k+=256u) {
        let local=select(vec2<u32>(k%16u,k/16u),vec2<u32>(k%32u,k/32u),horizontal);
        let xy=origin+vec2<i32>(local);
        var value=vec4<f32>(0.0);
        if xy.x>=0 && xy.y>=0 && xy.x<i32(w) && xy.y<i32(h) {
            value=a[u32(xy.y)*w+u32(xy.x)];
        }
        strip[k]=value;
    }
    // Partial edge workgroups must participate before returning.
    workgroupBarrier();
    if id.x>=w || id.y>=h { return; }
    let pos=select(i32(id.y),i32(id.x),horizontal);
    let len=select(i32(h),i32(w),horizontal);
    let lo=max(0,pos-i32(r)); let hi=min(len,pos+i32(r)+1);
    var sum=vec4<f32>(0.0);
    for(var k=lo;k<hi;k++) {
        let xy=select(vec2<i32>(i32(id.x),k),vec2<i32>(k,i32(id.y)),horizontal)-origin;
        sum+=strip[u32(xy.y)*sw+u32(xy.x)];
    }
    out[id.y*w+id.x]=sum/f32(hi-lo);
}
@compute @workgroup_size(8,8)
fn main(@builtin(global_invocation_id) id:vec3<u32>) {
    let w=u32(p[0]); let h=u32(p[1]);
    if id.x>=w || id.y>=h { return; }
    let i=id.y*w+id.x; let mode=u32(p[2]); let r=i32(p[3]);
    let x=i32(id.x); let y=i32(id.y);
    if mode==0u { // RGB -> log luminance
        out[i]=vec4<f32>(encode(max(luma(a[i].xyz),0.0)),0.0,0.0,0.0);
    } else if mode==1u { // guided-filter moments
        let g=a[i].x; let v=b[i].x;
        out[i]=vec4<f32>(g,v,g*g,g*v);
    } else if mode==2u || mode==3u { // horizontal / vertical means
        var sum=vec4<f32>(0.0);
        let pos=select(y,x,mode==2u); let len=select(i32(h),i32(w),mode==2u);
        let lo=max(0,pos-r); let hi=min(len,pos+r+1);
        for(var k=lo;k<hi;k++) {
            let j=select(u32(k)*w+id.x,id.y*w+u32(k),mode==2u);
            sum+=a[j];
        }
        out[i]=sum/f32(hi-lo);
    } else if mode==4u { // linear coefficients, covariance remains signed
        let m=a[i]; let aa=(m.w-m.x*m.y)/(max(m.z-m.x*m.x,0.0)+0.001);
        out[i]=vec4<f32>(aa,m.y-aa*m.x,0.0,0.0);
    } else if mode==5u {
        out[i]=vec4<f32>(a[i].x*b[i].x+a[i].y,0.0,0.0,0.0);
    } else if mode==6u { // presence with no-new-extrema protection
        let rgb=a[i].xyz; let lum=luma(rgb); let z=b[i].x;
        var lo=z; var hi=z;
        for(var yy=max(0,y-1);yy<min(i32(h),y+2);yy++) {
            for(var xx=max(0,x-1);xx<min(i32(w),x+2);xx++) {
                let v=b[u32(yy)*w+u32(xx)].x; lo=min(lo,v); hi=max(hi,v);
            }
        }
        let t=clamp(z,0.0,1.0); let weight=4.0*t*(1.0-t);
        let delta=p[4]*(c[i].x-d[i].x)+p[5]*weight*(d[i].x-e[i].x);
        let adjusted=clamp(z+delta,lo,hi);
        var result=rgb;
        if lum>0.0 && adjusted!=z {
            let gain=finite(decode(adjusted)/lum);
            result=vec3<f32>(finite(rgb.x*gain),finite(rgb.y*gain),finite(rgb.z*gain));
        }
        out[i]=vec4<f32>(result,0.0);
    } else if mode==7u || mode==8u { // dark channel (raw or air-normalized)
        var dark=3.402823466e38;
        for(var yy=max(0,y-3);yy<min(i32(h),y+4);yy++) {
            for(var xx=max(0,x-3);xx<min(i32(w),x+4);xx++) {
                var rgb=a[u32(yy)*w+u32(xx)].xyz;
                if mode==8u { rgb=rgb/vec3<f32>(p[6],p[7],p[8]); }
                dark=min(dark,max(min(rgb.x,min(rgb.y,rgb.z)),0.0));
            }
        }
        if mode==7u { out[i]=vec4<f32>(max(luma(a[i].xyz),0.0),dark,0.0,0.0); }
        else { out[i]=vec4<f32>(clamp(1.0-0.85*p[9]*clamp(dark,0.0,1.0),0.15,1.0),0.0,0.0,0.0); }
    } else if mode==9u {
        let t=clamp(b[i].x,0.15,1.0); var rgb=a[i].xyz;
        for(var ch=0u;ch<3u;ch++) {
            if rgb[ch]>=0.0 {
                if p[10]>0.0 { rgb[ch]=finite(max(p[6u+ch]+(rgb[ch]-p[6u+ch])/t,0.0)); }
                else { rgb[ch]=finite(rgb[ch]*t+p[6u+ch]*(1.0-t)); }
            }
        }
        out[i]=vec4<f32>(rgb,0.0);
    }
}
