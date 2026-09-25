// Scalar reference operators, with planar f32 input and output.
// p[0]: kind (highlights, demosaic, matrix, tone, display).
// p[1..9]: width, height, input halo, output halo, input plane length,
//           output plane length, absolute pixel origin x, origin y.
// p[9]: mode; p[10..14]: normalized Bayer channels in row-major order.
// p[16..25]: row-major matrix; p[25]: exposure gain; p[26]: contrast slope;
// p[27..31]: highlights, shadows, whites, blacks divided by 100;
// p[31]: neutral tone flag (all adjustments OTHER THAN exposure are zero).
// p[32]: ln(a) for the default display sigmoid (contrast=1.5, skew=0).
// Host validates layouts, finite parameters, CFA and required halo, and
// handles X-Trans on CPU. Highlights/demosaic/display have zero output halo.
@group(0) @binding(0) var<storage, read> src: array<f32>;
@group(0) @binding(1) var<storage, read_write> dst: array<f32>;
@group(0) @binding(2) var<storage, read> p: array<f32>;

fn input_index(x: i32, y: i32) -> u32 {
    let halo = i32(p[3]);
    let stride = u32(p[1]) + 2u * u32(p[3]);
    return u32(y + halo) * stride + u32(x + halo);
}

fn mosaic_get(x: i32, y: i32) -> f32 {
    return src[input_index(x, y)];
}

fn channel_at(x: i32, y: i32) -> u32 {
    // Bit parity also implements Euclidean modulo for negative halo positions.
    let cx = u32((i32(p[7]) + x) & 1);
    let cy = u32((i32(p[8]) + y) & 1);
    return u32(p[10u + cy * 2u + cx]);
}

fn read_rgb(index: u32) -> vec3<f32> {
    let n = u32(p[5]);
    return vec3<f32>(src[index], src[n + index], src[2u * n + index]);
}

fn write_rgb(index: u32, v: vec3<f32>) {
    let n = u32(p[6]);
    dst[index] = v.x;
    dst[n + index] = v.y;
    dst[2u * n + index] = v.z;
}

fn luminance(v: vec3<f32>) -> f32 {
    // Do not use dot(): keep the CPU's left-associated scalar expression.
    return (0.2627 * v.x + 0.6780 * v.y) + 0.0593 * v.z;
}

fn matrix(v: vec3<f32>) -> vec3<f32> {
    return vec3<f32>(
        (p[16] * v.x + p[17] * v.y) + p[18] * v.z,
        (p[19] * v.x + p[20] * v.y) + p[21] * v.z,
        (p[22] * v.x + p[23] * v.y) + p[24] * v.z,
    );
}

fn highlight_proxy(x: i32, y: i32, channel: u32) -> f32 {
    var sum = 0.0;
    var count = 0u;
    for (var dy = -1; dy <= 1; dy = dy + 1) {
        for (var dx = -1; dx <= 1; dx = dx + 1) {
            let v = mosaic_get(x + dx, y + dy);
            if channel_at(x + dx, y + dy) != channel && v > 0.0 && v < 1.0 {
                sum = sum + v;
                count = count + 1u;
            }
        }
    }
    if count > 0u {
        return sum / f32(count);
    }
    return 0.0;
}

fn highlights(x: i32, y: i32) -> f32 {
    let value = mosaic_get(x, y);
    if value < 1.0 || u32(p[9]) == 0u {
        return min(value, 1.0);
    }
    let channel = channel_at(x, y);
    let target_proxy = highlight_proxy(x, y, channel);
    var ratios = 0.0;
    var count = 0u;
    for (var dy = -3; dy <= 3; dy = dy + 1) {
        for (var dx = -3; dx <= 3; dx = dx + 1) {
            let v = mosaic_get(x + dx, y + dy);
            if channel_at(x + dx, y + dy) == channel && v > 0.0 && v < 1.0 {
                let proxy = highlight_proxy(x + dx, y + dy, channel);
                if proxy > 1e-6 {
                    ratios = ratios + v / proxy;
                    count = count + 1u;
                }
            }
        }
    }
    if count > 0u && target_proxy > 0.0 {
        return clamp(target_proxy * ratios / f32(count), 1.0, 4.0);
    }
    return 1.0;
}

