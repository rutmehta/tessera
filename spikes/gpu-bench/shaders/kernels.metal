// Native MSL versions of the three spike kernels. Same math and summation
// order as the WGSL and CPU reference. W and H are substituted by the host.
#include <metal_stdlib>
using namespace metal;

constant int W = __W__;
constant int H = __H__;

// ---------------------------------------------------------------- demosaic
static inline int refl(int v, int n) {
    int r = v;
    if (r < 0) r = -r;
    if (r >= n) r = 2 * n - 2 - r;
    return r;
}

static inline float fetch(device const ushort* cfa, int x, int y) {
    return float(cfa[refl(y, H) * W + refl(x, W)]) * (1.0f / 65535.0f);
}

kernel void demosaic(device const ushort* cfa [[buffer(0)]],
                     device float4* dst [[buffer(1)]],
                     uint2 gid [[thread_position_in_grid]]) {
    int x = int(gid.x), y = int(gid.y);
    if (x >= W || y >= H) return;
    float c = fetch(cfa, x, y);
    float l = fetch(cfa, x - 1, y);
    float r = fetch(cfa, x + 1, y);
    float u = fetch(cfa, x, y - 1);
    float d = fetch(cfa, x, y + 1);
    float cross = (l + r + u + d) * 0.25f;
    float diag = (fetch(cfa, x - 1, y - 1) + fetch(cfa, x + 1, y - 1) +
                  fetch(cfa, x - 1, y + 1) + fetch(cfa, x + 1, y + 1)) * 0.25f;
    float horiz = (l + r) * 0.5f;
    float vert = (u + d) * 0.5f;
    int px = x & 1, py = y & 1;
    float3 o;
    if (py == 0 && px == 0)      o = float3(c, cross, diag);
    else if (py == 0)            o = float3(horiz, c, vert);
    else if (px == 0)            o = float3(vert, c, horiz);
    else                         o = float3(diag, cross, c);
    dst[y * W + x] = float4(o, 1.0f);
}

// ----------------------------------------------------------- guided filter
constant int R = 8;
constant float EPS = 1e-3f;

kernel void hbox_in(device const float4* s0 [[buffer(0)]],
                    device float4* d0 [[buffer(3)]],
                    device float4* d1 [[buffer(4)]],
                    uint2 gid [[thread_position_in_grid]]) {
    int x = int(gid.x), y = int(gid.y);
    if (x >= W || y >= H) return;
    int x0 = max(x - R, 0), x1 = min(x + R, W - 1);
    float4 s = 0.0f, s2 = 0.0f;
    for (int k = x0; k <= x1; k++) {
        float4 v = s0[y * W + k];
        s += v;
        s2 += v * v;
    }
    float inv = 1.0f / float(x1 - x0 + 1);
    d0[y * W + x] = s * inv;
    d1[y * W + x] = s2 * inv;
}

kernel void vbox_coef(device const float4* s0 [[buffer(0)]],
                      device const float4* s1 [[buffer(1)]],
                      device float4* d0 [[buffer(3)]],
                      device float4* d1 [[buffer(4)]],
                      uint2 gid [[thread_position_in_grid]]) {
    int x = int(gid.x), y = int(gid.y);
    if (x >= W || y >= H) return;
    int y0 = max(y - R, 0), y1 = min(y + R, H - 1);
    float4 m = 0.0f, m2 = 0.0f;
    for (int k = y0; k <= y1; k++) {
        m += s0[k * W + x];
        m2 += s1[k * W + x];
    }
    float inv = 1.0f / float(y1 - y0 + 1);
    m = m * inv;
    m2 = m2 * inv;
    float4 v = max(m2 - m * m, float4(0.0f));
    float4 a = v / (v + float4(EPS));
    d0[y * W + x] = a;
    d1[y * W + x] = m - a * m;
}

kernel void hbox_ab(device const float4* s0 [[buffer(0)]],
                    device const float4* s1 [[buffer(1)]],
                    device float4* d0 [[buffer(3)]],
                    device float4* d1 [[buffer(4)]],
                    uint2 gid [[thread_position_in_grid]]) {
    int x = int(gid.x), y = int(gid.y);
    if (x >= W || y >= H) return;
    int x0 = max(x - R, 0), x1 = min(x + R, W - 1);
    float4 sa = 0.0f, sb = 0.0f;
    for (int k = x0; k <= x1; k++) {
        sa += s0[y * W + k];
        sb += s1[y * W + k];
    }
    float inv = 1.0f / float(x1 - x0 + 1);
    d0[y * W + x] = sa * inv;
    d1[y * W + x] = sb * inv;
}

