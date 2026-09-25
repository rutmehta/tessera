@group(0) @binding(0) var<storage, read> src: array<u32>;
@group(0) @binding(1) var<storage, read_write> dst: array<u32>;
@group(0) @binding(2) var<storage, read> p: array<f32>;
const MAX: f32 = 3.4028234663852886e38;
fn divide(a:f32,b:f32)->f32 {let q=a/b;return fma(fma(-q,b,a),1./b,q);}
// Compensated arithmetic makes pow round like scalar libm even where signed
// Lab values amplify a one-ulp radius error into a visible absolute difference.
fn dsadd(a:vec2<f32>,b:vec2<f32>)->vec2<f32>{
 let s=fma(a.x,1.,b.x);let v=fma(s,1.,-a.x);
 let e=fma(fma(a.x,1.,-fma(s,1.,-v)),1.,fma(b.x,1.,-v));
 let lo=fma(e,1.,fma(a.y,1.,b.y));
 let hi=fma(s,1.,lo);return vec2(hi,fma(lo,1.,-fma(hi,1.,-s)));
}
fn dsmul(a:vec2<f32>,b:vec2<f32>)->vec2<f32>{
 let hi=fma(a.x,b.x,0.);let lo=fma(fma(a.x,b.x,-hi),1.,fma(a.x,b.y,fma(a.y,b.x,0.)));
 let s=fma(hi,1.,lo);return vec2(s,fma(lo,1.,-fma(s,1.,-hi)));
}
fn dsdiv(a:vec2<f32>,b:vec2<f32>)->vec2<f32>{
 let q=a.x/b.x;let rem=dsadd(a,-dsmul(vec2(q,0.),b));
 return dsadd(vec2(q,0.),vec2((rem.x+rem.y)/b.x,0.));
}
fn accurate_pow(x:f32,y:f32)->f32 {
 if x==0. {return 0.;}
 let bits=bitcast<u32>(x);let e=i32((bits>>23u)&255u)-127;
 let m=bitcast<f32>((bits&0x7fffffu)|0x3f800000u);
 let z=dsdiv(vec2(m-1.,0.),dsadd(vec2(m,0.),vec2(1.,0.)));
 let z2=dsmul(z,z);var power=z;var sum=z;
 for(var k=3u;k<=29u;k+=2u){power=dsmul(power,z2);sum=dsadd(sum,dsdiv(power,vec2(f32(k),0.)));}
 let ln2=vec2(0.6931471824645996,-1.9046542121259336e-9);
 let ln=dsadd(dsmul(sum,vec2(2.,0.)),dsmul(ln2,vec2(f32(e),0.)));
 let exponent=dsmul(ln,vec2(y,0.));
 let n=round(exponent.x*1.4426950408889634);
 if n>127. {return pow(x,y);}if n < -125. {return pow(x,y);}
 let r=dsadd(exponent,-dsmul(ln2,vec2(n,0.)));
 var term=vec2(1.,0.);var result=term;
 for(var k=1u;k<=14u;k++){term=dsdiv(dsmul(term,r),vec2(f32(k),0.));result=dsadd(result,term);}
 return result.x*exp2(n);
}
fn finite(x: f32) -> f32 { return clamp(x,-MAX,MAX); }
fn smooth_value(x:f32)->f32 {let t=clamp(x,0.,1.);return fma(t,t,0.)*(3.-2.*t);}
// Little-endian u64 pairs. Four 16-bit partial products preserve every carry.
fn mul64(a:vec2<u32>,b:vec2<u32>)->vec2<u32> {
 let a0=a.x&65535u; let a1=a.x>>16u;
 let b0=b.x&65535u; let b1=b.x>>16u;
 let w0=a0*b0;
 let t=a1*b0+(w0>>16u);
 let w1=(t&65535u)+a0*b1;
 let hi=a1*b1+(t>>16u)+(w1>>16u)+a.x*b.y+a.y*b.x;
 return vec2((w1<<16u)|(w0&65535u),hi);
}
fn shr(a:vec2<u32>,n:u32)->vec2<u32>{return vec2((a.x>>n)|(a.y<<(32u-n)),a.y>>n);}
fn plus1(a:vec2<u32>)->vec2<u32>{let lo=a.x+1u;return vec2(lo,a.y+select(0u,1u,lo==0u));}
// Decode integral f32 directly, including negative and >i32 coordinates, with
// Rust's saturating f32->i64 semantics. No lossy signed 32-bit conversion.
fn integer64(x:f32)->vec2<u32>{
 let bits=bitcast<u32>(x); let exp=i32((bits>>23u)&255u)-127;
 if exp<0 {return vec2(0u);}
 let neg=(bits>>31u)!=0u;
 if exp>=63 {return select(vec2(0xffffffffu,0x7fffffffu),vec2(0u,0x80000000u),neg);}
 let m=(bits&0x7fffffu)|0x800000u;
 var v=vec2(0u);
 if exp<=23 {v.x=m>>u32(23-exp);} else {
  let shift=u32(exp-23);
  if shift<32u {v=vec2(m<<shift,m>>(32u-shift));} else {v=vec2(0u,m<<(shift-32u));}
 }
 if neg {v=plus1(~v);}
 return v;
}
fn hash64(x:vec2<u32>,y:vec2<u32>)->vec2<u32> {
 var z=mul64(x,vec2(0x7f4a7c15u,0x9e3779b9u))^mul64(y,vec2(0x1ce4e5b9u,0xbf58476du))^vec2(0x45524132u,0x54455353u);
 z=mul64(z^shr(z,30u),vec2(0x1ce4e5b9u,0xbf58476du));
 z=mul64(z^shr(z,27u),vec2(0x133111ebu,0x94d049bbu));
 z=z^shr(z,31u);
 return z;
}
fn hash(x:vec2<u32>,y:vec2<u32>)->f32 {
 let q=shr(hash64(x,y),11u);
 return (f32(q.y)*4294967296.+f32(q.x))/9007199254740992.*2.-1.;
}
fn noise(x:f32,y:f32)->f32 {
 let ix=integer64(floor(x));let iy=integer64(floor(y));
 let tx=smooth_value(x-floor(x));let ty=smooth_value(y-floor(y));
 let a=hash(ix,iy)*(1.-tx)+hash(plus1(ix),iy)*tx;
 let b=hash(ix,plus1(iy))*(1.-tx)+hash(plus1(ix),plus1(iy))*tx;
 return a*(1.-ty)+b*ty;
}
fn fd(rgb:vec3<f32>,w:vec3<f32>)->f32 {
 let scale=select(1.,16.,any(abs(rgb)>vec3(MAX/16.)));
 let v=rgb/scale;
 return finite(fma(fma(fma(v.x,w.x,0.),1.,fma(v.y,w.y,0.)),1.,fma(v.z,w.z,0.))*scale);
}
fn labf(t:f32)->f32 {if t>216./24389. {let root=pow(t,1./3.);return root+(t/root/root-root)/3.;}return finite(fma(t,841./108.,0.)+4./29.);}
fn labi(t:f32)->f32 {if t>6./29. {return finite(fma(t,t,0.)*t);}return (t-4./29.)*(108./841.);}
fn perceptual(rgb:vec3<f32>,a:f32)->vec3<f32>{
 var xyz=vec3(fd(rgb,vec3(0.63695806,0.1446169,0.16888097)),fd(rgb,vec3(0.2627002,0.67799807,0.059301715)),fd(rgb,vec3(0.,0.028072692,1.0609851)));
 let white=vec3(0.9504559,1.,1.0890578);
 let fy=labf(xyz.y);
 var delta=fma(a,1.-fy,0.);if a<0. {delta=fma(a,fy-16./116.,0.);}
 for(var i=0u;i<3u;i++){let t=finite(divide(xyz[i],white[i]));xyz[i]=finite(labi(finite(labf(t)+delta))*white[i]);}
 return vec3(fd(xyz,vec3(1.7166512,-0.35567078,-0.2533663)),fd(xyz,vec3(-0.6666843,1.6164812,0.015768547)),fd(xyz,vec3(0.017639857,-0.042770613,0.94210315)));
}
// Clamp integer global coordinates before converting to f32, including halos.
fn coordinate(origin:u32,local:u32,halo:u32,extent:u32)->f32 {
 if local<halo {return f32(origin-min(origin,halo-local));}
 let d=local-halo;
 return f32(origin+min(d,extent-1u-origin));
}
fn effects_pixel(input:vec3<f32>,i:u32)->vec3<f32>{
 if p[26]!=0. {return input;}
 let stride=bitcast<u32>(p[1]);let halo=bitcast<u32>(p[2]);
 let gx=coordinate(bitcast<u32>(p[3]),i%stride,halo,bitcast<u32>(p[5]));
 let gy=coordinate(bitcast<u32>(p[4]),i/stride,halo,bitcast<u32>(p[6]));
 let dx=gx+0.5-p[9];let dy=gy+0.5-p[10];
 let u=divide(fma(p[12],dx,0.)-fma(p[11],dy,0.),p[7])+0.5;
 let v=divide(fma(p[11],dx,0.)+fma(p[12],dy,0.),p[8])+0.5;
 let radius=accurate_pow(accurate_pow(abs(2.*u-1.),p[14])+accurate_pow(abs(2.*v-1.),p[14]),divide(1.,p[14]));
 var mask=select(0.,1.,radius>=p[15]);
 if p[16]!=0. {mask=smooth_value(divide(radius-p[15],p[16]*(1.5-p[15])));}
 var rgb=input;
 let y=fma(0.2627,rgb.x,0.)+fma(0.6780,rgb.y,0.)+fma(0.0593,rgb.z,0.);
 var protect=1.;if p[13]<0. {protect=1.-p[17]*smooth_value(y);}
 let a=fma(p[13],mask,0.)*protect;
 if a!=0. {
  if p[25]==0. {rgb=rgb*pow(2.,2.*a);} else if p[25]==1. {rgb=perceptual(rgb,a);} else {rgb=rgb*(1.-abs(a))+vec3(select(0.,1.,a>0.)*abs(a));}
 }
 if p[18]>0. {
  let px=u*p[21]*p[23]/p[19];let py=v*p[22]*p[24]/p[19];
  let value=(noise(px,py)+p[20]*0.5*noise(2.*px+19.,2.*py+7.))/(1.+p[20]*0.5);
  // Match CPU operation order: multiply by amount before division by 100.
  let delta=value*(0.025+0.075*p[20])*p[18];
  rgb=rgb+vec3(delta);
 }
 return vec3(finite(rgb.x),finite(rgb.y),finite(rgb.z));
}
@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) id:vec3<u32>){
 let i=id.x;let n=bitcast<u32>(p[0]);if i>=n{return;}
 let rgb=effects_pixel(vec3(bitcast<f32>(src[i]),bitcast<f32>(src[n+i]),bitcast<f32>(src[2u*n+i])),i);
 for(var c=0u;c<3u;c++){dst[c*n+i]=bitcast<u32>(rgb[c]);}
}