fn bilinear(x: i32, y: i32, channel: u32, fallback: f32) -> f32 {
    var sum = 0.0;
    var count = 0u;
    for (var dy = -1; dy <= 1; dy = dy + 1) {
        for (var dx = -1; dx <= 1; dx = dx + 1) {
            if channel_at(x + dx, y + dy) == channel {
                sum = sum + mosaic_get(x + dx, y + dy);
                count = count + 1u;
            }
        }
    }
    if count > 0u {
        return sum / f32(count);
    }
    return fallback;
}

fn demosaic(x: i32, y: i32) -> vec3<f32> {
    let known = channel_at(x, y);
    let v = mosaic_get(x, y);
    var result = vec3<f32>(v);
    for (var c = 0u; c < 3u; c = c + 1u) {
        if c == known {
            continue;
        }
        if u32(p[9]) == 0u {
            result[c] = bilinear(x, y, c, v);
        } else {
            let h1 = mosaic_get(x - 1, y) + mosaic_get(x + 1, y);
            let v1 = mosaic_get(x, y - 1) + mosaic_get(x, y + 1);
            let h2 = mosaic_get(x - 2, y) + mosaic_get(x + 2, y);
            let v2 = mosaic_get(x, y - 2) + mosaic_get(x, y + 2);
            let diag = ((mosaic_get(x - 1, y - 1) + mosaic_get(x + 1, y - 1))
                + mosaic_get(x - 1, y + 1)) + mosaic_get(x + 1, y + 1);
            if c == 1u {
                result[c] = (4.0 * v + 2.0 * (h1 + v1) - h2 - v2) / 8.0;
            } else if known != 1u {
                result[c] = (6.0 * v + 2.0 * diag - 1.5 * (h2 + v2)) / 8.0;
            } else if channel_at(x + 1, y) == c {
                result[c] = (5.0 * v + 4.0 * h1 - h2 - diag + 0.5 * v2) / 8.0;
            } else {
                result[c] = (5.0 * v + 4.0 * v1 - v2 - diag + 0.5 * h2) / 8.0;
            }
        }
    }
    return result;
}

// WGSL lacks log1p/expm1. Correct cancellation near zero rather than losing
// shadow detail to log(1+x) or exp(x)-1. Transcendentals remain GPU f32, not
// a promise of bit-identical results to the CPU's libm implementation.
fn log_one_plus(x: f32) -> f32 {
    let u = 1.0 + x;
    if u == 1.0 {
        return x;
    }
    return log(u) * (x / (u - 1.0));
}

fn exp_minus_one(x: f32) -> f32 {
    if abs(x) < 0.5 {
        // Taylor series through degree ten; Horner order avoids cancellation.
        var r = 1.0 / 3628800.0;
        r = 1.0 / 362880.0 + x * r;
        r = 1.0 / 40320.0 + x * r;
        r = 1.0 / 5040.0 + x * r;
        r = 1.0 / 720.0 + x * r;
        r = 1.0 / 120.0 + x * r;
        r = 1.0 / 24.0 + x * r;
        r = 1.0 / 6.0 + x * r;
        r = 0.5 + x * r;
        return x * (1.0 + x * r);
    }
    return exp(x) - 1.0;
}

fn softplus(v: f32) -> f32 {
    return max(v, 0.0) + log_one_plus(exp(-abs(v)));
}

