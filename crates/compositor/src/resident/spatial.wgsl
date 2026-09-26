struct Params { width:u32, height:u32, vertical:u32, radius:f32 }
@group(0) @binding(0) var<storage,read> original:array<vec4<f32>>;
@group(0) @binding(1) var<storage,read> horizontal:array<f32>;
@group(0) @binding(2) var<storage,read_write> output:array<f32>;
@group(0) @binding(3) var<uniform> p:Params;
fn luminance(i:u32)->f32 {
    if(p.vertical!=0u){return horizontal[i];}
    let px=original[i];
    if(px.w<=0.0){return 0.0;}
    let c=px.xyz/px.w;
    return 0.2126*c.x+0.7152*c.y+0.0722*c.z;
}
@compute @workgroup_size(256)
fn main(@builtin(workgroup_id) group:vec3<u32>, @builtin(num_workgroups) groups:vec3<u32>, @builtin(local_invocation_index) local:u32){
    let i=(group.y*groups.x+group.x)*256u+local;
    if(i>=p.width*p.height){return;}
    let id=vec2<u32>(i%p.width,i/p.width);
    let center=luminance(i);
    let support=i32(ceil(p.radius));
    let sigma=max(p.radius*0.5,0.5);
    var sum=0.0;var weight=0.0;
    for(var d=-support;d<=support;d++){
        let x=u32(clamp(i32(id.x)+select(d,0,p.vertical!=0u),0,i32(p.width)-1));
        let y=u32(clamp(i32(id.y)+select(0,d,p.vertical!=0u),0,i32(p.height)-1));
        let j=y*p.width+x;
        let alpha=original[j].w;
        if(alpha<=0.0){continue;}
        let value=luminance(j);let delta=value-center;let distance=f32(d);
        let w=exp(-(distance*distance)/(2.0*sigma*sigma)-(delta*delta)/(2.0*0.15*0.15))*alpha;
        sum=sum+w*value;weight=weight+w;
    }
    var value=center;
    if(weight>0.0){value=sum/weight;}
    output[i]=value;
}
