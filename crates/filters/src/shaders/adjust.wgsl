// Straight-alpha scene-linear Rec.2020 adjustments. Data starts at p[32].
fn rgb_at(base:u32)->vec3<f32>{return vec3(p[base],p[base+1u],p[base+2u]);}
fn luma(rgb:vec3<f32>)->f32{return 0.2627*rgb.r+0.6780*rgb.g+0.0593*rgb.b;}
fn to_hsl(rgb:vec3<f32>)->vec3<f32>{
 let hi=max(max(rgb.r,rgb.g),rgb.b);let lo=min(min(rgb.r,rgb.g),rgb.b);let d=hi-lo;let l=(hi+lo)*0.5;
 if d<=1e-10 {return vec3(0.0,0.0,l);}
 var h=0.0;
 if hi==rgb.r {h=modulo((rgb.g-rgb.b)/d,6.0);} else if hi==rgb.g {h=(rgb.b-rgb.r)/d+2.0;} else {h=(rgb.r-rgb.g)/d+4.0;}
 return vec3(h/6.0,d/max(1.0-abs(2.0*l-1.0),1e-10),l);
}
fn from_hsl(hsl:vec3<f32>)->vec3<f32>{
 let c=(1.0-abs(2.0*hsl.z-1.0))*hsl.y;let h=hsl.x*6.0;let x=c*(1.0-abs(modulo(h,2.0)-1.0));var rgb=vec3(c,0.0,x);
 switch u32(h) {case 0u:{rgb=vec3(c,x,0.0);} case 1u:{rgb=vec3(x,c,0.0);} case 2u:{rgb=vec3(0.0,c,x);} case 3u:{rgb=vec3(0.0,x,c);} case 4u:{rgb=vec3(x,0.0,c);} default:{}}
 return rgb+vec3(hsl.z-c*0.5);
}
fn shift_unit(v:f32,s:f32)->f32{if s>=0.0 {return v+(1.0-v)*s;}return v*(1.0+s);}
fn hue_weight(h:f32,band:u32)->f32{let d=abs(h*6.0-f32(band));return max(1.0-min(d,6.0-d),0.0);}
fn curve(c:u32,x:f32)->f32{
 let base=u32(p[16u+c*2u]);let n=u32(p[17u+c*2u]);let last=base+(n-1u)*3u;
 if x<=p[base]{return p[base+1u]+(x-p[base])*p[base+2u];}
 if x>=p[last]{return p[last+1u]+(x-p[last])*p[last+2u];}
 var i=base;for(var j=0u;j<n-2u;j++){if p[i+3u]>x {break;}i+=3u;}
 let h=p[i+3u]-p[i];let t=(x-p[i])/h;let t2=t*t;let t3=t2*t;
 return (2.0*t3-3.0*t2+1.0)*p[i+1u]+(t3-2.0*t2+t)*h*p[i+2u]+(-2.0*t3+3.0*t2)*p[i+4u]+(t3-t2)*h*p[i+5u];
}
fn matrix(base:u32,v:vec3<f32>)->vec3<f32>{return vec3(dot(rgb_at(base),v),dot(rgb_at(base+3u),v),dot(rgb_at(base+6u),v));}
fn adjustment(op:u32,rgb:vec3<f32>)->vec3<f32>{
 switch op {
 case 20u:{let t=clamp((rgb-rgb_at(32u))/(rgb_at(35u)-rgb_at(32u)),vec3(0.0),vec3(1.0));return pow(t,vec3(1.0)/rgb_at(38u))*(rgb_at(44u)-rgb_at(41u))+rgb_at(41u);}
 case 21u:{return vec3(curve(0u,rgb.r),curve(1u,rgb.g),curve(2u,rgb.b));}
 case 22u:{
  if p[34]!=0.0 {return (rgb-vec3(0.5))*(1.0+p[33]/100.0)+vec3(0.5+p[32]/150.0);}
  let v=rgb+(p[32]/150.0)*rgb*(vec3(1.0)-rgb);
  let out=v+(p[33]/100.0)*(v-vec3(0.5))*2.0*v*(vec3(1.0)-v);
  return select(rgb,out,(rgb>=vec3(0.0)) & (rgb<=vec3(1.0)));
 }
 case 23u:{let v=rgb*p[32]+vec3(p[33]);if p[34]==1.0{return v;}return sign(v)*pow(abs(v),vec3(p[34]));}
 case 24u:{return vec3(select(0.0,1.0,luma(rgb)>=p[32]));}
 case 25u:{return floor(clamp(rgb,vec3(0.0),vec3(1.0))*p[32]+vec3(0.5))/p[32];}
 case 26u:{let hsl=to_hsl(clamp(rgb,vec3(0.0),vec3(1.0)));return from_hsl(vec3(modulo(hsl.x+p[32],1.0),shift_unit(hsl.y,p[33]),shift_unit(hsl.z,p[34])));}
 case 27u:{let y=luma(rgb);let hi=max(max(rgb.r,rgb.g),rgb.b);let lo=min(min(rgb.r,rgb.g),rgb.b);var chroma=0.0;if hi>0.0{chroma=clamp((hi-lo)/hi,0.0,1.0);}let gain=(1.0+p[33])*(1.0+p[32]*(1.0-chroma));return vec3(y)+(rgb-vec3(y))*gain;}
 case 28u:{let filtered=rgb*(vec3(1.0-p[35])+p[35]*rgb_at(32u));let y=luma(filtered);if p[36]!=0.0 {if abs(y)>1e-10{return filtered*luma(rgb)/y;}return rgb;}return filtered;}
 case 29u:{return matrix(32u,rgb)+rgb_at(41u);}
 case 30u:{var t=clamp(luma(rgb),0.0,1.0);if p[17]!=0.0 {t=1.0-t;}var i=32u;for(var j=0u;j<u32(p[16])-2u;j++){if p[i+4u]>t {break;}i+=4u;}let u=(t-p[i])/(p[i+4u]-p[i]);return rgb_at(i+1u)*(1.0-u)+rgb_at(i+5u)*u;}
 case 31u:{
  let bounded=clamp(rgb,vec3(0.0),vec3(1.0));let hi=max(max(bounded.r,bounded.g),bounded.b);let lo=min(min(bounded.r,bounded.g),bounded.b);let chroma=hi-lo;let hue=to_hsl(bounded).x;
  var w:array<f32,9>;for(var i=0u;i<6u;i++){w[i]=hue_weight(hue,i)*chroma;}w[6]=max(2.0*lo-1.0,0.0);w[8]=max(1.0-2.0*hi,0.0);w[7]=max(1.0-chroma-w[6]-w[8],0.0);
  var correction=vec4(0.0);for(var i=0u;i<9u;i++){let b=32u+i*4u;correction+=w[i]*vec4(p[b],p[b+1u],p[b+2u],p[b+3u]);}
  let scale=select(vec3(1.0),vec3(1.0)-bounded,p[16]!=0.0);let black=select(1.0,1.0-hi,p[16]!=0.0);
  return rgb-correction.rgb*scale-vec3(correction.a*black);
 }
 case 32u:{let hsl=to_hsl(clamp(rgb,vec3(0.0),vec3(1.0)));var gain=0.0;for(var i=0u;i<6u;i++){gain+=hue_weight(hsl.x,i)*p[32u+i];}let y=luma(rgb)*(1.0+hsl.y*(gain-1.0));return rgb_at(38u)*y;}
 case 33u:{
  let lms=matrix(42u,rgb);let lab=matrix(51u,sign(lms)*pow(abs(lms),vec3(1.0/3.0)));
  let transferred=(lab-rgb_at(32u))*rgb_at(38u)+rgb_at(35u);let matched=lab+p[41]*(transferred-lab);
  let root=matrix(60u,matched);return matrix(69u,root*root*root);
 }
 case 34u:{return vec3(1.0)-rgb;}
 case 35u:{return vec3(luma(rgb));}
 default:{return rgb;}
 }
}
