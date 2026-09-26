//! IEEE-conformant compute pipelines: the GPU rounds like the CPU.
use gpu_core::{GpuDevice, Precision, precise_compute_pipeline};

const SHADER: &str = r#"
@group(0) @binding(0) var<storage, read> a: array<f32>;
@group(0) @binding(1) var<storage, read> b: array<f32>;
@group(0) @binding(2) var<storage, read_write> o: array<f32>;
var<workgroup> scratch: array<f32, 64>;

fn pdiv(a: f32, b: f32) -> f32 {
    return a / b;
}

fn pdiv3(a: vec3<f32>, b: vec3<f32>) -> vec3<f32> {
    return a / b;
}

@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) id: vec3<u32>, @builtin(local_invocation_index) l: u32) {
    let i = id.x;
    scratch[l] = b[i];
    workgroupBarrier();
    let x = a[i];
    let y = scratch[l];
    // Division (scalar, vector reciprocal-multiply), an uncontracted
    // product sum and sqrt.
    o[4u * i] = pdiv(x, y);
    o[4u * i + 1u] = x * pdiv3(vec3<f32>(1.0), vec3<f32>(y)).y;
    o[4u * i + 2u] = x * y + y * 0.3;
    o[4u * i + 3u] = sqrt(x);
}
"#;

fn bytes(v: &[f32]) -> Vec<u8> {
    v.iter().flat_map(|x| x.to_le_bytes()).collect()
}

#[test]
fn precise_pipeline_rounds_like_the_cpu() {
    let Ok(g) = GpuDevice::new() else {
        eprintln!("skipping: no Metal device");
        return;
    };
    let entry = |binding, read_only| wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::COMPUTE,
        ty: wgpu::BindingType::Buffer {
            ty: wgpu::BufferBindingType::Storage { read_only },
            has_dynamic_offset: false,
            min_binding_size: None,
        },
        count: None,
    };
    let p = precise_compute_pipeline(
        &g.device,
        "precise test",
        SHADER,
        "main",
        (64, 1, 1),
        &[entry(0, true), entry(1, true), entry(2, false)],
    )
    .unwrap();
    assert_eq!(
        p.precision == Precision::Ieee,
        g.capabilities.passthrough_shaders
    );
    let n = 1 << 21;
    let mut s = 0x1234_5678u32;
    let mut rnd = || {
        s ^= s << 13;
        s ^= s >> 17;
        s ^= s << 5;
        (s >> 8) as f32 / 16_777_216.0
    };
    // Every pair of 8-bit values (i/255 over max(j, 1)/255), then random
    // pairs over [0, 1) × [0.001, 1) and wide exponent ranges.
    let (mut a, mut b) = (Vec::with_capacity(n), Vec::with_capacity(n));
    for i in 0..256u32 {
        for j in 0..256u32 {
            a.push(i as f32 / 255.0);
            b.push(j.max(1) as f32 / 255.0);
        }
    }
    while a.len() < n {
        let wide = a.len() % 2 == 0;
        let (x, y) = (rnd(), rnd());
        if wide {
            a.push(x * 2f32.powi((rnd() * 40.0) as i32 - 20));
            b.push((y + 0.5) * 2f32.powi((rnd() * 40.0) as i32 - 20));
        } else {
            a.push(x);
            b.push(y * 0.999 + 0.001);
        }
    }
    use wgpu::util::DeviceExt;
    let init = |v: &[f32]| {
        g.device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: None,
                contents: &bytes(v),
                usage: wgpu::BufferUsages::STORAGE,
            })
    };
    let (ba, bb) = (init(&a), init(&b));
    let out = g.device.create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size: (n * 16) as u64,
        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
        mapped_at_creation: false,
    });
    let group = g.device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: None,
        layout: &p.layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: ba.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: bb.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 2,
                resource: out.as_entire_binding(),
            },
        ],
    });
    let mut enc = g.device.create_command_encoder(&Default::default());
    {
        let mut pass = enc.begin_compute_pass(&Default::default());
        pass.set_pipeline(&p.pipeline);
        pass.set_bind_group(0, &group, &[]);
        pass.dispatch_workgroups((n / 64) as u32, 1, 1);
    }
    g.queue.submit([enc.finish()]);
    let got = gpu_core::read_buffer(&g.device, &g.queue, &out, 0, (n * 16) as u64).unwrap();
    let got: Vec<f32> = got
        .as_chunks::<4>()
        .0
        .iter()
        .map(|c| f32::from_le_bytes(*c))
        .collect();
    let mut diff = [0usize; 4];
    for i in 0..n {
        let want = [
            a[i] / b[i],
            a[i] * (1.0 / b[i]),
            a[i] * b[i] + b[i] * 0.3,
            a[i].sqrt(),
        ];
        for c in 0..4 {
            if want[c].to_bits() != got[4 * i + c].to_bits() {
                diff[c] += 1;
            }
        }
    }
    println!(
        "{:?}: mismatches (div, rcp-mul, product sum, sqrt) = {diff:?} of {n}",
        p.precision
    );
    if p.precision == Precision::Ieee {
        assert_eq!(diff, [0; 4]);
    }
}