fn tone(v: vec3<f32>) -> vec3<f32> {
    let rgb = v * p[25];
    // Exposure-only is also neutral in the CPU reference, even if the host
    // uses p[31] only for the completely zero ToneSettings fast path.
    if p[31] == 1.0 || (p[26] == 1.0 && p[27] == 0.0 && p[28] == 0.0
        && p[29] == 0.0 && p[30] == 0.0) {
        return rgb;
    }
    let y = luminance(rgb);
    if y <= 0.0 {
        return rgb;
    }
    let initial_z = log_one_plus(y / 0.18);
    let pivot = 0.6931471805599453;
    let slope = p[26];
    let z = slope * initial_z
        + (1.0 - slope) * 2.0 * pivot * -exp_minus_one(-initial_z);
    var out = z;
    // Preserve black, shadow, highlight, white accumulation order.
    // Amounts are supplied clamped and normalized by the host.
    let black_region = z - (softplus(z - 0.25) - softplus(-0.25));
    out = out + 0.2 * p[30] * black_region;
    let shadow_region = z - (softplus(z - 0.8) - softplus(-0.8));
    out = out + 0.2 * p[28] * shadow_region;
    let highlight_region = softplus(z - 1.5) - softplus(-1.5);
    out = out + 0.2 * p[27] * highlight_region;
    let white_region = softplus(z - 2.5) - softplus(-2.5);
    out = out + 0.2 * p[29] * white_region;
    let scale = 0.18 * exp_minus_one(out) / y;
    return rgb * scale;
}

fn srgb_oetf(v: f32) -> f32 {
    if v <= 0.0031308 {
        return 12.92 * v;
    }
    return 1.055 * pow(v, 1.0 / 2.4) - 0.055;
}

fn display(rgb: vec3<f32>, x: u32, y: u32) -> vec3<f32> {
    let lum = luminance(rgb);
    var mapped = vec3<f32>(0.0);
    if lum > 0.0 {
        let sigmoid = 1.0 / (1.0 + exp(1.5 * (p[32] - log(lum))));
        // CPU performs channel * sigmoid / luminance, not channel * (s/y).
        mapped = vec3<f32>(rgb.x * sigmoid / lum,
            rgb.y * sigmoid / lum, rgb.z * sigmoid / lum);
    }
    let v = matrix(mapped);
    let grey = clamp((0.2126 * v.x + 0.7152 * v.y) + 0.0722 * v.z, 0.0, 1.0);
    var chroma = 1.0;
    if u32(p[9]) == 1u {
        for (var c = 0u; c < 3u; c = c + 1u) {
            let d = v[c] - grey;
            if v[c] < 0.0 {
                chroma = min(chroma, -grey / d);
            }
            if v[c] > 1.0 {
                chroma = min(chroma, (1.0 - grey) / d);
            }
        }
    }
    let bayer = array<f32, 16>(0.0, 8.0, 2.0, 10.0,
        12.0, 4.0, 14.0, 6.0, 3.0, 11.0, 1.0, 9.0,
        15.0, 7.0, 13.0, 5.0);
    let bx = (u32(p[7]) + x) % 4u;
    let by = (u32(p[8]) + y) % 4u;
    let noise = (bayer[by * 4u + bx] + 0.5) / 16.0 - 0.5;
    var result = vec3<f32>(0.0);
    for (var c = 0u; c < 3u; c = c + 1u) {
        var linear = v[c];
        if u32(p[9]) == 1u {
            linear = grey + chroma * (v[c] - grey);
        }
        let encoded = srgb_oetf(clamp(linear, 0.0, 1.0)) * 255.0 + noise;
        result[c] = clamp(floor(encoded + 0.5), 0.0, 255.0);
    }
    return result;
}

@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) global_id: vec3<u32>) {
    let i = global_id.x;
    if i >= u32(p[6]) {
        return;
    }
    let kind = u32(p[0]);
    let output_stride = u32(p[1]) + 2u * u32(p[4]);
    let x = i32(i % output_stride) - i32(p[4]);
    let y = i32(i / output_stride) - i32(p[4]);
    switch kind {
        case 0u: {
            dst[i] = highlights(x, y);
        }
        case 1u: {
            write_rgb(i, demosaic(x, y));
        }
        case 2u: {
            write_rgb(i, matrix(read_rgb(input_index(x, y))));
        }
        case 3u: {
            write_rgb(i, tone(read_rgb(input_index(x, y))));
        }
        case 4u: {
            write_rgb(i, display(read_rgb(input_index(x, y)), u32(x), u32(y)));
        }
        default: {}
    }
}
