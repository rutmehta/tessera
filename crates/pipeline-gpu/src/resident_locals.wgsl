@group(0) @binding(0) var<storage,read> base: array<f32>;
@group(0) @binding(1) var<storage,read> adjusted: array<f32>;
@group(0) @binding(2) var<storage,read_write> output: array<f32>;
@group(0) @binding(3) var<storage,read> p: array<f32>;
// Keep dispatch and planar offsets integer: 24MP exceeds exact f32 indices.
fn pixel_index(gid: vec3<u32>) -> u32 { return gid.x + gid.y * 65535u * 256u; }
fn mask_smooth(t0:f32)->f32 { let t=clamp(t0,0.0,1.0); return t*t*(3.0-2.0*t); }
fn falloff(d:f32,f:f32)->f32 { if f==0.0 { return select(0.0,1.0,d<=1.0); } return mask_smooth((1.0-d)/f); }
fn to_lab(rgb: vec3<f32>) -> vec3<f32> {
    let lms = vec3<f32>(
        0.6167558 * rgb.x + 0.3601984 * rgb.y + 0.0230458 * rgb.z,
        0.265133 * rgb.x + 0.6358394 * rgb.y + 0.0990276 * rgb.z,
        0.1001026 * rgb.x + 0.2039065 * rgb.y + 0.6959909 * rgb.z);
    let v = sign(lms) * pow(abs(lms), vec3<f32>(1.0 / 3.0));
    return vec3<f32>(
        0.21045426 * v.x + 0.7936178 * v.y - 0.004072047 * v.z,
        1.9779985 * v.x - 2.4285922 * v.y + 0.4505937 * v.z,
        0.025904037 * v.x + 0.78277177 * v.y - 0.80867577 * v.z);
}
fn from_lab(lab: vec3<f32>) -> vec3<f32> {
    let v = vec3<f32>(lab.x + 0.39633778 * lab.y + 0.21580376 * lab.z,
        lab.x - 0.105561346 * lab.y - 0.06385417 * lab.z,
        lab.x - 0.08948418 * lab.y - 1.2914855 * lab.z);
    let q = v * v * v;
    return vec3<f32>(2.1399066 * q.x - 1.2463895 * q.y + 0.1064829 * q.z,
        -0.8847359 * q.x + 2.163231 * q.y - 0.2784951 * q.z,
        -0.0485738 * q.x - 0.4545031 * q.y + 1.5030769 * q.z);
}
@compute @workgroup_size(256)
fn transform(@builtin(global_invocation_id) gid: vec3<u32>) {
 let i=pixel_index(gid); let n=u32(p[0])*u32(p[1]); if i>=n { return; }
 if p[2]==1.0 { for(var c=0u;c<3u;c++) { let j=i+c*n; output[j]=2.0*base[j]-adjusted[j]; } return; }
 let rgb=vec3<f32>(base[i],base[i+n],base[i+2u*n]);
 let lab=to_lab(rgb);
 let v=from_lab(vec3<f32>(lab.x,lab.y*p[4]-lab.z*p[3],lab.y*p[3]+lab.z*p[4]));
 output[i]=v.x; output[i+n]=v.y; output[i+2u*n]=v.z;
}
@compute @workgroup_size(256)
fn blend(@builtin(global_invocation_id) gid: vec3<u32>) {
 let i = pixel_index(gid);
 let w = u32(p[0]); let h = u32(p[1]); let n = w*h; if i >= n { return; }
 let x = (f32(i%w)+0.5)/p[0]; let y = (f32(i/w)+0.5)/p[1];
 var a=0.0; var cursor=4u;
 for (var component=0u;component<u32(p[2]);component++) {
   let k=cursor+4u; var b=0.0;
   switch u32(p[cursor]) {
     case 0u: { let dx=p[k+2u]-p[k]; let dy=p[k+3u]-p[k+1u]; b=clamp(1.0-((x-p[k])*dx+(y-p[k+1u])*dy)/(dx*dx+dy*dy),0.0,1.0); }
     case 1u: { let xx=x-p[k]; let yy=y-p[k+1u]; let s=p[k+4u]; let c=p[k+5u]; b=falloff(length(vec2<f32>((c*xx+s*yy)/p[k+2u],(-s*xx+c*yy)/p[k+3u])),p[k+6u]); }
     case 2u: { let l=0.2627*base[i]+0.6780*base[i+n]+0.0593*base[i+2u*n]; let d=max(max(p[k]-l,l-p[k+1u]),0.0); if d==0.0 { b=1.0; } else if p[k+2u]!=0.0 { b=mask_smooth(1.0-d/p[k+2u]); } }
     case 3u: { let lab=to_lab(vec3<f32>(base[i],base[i+n],base[i+2u*n])); var d=3.402823e38; for(var j=0u;j<u32(p[k+1u]);j++) { let off=k+2u+j*3u; d=min(d,length(lab-vec3<f32>(p[off],p[off+1u],p[off+2u]))); } if p[k]==0.0 { b=select(0.0,1.0,d<=1e-6); } else { b=falloff(d/p[k],0.5); } }
     case 4u: { for(var j=0u;j<u32(p[k]);j++) { let off=k+1u+j*7u; let px=f32(i%w)+0.5; let py=f32(i/w)+0.5; let r=p[off+3u]; let d=length(vec2<f32>(px-p[off],py-p[off+1u]))/r;
       // Match CPU stamp bounding rectangles, including feather-zero edges.
       if f32(i%w)>=floor(p[off]-r) && f32(i%w)<ceil(p[off]+r) && f32(i/w)>=floor(p[off+1u]-r) && f32(i/w)<ceil(p[off+1u]+r) {
         let alpha=falloff(d,p[off+4u])*p[off+2u]*p[off+5u]; if p[off+6u]!=0.0 { b*=1.0-alpha; } else { b+=(1.0-b)*alpha; }
       }
     } }
     default: {}
   }
   if p[cursor+2u]!=0.0 { b=1.0-b; }
   if component==0u { a=b; } else { switch u32(p[cursor+1u]) { case 0u:{ a=max(a,b); } case 1u:{ a*=1.0-b; } default:{ a*=b; } } }
   cursor+=u32(p[cursor+3u]);
 }
 if p[3] != 0.0 { a = 1.0-a; }
 if a == 0.0 { return; }
 for (var c=0u;c<3u;c++) { let j=i+c*n; var b=adjusted[j]; if a != 1.0 { b=base[j]+(b-base[j])*a; } output[j] += b-base[j]; }
}
