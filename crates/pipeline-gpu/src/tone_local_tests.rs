use super::*;

#[test]
fn presence_reuses_moments_and_skips_inactive_scale() {
    let ctx = crate::GpuContext::new().unwrap();
    let (pipeline, mean_pipeline) = pipelines(&ctx).unwrap();
    for (texture, clarity, dispatches) in [(0.8, 0., 15), (0., -0.75, 15), (0.8, -0.75, 21)] {
        let mut job = Job {
            ctx: &ctx,
            pipeline: pipeline.clone(),
            mean_pipeline: mean_pipeline.clone(),
            encoder: ctx.device.create_command_encoder(&Default::default()),
            p: [9., 17., 0., 0., texture, clarity, 0., 0., 0., 0., 0., 0.],
            bytes: 9 * 17 * 16,
            pending: Vec::new(),
        };
        let values: Vec<[f32; 4]> = (0..9 * 17)
            .map(|i| {
                let v = 0.3 + 0.02 * (i as f32 * 0.73).sin();
                [v, v * 0.8, v * 1.2, 0.]
            })
            .collect();
        let src = ctx
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: None,
                contents: bytemuck::cast_slice(&values),
                usage: wgpu::BufferUsages::STORAGE,
            });
        let result = job.presence(&src);
        assert_eq!(
            job.pending.len(),
            dispatches,
            "texture={texture} clarity={clarity}"
        );
        let actual = job.read(&result).unwrap();
        // Independently schedule the original three-scale graph. The optimized
        // graph must preserve its output, not merely the CPU tolerance.
        let z = job.pass(0, 0, &[&src]);
        let fine = job.guided(&z, &z, 1);
        let mid = job.guided(&z, &z, 3);
        let wide = if clarity != 0. {
            job.guided(&z, &z, 8)
        } else {
            mid.clone()
        };
        let reference = job.pass(6, 0, &[&src, &z, &fine, &mid, &wide]);
        assert_eq!(actual, job.read(&reference).unwrap());
    }
}

#[test]
fn in_place_percentiles_match_sort_for_repeated_selections() {
    for n in [1, 2, 7, 256, 4097] {
        let mut values: Vec<f32> = (0..n).map(|i| ((i * 73) % 101) as f32 - 50.).collect();
        values[0] = -0.0;
        let mut sorted = values.clone();
        sorted.sort_by(f32::total_cmp);
        for q in [0.90, 0.10, 0.99, 0.5, 0., 1.] {
            let rank = (((n - 1) as f32 * q).round() as usize).min(n - 1);
            assert_eq!(
                percentile_in_place(&mut values, q).to_bits(),
                sorted[rank].to_bits()
            );
        }
        values.sort_by(f32::total_cmp);
        assert_eq!(values, sorted, "selection must preserve the multiset");
    }
}

#[test]
fn dependent_dispatches_wait_for_one_readback_pass() {
    let ctx = crate::GpuContext::new().unwrap();
    let (pipeline, mean_pipeline) = pipelines(&ctx).unwrap();
    let mut job = Job {
        ctx: &ctx,
        pipeline,
        mean_pipeline,
        encoder: ctx.device.create_command_encoder(&Default::default()),
        p: [1., 1., 0., 0., 0., 0., 0., 0., 0., 0., 0., 0.],
        bytes: 16,
        pending: Vec::new(),
    };
    let src = ctx
        .device
        .create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: None,
            contents: bytemuck::cast_slice(&[0.25_f32; 4]),
            usage: wgpu::BufferUsages::STORAGE,
        });
    let first = job.mean(&src, 8);
    let second = job.mean(&first, 3);
    assert_eq!(job.pending.len(), 4, "no per-dispatch Metal passes");
    assert_eq!(job.read(&second).unwrap(), vec![[0.25; 4]]);
    assert!(job.pending.is_empty(), "readback must flush queued work");
    let third = job.mean(&second, 1);
    assert_eq!(job.pending.len(), 2);
    assert_eq!(job.read(&third).unwrap(), vec![[0.25; 4]]);
    assert!(job.pending.is_empty());
}

