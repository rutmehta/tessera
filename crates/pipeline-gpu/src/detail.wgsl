// Immutable planar input; integer copies preserve neutral/halo bits exactly.
@group(0) @binding(0) var<storage, read> src: array<u32>;
@group(0) @binding(1) var<storage, read_write> dst: array<u32>;
// n, stride, halo, width, height, sharp/lum/chroma, sharp/chroma radius,
// amount, radius, detail, masking, luminance, lum detail/contrast, color/detail/smoothness.
@group(0) @binding(2) var<storage, read> p: array<f32>;
@group(0) @binding(3) var<storage, read_write> dec: array<vec4<f32>>;

fn rgb_at(i: u32) -> vec3<f32> {
    let n = u32(p[0]);
    return vec3<f32>(bitcast<f32>(src[i]), bitcast<f32>(src[n+i]), bitcast<f32>(src[2u*n+i]));
}
fn y_of(v: vec3<f32>) -> f32 { return 0.2627*v.x + 0.6780*v.y + 0.0593*v.z; }
fn cbrt_signed(v: f32) -> f32 {
    if v == 0.0 { return v; }
    return sign(v) * pow(abs(v), 1.0/3.0);
}
fn to_lab(v: vec3<f32>) -> vec3<f32> {
    let a = cbrt_signed(0.6167558*v.x + 0.3601984*v.y + 0.0230458*v.z);
    let b = cbrt_signed(0.265133*v.x + 0.6358394*v.y + 0.0990276*v.z);
    let c = cbrt_signed(0.1001026*v.x + 0.2039065*v.y + 0.6959909*v.z);
    return vec3<f32>(0.21045426*a + 0.7936178*b - 0.004072047*c,
        1.9779985*a - 2.4285922*b + 0.4505937*c,
        0.025904037*a + 0.78277177*b - 0.80867577*c);
}
fn from_lab(v: vec3<f32>) -> vec3<f32> {
    let a = v.x + 0.39633778*v.y + 0.21580376*v.z;
    let b = v.x - 0.105561346*v.y - 0.06385417*v.z;
    let c = v.x - 0.08948418*v.y - 1.2914855*v.z;
    let x = a*a*a; let y = b*b*b; let z = c*c*c;
    return vec3<f32>(2.1399066*x - 1.2463895*y + 0.1064829*z,
        -0.8847359*x + 2.163231*y - 0.2784951*z,
        -0.0485738*x - 0.4545031*y + 1.5030769*z);
}
@compute @workgroup_size(64)
fn decompose(@builtin(global_invocation_id) id: vec3<u32>) {
    let i = id.x;
    if i >= u32(p[0]) { return; }
    let rgb = rgb_at(i);
    var lab = vec3<f32>(0.0);
    if p[7] != 0.0 { lab = to_lab(rgb); }
    dec[i] = vec4<f32>(y_of(rgb), lab);
}
fn y_at(i: i32) -> f32 { return dec[u32(i)].x; }
fn edge(i: i32, stride: i32) -> f32 {
    var sum = 0.0;
    for (var dy = -1; dy <= 1; dy++) {
        for (var dx = -1; dx <= 1; dx++) {
            let j = i + dy*stride + dx;
            let gx = y_at(j-stride+1) + 2.0*y_at(j+1) + y_at(j+stride+1)
                - y_at(j-stride-1) - 2.0*y_at(j-1) - y_at(j+stride-1);
            let gy = y_at(j+stride-1) + 2.0*y_at(j+stride) + y_at(j+stride+1)
                - y_at(j-stride-1) - 2.0*y_at(j-stride) - y_at(j-stride+1);
            sum += length(vec2<f32>(gx, gy))/8.0;
        }
    }
    return sum/9.0;
}
fn spatial(dx: i32, dy: i32, sigma: f32) -> f32 {
    return exp(-0.5*f32(dx*dx+dy*dy)/(sigma*sigma));
}
@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) id: vec3<u32>) {
    let i = id.x; let n = u32(p[0]);
    if i >= n { return; }
    let stride = u32(p[1]); let halo = u32(p[2]);
    let x = i%stride; let y = i/stride;
    if x < halo || y < halo || x >= halo+u32(p[3]) || y >= halo+u32(p[4])
        || (p[5] == 0.0 && p[6] == 0.0 && p[7] == 0.0) {
        dst[i] = src[i]; dst[n+i] = src[n+i]; dst[2u*n+i] = src[2u*n+i];
        return;
    }
    let base = dec[i].x;
    var target_y = base;
    if p[5] != 0.0 {
        var delta = 0.0; var total = 0.0;
        let radius = i32(p[8]);
        for (var dy = -radius; dy <= radius; dy++) {
            for (var dx = -radius; dx <= radius; dx++) {
                let w = spatial(dx,dy,p[11]);
                delta += w*(y_at(i32(i)+dy*i32(stride)+dx)-base);
                total += w;
            }
        }
        let residual = -delta/total;
        let d = p[12]/100.0;
        let limit = 0.05*(abs(base)+0.1);
        let boost = (1.0-d)*clamp(residual,-limit,limit) + d*residual*1.5;
        let masking = p[13]/100.0;
        var gate = 1.0;
        if masking > 0.0 {
            let t = clamp((edge(i32(i),i32(stride))/(abs(base)+0.1)-0.15*masking)/0.05,0.0,1.0);
            gate = t*t*(3.0-2.0*t);
        }
        target_y += p[10]/100.0*gate*boost;
    }
    if p[6] != 0.0 {
        let range = (0.02+0.18*(1.0-p[15]/100.0))*(abs(base)+0.1);
        var delta = 0.0; var total = 0.0; var variance = 0.0; var spatial_sum = 0.0;
        for (var dy = -2; dy <= 2; dy++) {
            for (var dx = -2; dx <= 2; dx++) {
                let w = spatial(dx,dy,1.2);
                let d = y_at(i32(i)+dy*i32(stride)+dx)-base;
                let ratio = d/range;
                let weight = w*exp(-0.5*(ratio*ratio));
                delta += weight*d; total += weight;
                variance += w*d*d; spatial_sum += w;
            }
        }
        let protect = 1.0/(1.0+8.0*(p[16]/100.0)*variance/spatial_sum/(range*range));
        target_y += (p[14]/100.0)*protect*delta/total;
    }
    var out = rgb_at(i);
    if p[7] != 0.0 {
        let lab = dec[i].yzw;
        let range = 0.02+0.18*(1.0-p[18]/100.0);
        var delta = vec2<f32>(0.0); var total = 0.0;
        let radius = i32(p[9]);
        let sigma = 0.7+2.0*(p[19]/100.0);
        for (var dy = -radius; dy <= radius; dy++) {
            for (var dx = -radius; dx <= radius; dx++) {
                let w = spatial(dx,dy,sigma);
                let q = dec[u32(i32(i)+dy*i32(stride)+dx)].yzw;
                let da = q.y-lab.y; let db = q.z-lab.z;
                let dl = (q.x-lab.x)/0.08;
                let weight = w*exp(-0.5*((da*da+db*db)/(range*range)+dl*dl));
                delta += weight*vec2<f32>(da,db); total += weight;
            }
        }
        let amount = (p[17]/100.0)/total;
        if any(delta != vec2<f32>(0.0)) {
            out = from_lab(vec3<f32>(lab.x, lab.y+amount*delta.x, lab.z+amount*delta.y));
        }
    }
    let shift = target_y-y_of(out);
    out += vec3<f32>(shift);
    dst[i] = bitcast<u32>(out.x); dst[n+i] = bitcast<u32>(out.y); dst[2u*n+i] = bitcast<u32>(out.z);
}