kernel void vbox_out(device const float4* s0 [[buffer(0)]],
                     device const float4* s1 [[buffer(1)]],
                     device const float4* s2 [[buffer(2)]],
                     device float4* d0 [[buffer(3)]],
                     uint2 gid [[thread_position_in_grid]]) {
    int x = int(gid.x), y = int(gid.y);
    if (x >= W || y >= H) return;
    int y0 = max(y - R, 0), y1 = min(y + R, H - 1);
    float4 ma = 0.0f, mb = 0.0f;
    for (int k = y0; k <= y1; k++) {
        ma += s0[k * W + x];
        mb += s1[k * W + x];
    }
    float inv = 1.0f / float(y1 - y0 + 1);
    d0[y * W + x] = (ma * inv) * s2[y * W + x] + mb * inv;
}

// ------------------------------------------------------------ Oklab 3D LUT
constant int N = 33;

static inline float cbrt1(float v) { return sign(v) * pow(abs(v), 1.0f / 3.0f); }

static inline float3 to_oklab(float3 c) {
    float l = 0.4122214708f * c.x + 0.5363325363f * c.y + 0.0514459929f * c.z;
    float m = 0.2119034982f * c.x + 0.6806995451f * c.y + 0.1073969566f * c.z;
    float s = 0.0883024619f * c.x + 0.2817188376f * c.y + 0.6299787005f * c.z;
    float l_ = cbrt1(l), m_ = cbrt1(m), s_ = cbrt1(s);
    return float3(0.2104542553f * l_ + 0.7936177850f * m_ - 0.0040720468f * s_,
                  1.9779984951f * l_ - 2.4285922050f * m_ + 0.4505937099f * s_,
                  0.0259040371f * l_ + 0.7827717662f * m_ - 0.8086757660f * s_);
}

static inline float3 from_oklab(float3 c) {
    float l_ = c.x + 0.3963377774f * c.y + 0.2158037573f * c.z;
    float m_ = c.x - 0.1055613458f * c.y - 0.0638541728f * c.z;
    float s_ = c.x - 0.0894841775f * c.y - 1.2914855480f * c.z;
    float l = l_ * l_ * l_, m = m_ * m_ * m_, s = s_ * s_ * s_;
    return float3(4.0767416621f * l - 3.3077115913f * m + 0.2309699292f * s,
                  -1.2684380046f * l + 2.6097574011f * m - 0.3413193965f * s,
                  -0.0041960863f * l - 0.7034186147f * m + 1.7076147010f * s);
}

static inline float3 lerp3(float3 a, float3 b, float t) { return a + (b - a) * t; }

kernel void lut3d(device const float4* src [[buffer(0)]],
                  device const float4* lut [[buffer(1)]],
                  device float4* dst [[buffer(2)]],
                  uint2 gid [[thread_position_in_grid]]) {
    int x = int(gid.x), y = int(gid.y);
    if (x >= W || y >= H) return;
    float4 p = src[y * W + x];
    float3 lab = to_oklab(p.xyz);
    float3 t = clamp(float3(lab.x, lab.y + 0.5f, lab.z + 0.5f), 0.0f, 1.0f) * 32.0f;
    int3 i0 = min(int3(floor(t)), int3(31));
    float3 fr = t - float3(i0);
    #define NODE(i, j, k) lut[((k) * N + (j)) * N + (i)].xyz
    float3 c00 = lerp3(NODE(i0.x, i0.y, i0.z),         NODE(i0.x + 1, i0.y, i0.z), fr.x);
    float3 c10 = lerp3(NODE(i0.x, i0.y + 1, i0.z),     NODE(i0.x + 1, i0.y + 1, i0.z), fr.x);
    float3 c01 = lerp3(NODE(i0.x, i0.y, i0.z + 1),     NODE(i0.x + 1, i0.y, i0.z + 1), fr.x);
    float3 c11 = lerp3(NODE(i0.x, i0.y + 1, i0.z + 1), NODE(i0.x + 1, i0.y + 1, i0.z + 1), fr.x);
    #undef NODE
    float3 c0 = lerp3(c00, c10, fr.y);
    float3 c1 = lerp3(c01, c11, fr.y);
    dst[y * W + x] = float4(from_oklab(lerp3(c0, c1, fr.z)), p.w);
}
