// Host payload is planar packed Bayer, never the resident cache's f16 format.
@group(0) @binding(0) var<storage, read> src: array<f32>;
@group(0) @binding(1) var<storage, read_write> dst: array<f32>;
@group(0) @binding(2) var<storage, read> p: array<u32>;
@group(0) @binding(3) var<storage, read> full: array<f32>;
fn fold(v:i32,n:u32)->u32 {
    if v>=0 && v<i32(n) {return u32(v);}
    let phase=u32((v%2+2)%2);
    if v<0 {return phase;}
    return phase+(n-1u-phase)/2u*2u;
}
@compute @workgroup_size(64)
fn unpack(@builtin(global_invocation_id) id:vec3<u32>) {
    let i=id.x+id.y*65535u*64u;
    let stride=p[0]; let n=stride*p[1]; if i>=n {return;}
    let x=fold(i32(i%stride)+i32(p[3])-i32(p[2]),p[5]);
    let y=fold(i32(i/stride)+i32(p[4])-i32(p[2]),p[6]);
    let w=(p[5]+1u)/2u*2u; let h=(p[6]+1u)/2u*2u;
    var rx=x; var ry=y;
    switch p[7] {case 1u:{rx=y;ry=w-1u-x;} case 2u:{rx=w-1u-x;ry=h-1u-y;} case 3u:{rx=h-1u-y;ry=x;} default:{}}
    let pn=p[10]*p[11]; let c=(ry%2u)*2u+rx%2u;
    let j=c*pn+(ry/2u-p[9])*p[10]+rx/2u-p[8];
    dst[i]=src[j];
    var mask=1.0; if p[12]!=0u {mask=src[4u*pn+j];}
    dst[n+i]=mask;
}
@compute @workgroup_size(64)
fn blend(@builtin(global_invocation_id) id:vec3<u32>) {
    let i=id.x+id.y*65535u*64u; let n=p[0]; if i>=n {return;}
    let a=bitcast<f32>(p[1])*full[n+i];
    if a==0.0 {dst[i]=src[i];}
    else if a==1.0 {dst[i]=full[i];}
    else {dst[i]=src[i]*(1.0-a)+full[i]*a;}
}
