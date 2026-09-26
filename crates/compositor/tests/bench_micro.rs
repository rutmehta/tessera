//! Micro-benchmarks of the resident shader (ignored).
mod common;
use common::*;
use compositor::gpu::GpuCompositor;
use compositor::resident::ResidentRenderer;
use compositor::*;
use engine_api::tile::Extent;
use std::time::Instant;

fn time(r: &mut ResidentRenderer, d: &Document) -> f64 {
    r.render(d, 0).unwrap();
    r.wait().unwrap();
    let mut v = Vec::new();
    for _ in 0..5 {
        r.invalidate();
        let t = Instant::now();
        r.render(d, 0).unwrap();
        r.wait().unwrap();
        v.push(t.elapsed().as_secs_f64() * 1e3);
    }
    v.sort_by(f64::total_cmp);
    let t = Instant::now();
    for _ in 0..10 {
        r.invalidate();
        r.render(d, 0).unwrap();
    }
    r.wait().unwrap();
    println!(
        "   (back-to-back: {:.2} ms/frame)",
        t.elapsed().as_secs_f64() * 1e3 / 10.0
    );
    v[2]
}

#[test]
#[ignore = "bench"]
fn resident_micro() {
    let gpu = GpuCompositor::new().unwrap();
    let e = Extent::new(2048, 2048);
    let mk = |n: usize, mode: BlendMode| {
        let mut d = doc(e, Depth::U8);
        let base = add(
            &mut d,
            None,
            layer_fn("l", e, Depth::U8, |x, y| {
                [(x % 256) as f32 / 255.0, (y % 256) as f32 / 255.0, 0.5, 0.6]
            }),
        );
        for _ in 1..n {
            d.apply(DocOp::DuplicateLayer { id: base }).unwrap();
        }
        for id in d.state().layer_ids() {
            let mut p = d.state().find(id).unwrap().props.clone();
            p.blend_mode = mode;
            p.opacity = 0.7;
            d.apply(DocOp::SetProps { id, props: p }).unwrap();
        }
        d
    };
    let mut r = ResidentRenderer::new(&gpu).unwrap();
    let px = e.area() as f64;
    for n in [1usize, 10, 40] {
        for mode in [BlendMode::Normal, BlendMode::Overlay, BlendMode::Hue] {
            let d = mk(n, mode);
            let t = time(&mut r, &d);
            println!(
                "{n:>3} layers {mode:?}: {t:.2} ms  ({:.2} ns / layer-px)",
                t * 1e6 / (px * n as f64)
            );
        }
    }
    let mut d = mk(10, BlendMode::Normal);
    for a in [
        Adjustment::Invert,
        Adjustment::Curves {
            master: Curve(vec![[0.0, 0.1], [1.0, 0.9]]),
            rgb: Default::default(),
        },
        Adjustment::HueSaturation {
            hue: 10.0,
            saturation: 10.0,
            lightness: 0.0,
            colorize: false,
        },
    ] {
        add(
            &mut d,
            None,
            Layer::new("a", LayerKind::Adjustment(a.clone())),
        );
        let t = time(&mut r, &d);
        println!("10 layers + {a:?}: {t:.2} ms");
    }
    let mut d = mk(10, BlendMode::Normal);
    let g = add(&mut d, None, Layer::group("g", GroupMode::Isolated));
    add(
        &mut d,
        Some(g),
        layer_fn("x", e, Depth::U8, |_, _| [0.2, 0.3, 0.4, 0.5]),
    );
    println!("10 layers + isolated group: {:.2} ms", time(&mut r, &d));
}

#[test]
#[ignore = "bench"]
fn gpu_calibration() {
    use wgpu::util::DeviceExt;
    let gpu = GpuCompositor::new().unwrap();
    let (device, queue) = gpu.handles();
    let n = 2048u32 * 2048;
    let src = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: None,
        contents: bytemuck::cast_slice(
            &(0..65536u32)
                .map(|i| i.wrapping_mul(2654435761))
                .collect::<Vec<_>>(),
        ),
        usage: wgpu::BufferUsages::STORAGE,
    });
    let out = device.create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size: u64::from(n) * 16,
        usage: wgpu::BufferUsages::STORAGE,
        mapped_at_creation: false,
    });
    for (name, body) in [
        (
            "alu only",
            "let s = vec4<f32>(f32(i & 255u) / 255.0, 0.3, 0.2, 0.6);",
        ),
        (
            "load+unpack",
            "let s = unpack4x8unorm(src[(i + k * 7u) & 65535u]);",
        ),
    ] {
        for layers in [1u32, 40] {
            let wgsl = format!(
                "@group(0) @binding(0) var<storage, read> src: array<u32>;
@group(0) @binding(1) var<storage, read_write> outp: array<vec4<f32>>;
@compute @workgroup_size(256)
fn main(@builtin(global_invocation_id) g: vec3<u32>) {{
    let i = g.x + g.y * 65535u * 256u;
    if (i >= {n}u) {{ return; }}
    var cur = vec4<f32>(0.0);
    for (var k = 0u; k < {layers}u; k++) {{
        {body}
        let ab = cur.w;
        var cb = vec3<f32>(0.0);
        if (ab > 0.0) {{ cb = cur.xyz * (1.0 / ab); }}
        let a = s.w * 0.7;
        let u = a * (1.0 - ab); let v = a * ab; let w = 1.0 - a;
        cur = vec4<f32>(u * s.xyz + v * (cb * s.xyz) + w * cur.xyz, a + w * ab);
    }}
    if (i == 0xffffffffu) {{ cur.x += f32(src[0]); }}
    outp[i] = cur;
}}"
            );
            let m = device.create_shader_module(wgpu::ShaderModuleDescriptor {
                label: None,
                source: wgpu::ShaderSource::Wgsl(wgsl.into()),
            });
            let p = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                label: None,
                layout: None,
                module: &m,
                entry_point: Some("main"),
                compilation_options: Default::default(),
                cache: None,
            });
            let bg = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: None,
                layout: &p.get_bind_group_layout(0),
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: src.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: out.as_entire_binding(),
                    },
                ],
            });
            let mut v = Vec::new();
            for _ in 0..6 {
                let t = Instant::now();
                let mut e = device.create_command_encoder(&Default::default());
                {
                    let mut pass = e.begin_compute_pass(&Default::default());
                    pass.set_pipeline(&p);
                    pass.set_bind_group(0, &bg, &[]);
                    pass.dispatch_workgroups(n / 256, 1, 1);
                }
                queue.submit([e.finish()]);
                device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
                v.push(t.elapsed().as_secs_f64() * 1e3);
            }
            v.sort_by(f64::total_cmp);
            println!(
                "calib {name} {layers} layers: {:.2} ms ({:.3} ns/layer-px)",
                v[3],
                v[3] * 1e6 / (n as f64 * layers as f64)
            );
        }
    }
}
