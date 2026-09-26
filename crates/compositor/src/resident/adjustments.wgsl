// M5-26 pointwise operators. Operation order follows adjust's CPU reference.
// All non-power-of-two divisions use the correctly rounded pdiv helpers.
fn adj_hsl(c: vec3<f32>) -> vec3<f32> {
    let mx=max(max(c.x,c.y),c.z); let mn=min(min(c.x,c.y),c.z);
    let l=(mx+mn)*0.5;
    if (mx == mn) { return vec3<f32>(0.0,0.0,l); }
    let d=mx-mn; var s=pdiv(d,mx+mn);
    if (l>0.5) { s=pdiv(d,2.0-mx-mn); }
    var h=0.0;
    if (mx==c.x) {h=pdiv(c.y-c.z,d)+select(0.0,6.0,c.y<c.z);}
    else if (mx==c.y) {h=pdiv(c.z-c.x,d)+2.0;} else {h=pdiv(c.x-c.y,d)+4.0;}
    return vec3<f32>(pdiv(h,6.0),s,l);
}
fn adj_rgb(h: vec3<f32>) -> vec3<f32> {
    if (h.y<=0.0) {return vec3<f32>(h.z);}
    var q=h.z+h.y-h.z*h.y;
    if (h.z<0.5) {q=h.z*(1.0+h.y);}
    let p=2.0*h.z-q;
    return vec3<f32>(hsl_hue(p,q,h.x+1.0/3.0),hsl_hue(p,q,h.x),hsl_hue(p,q,h.x-1.0/3.0));
}
fn adj_preserve_luma(input: vec3<f32>, yy: f32) -> vec3<f32> {
    let y=clamp(yy,0.0,1.0); let old=y601(input);
    if (abs(old)<1e-8) {return vec3<f32>(y);}
    let c=pdiv3(input*y,vec3<f32>(old));
    var scale=1.0;
    for (var i=0u;i<3u;i++) {
        let v=c[i];
        if (v>1.0) {scale=min(scale,pdiv(1.0-y,v-y));}
        else if (v<0.0) {scale=min(scale,pdiv(y,y-v));}
    }
    return vec3<f32>(y)+(c-vec3<f32>(y))*scale;
}
fn adj_lut(off:u32,n:u32,v:f32)->f32 {
    if(n==0u){return v;} if(n==1u){return aux[off];}
    let x=clamp(v,0.0,1.0)*f32(n-1u); let i=min(u32(x),n-2u);let f=x-f32(i);
    return aux[off+i]+(aux[off+i+1u]-aux[off+i])*f;
}
fn adj_power(o:u32,v:f32,kind:u32)->f32 {
    if(v==0.0){return v;}
    var bits=bitcast<u32>(v);var shift=0i;
    if(bits>>23u==0u){bits=bitcast<u32>(v*16777216.0);shift=24i;}
    let e=i32(bits>>23u)-127i-shift;
    let m=bitcast<f32>((bits&0x7fffffu)|0x3f800000u);
    let x=(m-1.0)*4096.0;let i=u32(x);let f=x-f32(i);let off=o+kind*4374u;
    return (aux[off+i]+(aux[off+i+1u]-aux[off+i])*f)*aux[off+4097u+u32(e+149i)];
}
fn adj_root(o:u32,v:f32)->f32 {return select(1.0,-1.0,v<0.0)*adj_power(o,abs(v),2u);}
fn adj_decode(o:u32,v:f32)->f32 {
    if(v<=0.04045){return pdiv(v,12.92);} return adj_power(o,pdiv(v+0.055,1.055),0u);
}
fn adj_encode(o:u32,v:f32)->f32 {
    if(v<=0.0031308){return 12.92*v;} return 1.055*adj_power(o,v,1u)-0.055;
}
fn adj_encode3(o:u32,v:vec3<f32>)->vec3<f32> {return vec3<f32>(adj_encode(o,v.x),adj_encode(o,v.y),adj_encode(o,v.z));}
fn adj_decode3(o:u32,v:vec3<f32>)->vec3<f32> {return vec3<f32>(adj_decode(o,v.x),adj_decode(o,v.y),adj_decode(o,v.z));}
fn adj_oklab(o:u32,c:vec3<f32>)->vec3<f32> {
    let rgb=adj_decode3(o,c);let r=rgb.x;let g=rgb.y;let b=rgb.z;
    let l=adj_root(o,0.41222146*r+0.53633255*g+0.051445995*b);
    let m=adj_root(o,0.2119035*r+0.6806995*g+0.10739696*b);
    let s=adj_root(o,0.08830246*r+0.28171885*g+0.6299787*b);
    return vec3<f32>(0.21045426*l+0.7936178*m-0.004072047*s,1.9779985*l-2.4285922*m+0.4505937*s,0.025904037*l+0.78277177*m-0.80867577*s);
}
fn adj_cube(v:f32)->f32 {return v*v*v;}
fn adj_from_oklab(o:u32,c:vec3<f32>)->vec3<f32> {
    let l=c.x;let a=c.y;let b=c.z;
    let ll=adj_cube(l+0.39633778*a+0.21580376*b);
    let m=adj_cube(l-0.105561346*a-0.06385417*b);
    let s=adj_cube(l-0.08948418*a-1.2914855*b);
    return adj_encode3(o,vec3<f32>(4.0767417*ll-3.3077116*m+0.23096994*s,-1.268438*ll+2.6097574*m-0.3413194*s,-0.0041960863*ll-0.7034186*m+1.7076147*s));
}
fn adj_stop(off:u32)->vec3<f32> {return vec3<f32>(aux[off+1u],aux[off+2u],aux[off+3u]);}
fn adj_lab_f(o:u32,v:f32)->f32 {
    if(v>216.0/24389.0){return adj_root(o,v);}return pdiv((24389.0/27.0)*v+16.0,116.0);
}
fn adj_lab(o:u32,c:vec3<f32>)->vec3<f32> {
    let rgb=adj_decode3(o,c);let r=rgb.x;let g=rgb.y;let b=rgb.z;
    let x=adj_lab_f(o,pdiv(0.4124564*r+0.3575761*g+0.1804375*b,0.95047));
    let y=adj_lab_f(o,0.2126729*r+0.7151522*g+0.0721750*b);
    let z=adj_lab_f(o,pdiv(0.0193339*r+0.1191920*g+0.9503041*b,1.08883));
    return vec3<f32>(116.0*y-16.0,500.0*(x-y),200.0*(y-z));
}
fn adj_lab_inv(v:f32)->f32 {
    if(v>6.0/29.0){return adj_cube(v);}return pdiv(116.0*v-16.0,24389.0/27.0);
}
fn adj_from_lab(o:u32,c:vec3<f32>)->vec3<f32> {
    let yy=pdiv(c.x+16.0,116.0);let xx=yy+pdiv(c.y,500.0);let zz=yy-pdiv(c.z,200.0);
    let x=adj_lab_inv(xx)*0.95047;let y=adj_lab_inv(yy);let z=adj_lab_inv(zz)*1.08883;
    return adj_encode3(o,vec3<f32>(3.2404542*x-1.5371385*y-0.4985314*z,-0.969266*x+1.8760108*y+0.041556*z,0.0556434*x-0.2040259*y+1.0572252*z));
}
fn adj_membership(l:f32,tone:f32)->f32 {
    if(tone<=0.0){return 0.0;}let t=clamp(1.0-pdiv(l,tone),0.0,1.0);return t*t*(3.0-2.0*t);
}
fn extended_adjustment(k: u32, c: vec3<f32>, position: vec2<u32>) -> vec3<f32> {
    switch steps[k].t.z {
        case 7u: { return vec3<f32>((max(max(c.x, c.y), c.z) + min(min(c.x, c.y), c.z)) * 0.5); }
        case 8u: {
            let a = steps[k].p[0];
            if (a.z != 0.0) { return clamp((c - vec3<f32>(0.5)) * (1.0+a.y) + vec3<f32>(0.5) + vec3<f32>(a.x), vec3<f32>(0.0),vec3<f32>(1.0)); }
            let v = clamp(c,vec3<f32>(0.0),vec3<f32>(1.0));
            let t = v + a.x*v*(vec3<f32>(1.0)-v);
            let x = pow(t,vec3<f32>(a.w));
            return pdiv3(x,x+pow(vec3<f32>(1.0)-t,vec3<f32>(a.w)));
        }
        case 9u: {
            let a=steps[k].p[0];
            let o=c*(vec3<f32>(1.0-a.w)+a.w*a.xyz);
            if (steps[k].p[1].x != 0.0) {return adj_preserve_luma(o,y601(c));}
            return o;
        }
        case 10u: {
            let y=clamp(y601(c),0.0,1.0);
            let sy=1.0-y;
            let o=c+pdiv3(sy*sy*steps[k].p[0].xyz+2.0*y*(1.0-y)*steps[k].p[1].xyz+y*y*steps[k].p[2].xyz,vec3<f32>(100.0));
            if (steps[k].p[0].w != 0.0) {return adj_preserve_luma(o,y);}
            return clamp(o,vec3<f32>(0.0),vec3<f32>(1.0));
        }
        case 11u: {
            let h=adj_hsl(c).x*6.0;
            let i=u32(floor(h))%6u; let f=h-trunc(h);
            let mx=max(max(c.x,c.y),c.z); let mn=min(min(c.x,c.y),c.z);
            let off=steps[k].t.w;
            let y=clamp(mn+pdiv((mx-mn)*(aux[off+i]*(1.0-f)+aux[off+(i+1u)%6u]*f),100.0),0.0,1.0);
            if (steps[k].p[0].w != 0.0) {let hs=adj_hsl(steps[k].p[0].xyz);return adj_rgb(vec3<f32>(hs.xy,y));}
            return vec3<f32>(y);
        }
        case 12u: {
            let mx=max(max(c.x,c.y),c.z); let mn=min(min(c.x,c.y),c.z);
            let chroma=clamp(mx-mn,0.0,1.0); let h=adj_hsl(c).x*6.0;
            let y=clamp(y601(c),0.0,1.0);
            var weights: array<f32,9>;
            for(var i=0u;i<6u;i++) {let d=abs(h-f32(i));weights[i]=chroma*max(1.0-min(d,6.0-d),0.0);}
            weights[6]=(1.0-chroma)*max(2.0*y-1.0,0.0);
            weights[8]=(1.0-chroma)*max(1.0-2.0*y,0.0);
            weights[7]=(1.0-chroma)*(1.0-abs(2.0*y-1.0));
            var out=c; let absolute=steps[k].p[0].x!=0.0; let off=steps[k].t.w;
            for(var i=0u;i<3u;i++) {
                var v=c[i];
                for(var j=0u;j<9u;j++) {
                    let correction=aux[off+4u*j+i];let key=aux[off+4u*j+3u];
                    v=v-weights[j]*(correction*select(1.0-c[i],1.0,absolute)+key*select(1.0-mx,1.0,absolute));
                }
                out[i]=clamp(v,0.0,1.0);
            }
            return out;
        }
        case 13u: {
            let a=steps[k].p[0];let b=steps[k].p[1];let d=c-a.xyz;
            let distance=pdiv(sqrt(d.x*d.x+d.y*d.y+d.z*d.z),b.w);
            var weight=select(0.0,1.0,distance<=1e-7);
            if (a.w>0.0) {weight=clamp(1.0-pdiv(distance,a.w),0.0,1.0);}
            var hs=adj_hsl(c);let t=hs.x+b.x; hs.x=t-floor(t);
            if(b.y<0.0) {hs.y=hs.y*(1.0+b.y);} else {hs.y=hs.y+(1.0-hs.y)*b.y;}
            var o=adj_rgb(hs);
            if(b.z<0.0) {o=o*(1.0+b.z);} else {o=o+(vec3<f32>(1.0)-o)*b.z;}
            return c+weight*(o-c);
        }
        case 15u: {
            let a=steps[k].p[0];if(a.x==0.0 && a.y==0.0){return c;}
            let hs=adj_hsl(c);let d=abs(hs.x-1.0/12.0);let dist=min(d,1.0-d);
            let skin=max(1.0-pdiv(dist,1.0/12.0),0.0);
            let scale=(1.0+a.y)*(1.0+a.x*(1.0-hs.y)*(1.0-0.75*skin));
            let o=steps[k].t.w;var lab=adj_oklab(o,c);lab.y=lab.y*scale;lab.z=lab.z*scale;
            return clamp(adj_from_oklab(o,lab),vec3<f32>(0.0),vec3<f32>(1.0));
        }
        case 16u: {
            let a=steps[k].p[0];let off=steps[k].t.w;let n=steps[k].u.x;let o=off+4u*n;
            var t=clamp(y601(c),0.0,1.0);if(a.y!=0.0){t=1.0-t;}
            if(a.x!=0.0){
                var h=position.x ^ (position.y*0x9e3779b9u);
                h=(h^(h>>16u))*0x7feb352du;h=(h^(h>>15u))*0x846ca68bu;h=h^(h>>16u);
                t=clamp(t+pdiv(pdiv(f32(h&65535u),65535.0)-0.5,255.0),0.0,1.0);
            }
            if(n==0u){return vec3<f32>(t);}if(t<=aux[off]){return adj_stop(off);}
            var i=1u; loop {if(i>=n){return adj_stop(off+4u*(n-1u));}if(aux[off+4u*i]>t){break;}i=i+1u;}
            let prev=off+4u*(i-1u);let next=off+4u*i;
            let f=clamp(pdiv(t-aux[prev],aux[next]-aux[prev]),0.0,1.0);
            var ca=adj_stop(prev);var cb=adj_stop(next);
            if(a.z==1.0){ca=adj_decode3(o,ca);cb=adj_decode3(o,cb);}
            if(a.z==2.0){ca=adj_oklab(o,ca);cb=adj_oklab(o,cb);}
            var out=ca+f*(cb-ca);
            if(a.z==1.0){out=adj_encode3(o,out);}if(a.z==2.0){out=adj_from_oklab(o,out);}
            return clamp(out,vec3<f32>(0.0),vec3<f32>(1.0));
        }
        case 17u: {
            var out=c;
            for(var i=0u;i<3u;i++) {
                let a=steps[k].p[i];let span=a.y-a.x;var t=select(0.0,1.0,c[i]>=a.y);
                if(abs(span)>=1e-9){t=clamp(pdiv(c[i]-a.x,span),0.0,1.0);}
                if(a.z!=1.0){t=lut(steps[k].t.w+4096u*i,t);}out[i]=t;
            }
            return out;
        }
        case 18u: {
            let a=steps[k].p[0];let f=a.z;if(f==1.0){return c;}
            let off=steps[k].t.w;let o=off+12u;let lab=adj_lab(o,c);var mapped=lab;
            for(var i=0u;i<3u;i++) {mapped[i]=pdiv((lab[i]-aux[off+6u+i])*max(aux[off+3u+i],0.0),max(aux[off+9u+i],1e-6))+aux[off+i];}
            mapped.x=mapped.x*a.x;mapped.y=mapped.y*a.y;mapped.z=mapped.z*a.y;
            let out=clamp(adj_from_lab(o,mapped),vec3<f32>(0.0),vec3<f32>(1.0));
            return out*(1.0-f)+c*f;
        }
        case 19u: {
            let n=steps[k].u.x;let off=steps[k].t.w;
            let x=clamp(c,vec3<f32>(0.0),vec3<f32>(1.0))*f32(n-1u);
            let lo=min(vec3<u32>(x),vec3<u32>(n-2u));let f=x-vec3<f32>(lo);var out=vec3<f32>(0.0);
            for(var b=0u;b<2u;b++){for(var g=0u;g<2u;g++){for(var r=0u;r<2u;r++){
                let w=select(1.0-f.x,f.x,r!=0u)*select(1.0-f.y,f.y,g!=0u)*select(1.0-f.z,f.z,b!=0u);
                let p=off+3u*(lo.x+r+n*(lo.y+g+n*(lo.z+b)));
                out=out+w*vec3<f32>(aux[p],aux[p+1u],aux[p+2u]);
            }}}
            return out;
        }
        case 20u: {
            if(steps[k].p[2].x!=0.0){return c;}
            let a=steps[k].p[0];let b=steps[k].p[1];
            let l=0.2126*c.x+0.7152*c.y+0.0722*c.z;
            let shadow=adj_membership(l,a.y);let highlight=adj_membership(1.0-l,a.w);
            var mapped=l+0.5*a.x*shadow*max(1.0-l,0.0)-0.5*a.z*highlight*max(l,0.0);
            let t=clamp(mapped,0.0,1.0);mapped=mapped+b.y*(t-0.5)*4.0*t*(1.0-t);
            let span=1.0-b.z-b.w;
            return clamp(pdiv3(vec3<f32>(mapped)+(c-vec3<f32>(l))*(1.0+b.x)-vec3<f32>(b.z),vec3<f32>(span)),vec3<f32>(0.0),vec3<f32>(1.0));
        }
        case 14u: {
            let n=vec3<u32>(steps[k].p[0].xyz);let o=steps[k].t.w;
            return vec3<f32>(adj_lut(o,n.x,c.x),adj_lut(o+n.x,n.y,c.y),adj_lut(o+n.x+n.y,n.z,c.z));
        }
        default: { return c; }
    }
}