// Dispatch either the historical global-load kernel or cooperative kernel with
// identical input. Kept separate from Job::pass so the reference cannot silently
// start using the optimized path.
fn means(ctx: &crate::GpuContext, w: u32, h: u32, r: u32, shared: bool) -> Vec<[f32; 4]> {
    let (main, mean) = pipelines(ctx).unwrap();
    let values: Vec<[f32; 4]> = (0..w * h)
        .map(|i| {
            std::array::from_fn(|c| {
                let v = ((i * 73 + c as u32 * 19) % 997) as f32 / 613.0;
                if c == 1 { -v } else { v }
            })
        })
        .collect();
    let bytes = values.len() as u64 * 16;
    let mut job = Job {
        ctx,
        pipeline: main.clone(),
        mean_pipeline: mean.clone(),
        encoder: ctx.device.create_command_encoder(&Default::default()),
        p: [0.; 12],
        bytes,
        pending: Vec::new(),
    };
    let mut src = ctx
        .device
        .create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("mean test source"),
            contents: bytemuck::cast_slice(&values),
            usage: wgpu::BufferUsages::STORAGE,
        });
    let pipeline = if shared { &mean } else { &main };
    for mode in [2u32, 3] {
        let p = [
            w as f32,
            h as f32,
            mode as f32,
            r as f32,
            0.,
            0.,
            0.,
            0.,
            0.,
            0.,
            0.,
            0.,
        ];
        let params = ctx
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: None,
                contents: bytemuck::cast_slice(&p),
                usage: wgpu::BufferUsages::STORAGE,
            });
        let dst = ctx.device.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size: bytes,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
            mapped_at_creation: false,
        });
        let buffers = [&src, &src, &src, &src, &src, &dst, &params];
        let entries: Vec<_> = buffers
            .iter()
            .enumerate()
            .filter(|(i, _)| !shared || matches!(i, 0 | 5 | 6))
            .map(|(i, buffer)| wgpu::BindGroupEntry {
                binding: i as u32,
                resource: buffer.as_entire_binding(),
            })
            .collect();
        let group = ctx.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None,
            layout: &pipeline.get_bind_group_layout(0),
            entries: &entries,
        });
        {
            let mut pass = job.encoder.begin_compute_pass(&Default::default());
            pass.set_pipeline(pipeline);
            pass.set_bind_group(0, &group, &[]);
            let tile = if shared { 16 } else { 8 };
            pass.dispatch_workgroups(w.div_ceil(tile), h.div_ceil(tile), 1);
        }
        src = dst;
    }
    job.read(&src).unwrap()
}

#[test]
fn shared_means_match_ordered_global_reference() {
    let ctx = crate::GpuContext::new().unwrap();
    for (w, h) in [(1, 1), (1, 35), (35, 1), (7, 9), (8, 8), (9, 17), (65, 63)] {
        for r in [1, 3, 4, 8] {
            let expected = means(&ctx, w, h, r, false);
            let actual = means(&ctx, w, h, r, true);
            assert_eq!(
                actual, expected,
                "{w}x{h}, radius {r}: accumulation order changed"
            );
        }
    }
}

#[test]
#[ignore = "manual wall-clock benchmark; includes allocation, upload and readback"]
fn shared_means_benchmark() {
    let ctx = crate::GpuContext::new().unwrap();
    let w = 1024;
    let h = 768;
    let expected = means(&ctx, w, h, 8, false);
    assert_eq!(means(&ctx, w, h, 8, true), expected);
    let mut old = Vec::new();
    let mut new = Vec::new();
    for i in 0..12 {
        for shared in if i % 2 == 0 {
            [false, true]
        } else {
            [true, false]
        } {
            let start = std::time::Instant::now();
            std::hint::black_box(means(&ctx, w, h, 8, shared));
            if shared {
                new.push(start.elapsed());
            } else {
                old.push(start.elapsed());
            }
        }
    }
    old.sort();
    new.sort();
    eprintln!(
        "1024x768 r8 two-pass means wall median: global {:?}, shared {:?}",
        old[6], new[6]
    );
}
