fn adjustment_lut(offset: u32, v: f32) -> f32 {
    let x=clamp(v,0.0,1.0)*4095.0;
    let i=min(u32(x),4094u); let f=x-f32(i);
    return srcs[offset+i]+(srcs[offset+i+1u]-srcs[offset+i])*f;
}
fn adj_hue(p: f32,q: f32,t0: f32) -> f32 {
    let t=fract(t0);
    if(t<1.0/6.0) {return p+(q-p)*6.0*t;}
    if(t<0.5) {return q;}
    if(t<2.0/3.0) {return p+(q-p)*(2.0/3.0-t)*6.0;}
    return p;
}
fn adjustment(c: vec3<f32>, op: Op) -> vec3<f32> {
    let a=op.bi[0];
    switch(op.mode) {
    case 0u: {return vec3<f32>(1.0)-c;}
    case 1u: {return pow(max(c*a.x+vec3<f32>(a.y),vec3<f32>(0.0)),vec3<f32>(a.z));}
    case 2u: {return vec3<f32>(select(0.0,1.0,dot(c,vec3<f32>(0.299,0.587,0.114))>=a.x));}
    case 3u: {return min(floor(clamp(c,vec3<f32>(0.0),vec3<f32>(1.0))*a.x),vec3<f32>(a.x-1.0))/(a.x-1.0);}
    case 4u: {
        return vec3<f32>(adjustment_lut(12288u,adjustment_lut(0u,c.x)),adjustment_lut(12288u,adjustment_lut(4096u,c.y)),adjustment_lut(12288u,adjustment_lut(8192u,c.z)));
    }
    case 5u: {
        return vec3<f32>(dot(c,op.bi[0].xyz)+op.bi[0].w,dot(c,op.bi[1].xyz)+op.bi[1].w,dot(c,op.bi[2].xyz)+op.bi[2].w);
    }
    case 6u: {
        let mx=max(max(c.x,c.y),c.z); let mn=min(min(c.x,c.y),c.z);
        let d=mx-mn; var l=(mx+mn)*0.5; var h=0.0; var s=0.0;
        if(d!=0.0) {
            if(l>0.5) {s=d/(2.0-mx-mn);} else {s=d/(mx+mn);}
            if(mx==c.x) {h=(c.y-c.z)/d+select(0.0,6.0,c.y<c.z);}
            else if(mx==c.y) {h=(c.z-c.x)/d+2.0;} else {h=(c.x-c.y)/d+4.0;}
            h=h/6.0;
        }
        if(a.w!=0.0) {h=fract(a.x);s=max(a.y,0.0);l=dot(c,vec3<f32>(0.299,0.587,0.114));}
        else {h=fract(h+a.x);if(a.y<0.0){s=s*(1.0+a.y);}else{s=s+(1.0-s)*a.y;}}
        var rgb=vec3<f32>(l);
        if(s>0.0) {
            var q=l*(1.0+s); if(l>=0.5) {q=l+s-l*s;}
            let p=2.0*l-q;
            rgb=vec3<f32>(adj_hue(p,q,h+1.0/3.0),adj_hue(p,q,h),adj_hue(p,q,h-1.0/3.0));
        }
        if(a.z<0.0) {return rgb*(1.0+a.z);} return rgb+(vec3<f32>(1.0)-rgb)*a.z;
    }
    default: {return c;}
    }
}
