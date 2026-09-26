@group(0) @binding(0) var<storage,read> original: array<vec4<f32>>;
@group(0) @binding(1) var<storage,read> src: array<vec4<f32>>;
@group(0) @binding(2) var<storage,read_write> dst: array<vec4<f32>>;
@group(0) @binding(3) var<storage,read> p: array<f32>;
fn at(x:i32,y:i32)->vec4<f32> { return src[u32(clamp(y,0,i32(p[1])-1))*u32(p[0])+u32(clamp(x,0,i32(p[0])-1))]; }
fn sized_at(x:i32,y:i32,w:i32,h:i32)->vec4<f32> {return src[u32(clamp(y,0,h-1)*w+clamp(x,0,w-1))];}
fn modulo(x:f32,n:f32)->f32 { return x-floor(x/n)*n; }
fn lanczos(v:f32)->f32 {
 let x=abs(v); if x==0.0 {return 1.0;} if x>=3.0 || fract(x)==0.0 {return 0.0;}
 let px=3.141592653589793*x; return (sin(px)/px)*(sin(px/3.0)/(px/3.0));
}
fn tap(x:i32,y:i32,wrap:bool)->vec4<f32> {
 if !wrap {return at(x,y);}
 return src[u32(modulo(f32(y),p[1]))*u32(p[0])+u32(modulo(f32(x),p[0]))];
}
fn sample(pos:vec2<f32>,wrap:bool)->vec4<f32> {
 var q=clamp(pos,vec2(0.0),vec2(p[0]-1.0,p[1]-1.0));
 if wrap {q=vec2(modulo(pos.x,p[0]),modulo(pos.y,p[1]));}
 if all(fract(q)==vec2(0.0)) {return tap(i32(q.x),i32(q.y),wrap);}
 var sum=vec4(0.0); var total=0.0;
 for(var j=-2;j<=3;j++){ let y=floor(q.y)+f32(j); let wy=lanczos(q.y-y);
  for(var k=-2;k<=3;k++){ let x=floor(q.x)+f32(k); let weight=wy*lanczos(q.x-x);
   sum+=weight*tap(i32(x),i32(y),wrap);total+=weight;
  }
 } return sum/total;
}
fn hash(v0:u32)->u32 {var v=v0;v=v^(v>>16u);v=v*0x7feb352du;v=v^(v>>15u);v=v*0x846ca68bu;return v^(v>>16u);}
fn noise(x:u32,y:u32,c:u32)->f32 {
 let seed=u32(p[9])|(u32(p[10])<<16u);
 let key=x*0x9e3779b9u ^ y*0x85ebca6bu ^ c*0xc2b2ae35u ^ seed;
 let u=(f32(hash(key)>>8u)+0.5)/16777216.0;
 if p[12]==0.0 {return 2.0*u-1.0;}
 let v=(f32(hash(key^0xa511e9b3u)>>8u)+0.5)/16777216.0;
 return sqrt(-2.0*log(u))*cos(6.283185307179586*v);
}
fn radial(kind:u32,radius:f32,r:f32)->f32 {
 if kind==14u {let k=6.283185307179586/p[17];let s=length(vec2(r,1.0/k));return r+p[16]*r/s*sin(k*s+p[18]);}
 let b=p[16]*select(0.5,-0.5,kind==10u);return r*(1.0+b/(1.0+(r/radius)*(r/radius)));
}
fn inverse(kind:u32,pos:vec2<f32>)->vec2<f32> {
 if kind==17u {return vec2(modulo(pos.x-p[19],p[0]),modulo(pos.y-p[20],p[1]));}
 if p[16]==0.0 && kind!=15u && kind!=16u {return pos;}
 let wh=vec2(p[0]-1.0,p[1]-1.0);let center=vec2(p[21],p[22])*wh;
 let radius=max(min(wh.x,wh.y)*0.5,1.0);let d=pos-center;let r=length(d);
 if kind==13u {return pos-vec2(p[16]*sin(6.283185307179586*pos.y/p[17]+p[18]),0.0);}
 if kind==12u {let angle=-p[16]/(1.0+(r/radius)*(r/radius));let s=sin(angle);let c=cos(angle);return center+vec2(c*d.x-s*d.y,s*d.x+c*d.y);}
 if kind==15u {let angle=6.283185307179586*pos.x/max(wh.x,1.0);let rad=pos.y*radius/max(wh.y,1.0);return center+rad*vec2(cos(angle),sin(angle));}
 if kind==16u {var angle=0.0;if any(d!=vec2(0.0)){angle=atan2(d.y,d.x);}return vec2(modulo(angle,6.283185307179586)*wh.x/6.283185307179586,r*wh.y/radius);}
 if r==0.0 {return center;}
 var low=0.0;var high=select(r*2.0,r+abs(p[16]),kind==14u);
 for(var j=0;j<40;j++){let mid=(low+high)*0.5;if radial(kind,radius,mid)<r {low=mid;} else {high=mid;}}
 return center+d*((low+high)*0.5)/r;
}
@compute @workgroup_size(8,8)
fn main(@builtin(global_invocation_id) gid:vec3<u32>) {
 if gid.x>=u32(p[0]) || gid.y>=u32(p[1]) {return;}
 let i=gid.y*u32(p[0])+gid.x; let op=u32(p[2]);
 if op==40u {
  let f=i32(p[16]);let pad=i32(p[17]);var sum=vec4(0.0);
  for(var dy=0;dy<f;dy++){for(var dx=0;dx<f;dx++){
   sum+=sized_at(i32(gid.x)*f+dx-pad,i32(gid.y)*f+dy-pad,i32(p[18]),i32(p[19]))/f32(f*f);
  }}dst[i]=sum;return;
 }
 if op==41u {
  let q=(vec2<f32>(gid.xy)+vec2(p[17]-(p[16]-1.0)*0.5))/p[16];let t=fract(q);let pos=vec2<i32>(floor(q));let w=i32(p[20]);let h=i32(p[21]);
  let a=sized_at(pos.x,pos.y,w,h);let b=sized_at(pos.x+1,pos.y,w,h);let c=sized_at(pos.x,pos.y+1,w,h);let d=sized_at(pos.x+1,pos.y+1,w,h);
  let top=a+t.x*(b-a);let bot=c+t.x*(d-c);dst[i]=top+t.y*(bot-top);return;
 }
 let a=original[i]; var v=src[i];
 if op<2u {
  v=vec4(0.0); let n=i32(p[8]);
  for(var j=0;j<n;j++){ let d=j-n/2; v+=p[32+j]*at(i32(gid.x)+select(d,0,op==1u),i32(gid.y)+select(0,d,op==1u)); }
  dst[i]=v; return;
 }
 if op==3u || op==4u {
  if p[4]==0.0 {v=a;} else {
   let d=a.rgb-v.rgb;
   v=vec4(select(a.rgb+select(vec3(0.0),p[7]*d,abs(d)>=vec3(p[6])),vec3(0.5)+d,op==4u),a.a);
  }
 }
 if op>=5u && op<=7u {
  let n=2*i32(ceil(p[4]))+1; v=vec4(0.0); let center=vec2(p[0]-1.0,p[1]-1.0)*0.5;let pos=vec2<f32>(gid.xy);let delta=pos-center;
  for(var j=0;j<n;j++) {
   var t=0.0;if n>1 {t=f32(j)/f32(n-1)*2.0-1.0;}
   var q=pos+t*p[4]*vec2(cos(p[5]),sin(p[5]));
   if op==6u {let s=sin(t*p[5]);let c=cos(t*p[5]);q=center+vec2(c*delta.x-s*delta.y,s*delta.x+c*delta.y);}
   if op==7u {q=center+delta*(1.0+t*p[5]);}
   v+=sample(q,false)/f32(n);
  }
 }
 if op==8u && p[4]>0.0 && p[6]>0.0 {
  let r=i32(ceil(p[4]));var sum=vec4(0.0);var total=0.0;
  for(var dy=-r;dy<=r;dy++){for(var dx=-r;dx<=r;dx++) {
   let q=at(i32(gid.x)+dx,i32(gid.y)+dy);let d=q.rgb-a.rgb;
   let weight=exp(-0.5*(f32(dx*dx+dy*dy)/(p[4]*p[4])+dot(d,d)/(p[6]*p[6])));
   sum+=q*weight;total+=weight;
  }}v=sum/total;
 }
 if op==9u {for(var c=0u;c<3u;c++){v[c]=a[c]+p[7]*noise(gid.x,gid.y,select(c,0u,p[11]!=0.0));}}
 if op>=10u && op<=17u {v=sample(inverse(op,vec2<f32>(gid.xy)),op==17u);}
 if op==18u && p[4]>0.0 {
  let depth=p[32u+i];
  if depth<p[18] || depth>p[19] {
   var colour=vec3(0.0);var coverage=0.0;
   for(var layer=15;layer>=0;layer--) {
    let desc=u32(p[17])+u32(layer)*2u;let start=u32(p[desc]);let n=u32(p[desc+1u]);var sum=vec3(0.0);var count=0.0;var support=0.0;
    for(var j=0u;j<n;j++) {
     let x=i32(gid.x)+i32(p[start+2u*j]);let y=i32(gid.y)+i32(p[start+2u*j+1u]);
     if x<0 || y<0 || x>=i32(p[0]) || y>=i32(p[1]) {continue;}
     support+=1.0;let index=u32(y)*u32(p[0])+u32(x);let d=p[32u+index];
     if (d>=p[18] && d<=p[19]) || min(i32(d*16.0),15)!=layer {continue;}
     count+=1.0;sum+=src[index].rgb;
    }
    if count>0.0 {let alpha=count/support;colour=sum/count*alpha+colour*(1.0-alpha);coverage=alpha+coverage*(1.0-alpha);}
   }
   if coverage>0.0 {v=vec4(colour/coverage,a.a);}
  }
 }
 if op>=20u {v=vec4(adjustment(op,a.rgb),a.a);}
 dst[i]=a+p[3]*(v-a);
}
