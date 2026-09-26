// Layer compositor tile program interpreter (the per-tile correctness port;
// prepended with blend.wgsl). See COMPOSITOR.md §6.

struct Header {
    n: u32,
    w: u32,
    ox: u32,
    oy: u32,
    count: u32,
    p0: u32,
    p1: u32,
    p2: u32,
}

struct Op {
    kind: u32,     // 0 blend, 2 push isolated/clip, 3 push pass, 4 pop blend, 5 pop pass, 6 snapshot bg
    mode: u32,     // BlendMode::index
    src: u32,      // offset of straight RGBA planes in srcs
    mask: u32,     // offset of a mask plane in srcs, or 0xffffffff
    flags: u32,    // bit0 atop, bits1-2 knockout (1 shallow, 2 deep), bit3 blend-if
    opacity: f32,
    fill: f32,
    seed: u32,
    bi: array<vec4<f32>, 8>,
}

@group(0) @binding(0) var<storage, read> hdr: Header;
@group(0) @binding(1) var<storage, read> ops: array<Op>;
@group(0) @binding(2) var<storage, read> srcs: array<f32>;
@group(0) @binding(3) var<storage, read_write> outp: array<f32>;

const NO_MASK: u32 = 0xffffffffu;

fn composite(b: vec4<f32>, s: vec4<f32>, op: Op, oi: u32, kb_on: bool, k: vec4<f32>, x: u32, y: u32) -> vec4<f32> {
    let cb = unpremul(b);
    var sigma = s.w;
    if ((op.flags & 8u) != 0u) { sigma = sigma * blend_if(ops[oi].bi, s.xyz, cb); }
    return composite_core(b, cb, s.xyz, sigma, op.mode, op.flags, op.opacity, op.fill, op.seed, kb_on, k, x, y);
}

@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let i = gid.x;
    let n = hdr.n;
    if (i >= n) { return; }
    let x = hdr.ox + i % hdr.w;
    let y = hdr.oy + i / hdr.w;
    var acc: array<vec4<f32>, 8>;
    var kinds: array<u32, 8>;
    var sp = 0u;
    acc[0] = vec4<f32>(0.0);
    kinds[0] = 0u;
    var deep = vec4<f32>(0.0);
    for (var k = 0u; k < hdr.count; k = k + 1u) {
        let op = ops[k];
        switch op.kind {
            case 0u, 4u: {
                var s: vec4<f32>;
                if (op.kind == 0u) {
                    s = vec4<f32>(srcs[op.src + i], srcs[op.src + n + i], srcs[op.src + 2u * n + i], srcs[op.src + 3u * n + i]);
                } else {
                    let c = acc[sp];
                    sp = sp - 1u;
                    s = vec4<f32>(unpremul(c), c.w);
                    if (op.mask != NO_MASK) { s.w = s.w * srcs[op.mask + i]; }
                }
                let knock = (op.flags >> 1u) & 3u;
                var kb_on = false;
                var kb = vec4<f32>(0.0);
                if (knock == 2u) {
                    kb_on = true;
                    kb = deep;
                } else if (knock == 1u) {
                    kb_on = true;
                    if (kinds[sp] == 0u) { kb = deep; }
                    else if (kinds[sp] == 2u) { kb = acc[sp - 1u]; }
                }
                acc[sp] = composite(acc[sp], s, op, k, kb_on, kb, x, y);
            }
            case 2u: {
                sp = sp + 1u;
                acc[sp] = vec4<f32>(0.0);
                kinds[sp] = 1u;
            }
            case 3u: {
                sp = sp + 1u;
                acc[sp] = acc[sp - 1u];
                kinds[sp] = 2u;
            }
            case 5u: {
                let c = acc[sp];
                sp = sp - 1u;
                var t = op.opacity * op.fill;
                if (op.mask != NO_MASK) { t = t * srcs[op.mask + i]; }
                acc[sp] = acc[sp] + t * (c - acc[sp]);
            }
            case 6u: {
                deep = acc[0];
            }
            default: {}
        }
    }
    let r = acc[0];
    outp[i] = r.x;
    outp[n + i] = r.y;
    outp[2u * n + i] = r.z;
    outp[3u * n + i] = r.w;
}
