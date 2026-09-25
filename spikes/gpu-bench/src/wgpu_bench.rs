//! wgpu 30 compute path. GPU time comes from TIMESTAMP_QUERY writes at the
//! beginning and end of the single compute pass that holds all dispatches of
//! a kernel (resolved with `resolve_query_set`, scaled by `get_timestamp_period`).

use crate::data::{Px, H, W};
use crate::Timing;
use std::time::Instant;
use wgpu::util::DeviceExt;

pub struct Ctx {
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
    pub adapter_info: String,
    ts_period_ns: f64,
    /// Compile with naga's default bounds-checked runtime checks (true) or trusted/unchecked.
    pub checked: bool,
}

pub struct Job {
    passes: Vec<(wgpu::ComputePipeline, wgpu::BindGroup)>,
    output: wgpu::Buffer,
    // Keep inputs alive for the duration of the job.
    _keep: Vec<wgpu::Buffer>,
}

impl Ctx {
    pub fn new(checked: bool) -> Result<Ctx, String> {
        let mut desc = wgpu::InstanceDescriptor::new_without_display_handle();
        desc.backends = wgpu::Backends::METAL;
        desc.flags = wgpu::InstanceFlags::empty();
        let instance = wgpu::Instance::new(desc);
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            ..Default::default()
        }))
        .map_err(|e| format!("request_adapter: {e}"))?;
        if !adapter.features().contains(wgpu::Features::TIMESTAMP_QUERY) {
            return Err("adapter lacks Features::TIMESTAMP_QUERY".into());
        }
        let info = adapter.get_info();
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("gpu-bench"),
            required_features: wgpu::Features::TIMESTAMP_QUERY,
            required_limits: adapter.limits(),
            ..Default::default()
        }))
        .map_err(|e| format!("request_device: {e}"))?;
        let ts_period_ns = queue.get_timestamp_period() as f64;
        Ok(Ctx {
            device,
            queue,
            adapter_info: format!("{} ({:?}, {:?})", info.name, info.backend, info.device_type),
            ts_period_ns,
            checked,
        })
    }

    fn module(&self, label: &str, body: &str) -> wgpu::ShaderModule {
        let src = format!(
            "{}\n{}",
            include_str!("../shaders/common.wgsl")
                .replace("__W__", &W.to_string())
                .replace("__H__", &H.to_string()),
            body
        );
        let desc = wgpu::ShaderModuleDescriptor {
            label: Some(label),
            source: wgpu::ShaderSource::Wgsl(src.into()),
        };
        if self.checked {
            self.device.create_shader_module(desc)
        } else {
            // SAFETY: the kernels only index in-bounds (every index is range-checked
            // against W/H or clamped); this measures the cost of naga's bounds checks.
            unsafe {
                self.device
                    .create_shader_module_trusted(desc, wgpu::ShaderRuntimeChecks::unchecked())
            }
        }
    }

    fn pipeline(&self, module: &wgpu::ShaderModule, entry: &str) -> wgpu::ComputePipeline {
        self.device
            .create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                label: Some(entry),
                layout: None,
                module,
                entry_point: Some(entry),
                compilation_options: Default::default(),
                cache: None,
            })
    }

    fn bind(&self, p: &wgpu::ComputePipeline, bufs: &[(u32, &wgpu::Buffer)]) -> wgpu::BindGroup {
        let entries: Vec<_> = bufs
            .iter()
            .map(|(b, buf)| wgpu::BindGroupEntry {
                binding: *b,
                resource: buf.as_entire_binding(),
            })
            .collect();
        self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None,
            layout: &p.get_bind_group_layout(0),
            entries: &entries,
        })
    }

    fn storage_init(&self, bytes: &[u8]) -> wgpu::Buffer {
        self.device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: None,
                contents: bytes,
                usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
            })
    }

    fn storage(&self) -> wgpu::Buffer {
        self.device.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size: (W * H * 16) as u64,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
            mapped_at_creation: false,
        })
    }

    pub fn demosaic_job(&self, cfa: &[u16]) -> Job {
        let m = self.module("demosaic", include_str!("../shaders/demosaic.wgsl"));
        let p = self.pipeline(&m, "main");
        let input = self.storage_init(bytemuck::cast_slice(cfa));
        let out = self.storage();
        let bg = self.bind(&p, &[(0, &input), (1, &out)]);
        Job {
            passes: vec![(p, bg)],
            output: out,
            _keep: vec![input],
        }
    }

    pub fn guided_job(&self, img: &[Px]) -> Job {
        let m = self.module("guided", include_str!("../shaders/guided.wgsl"));
        let i = self.storage_init(bytemuck::cast_slice(img));
        let (t1, t2, a, b) = (
            self.storage(),
            self.storage(),
            self.storage(),
            self.storage(),
        );
        let p1 = self.pipeline(&m, "hbox_in");
        let p2 = self.pipeline(&m, "vbox_coef");
        let p3 = self.pipeline(&m, "hbox_ab");
        let p4 = self.pipeline(&m, "vbox_out");
        let g1 = self.bind(&p1, &[(0, &i), (3, &t1), (4, &t2)]);
        let g2 = self.bind(&p2, &[(0, &t1), (1, &t2), (3, &a), (4, &b)]);
        let g3 = self.bind(&p3, &[(0, &a), (1, &b), (3, &t1), (4, &t2)]);
        // Output written into `b` (a and b are dead after pass 3).
        let g4 = self.bind(&p4, &[(0, &t1), (1, &t2), (2, &i), (3, &b)]);
        Job {
            passes: vec![(p1, g1), (p2, g2), (p3, g3), (p4, g4)],
            output: b,
            _keep: vec![i, t1, t2, a],
        }
    }

    pub fn lut_job(&self, img: &[Px], lut: &[Px]) -> Job {
        let m = self.module("lut", include_str!("../shaders/lut.wgsl"));
        let p = self.pipeline(&m, "main");
        let src = self.storage_init(bytemuck::cast_slice(img));
        let l = self.storage_init(bytemuck::cast_slice(lut));
        let out = self.storage();
        let bg = self.bind(&p, &[(0, &src), (1, &l), (2, &out)]);
        Job {
            passes: vec![(p, bg)],
            output: out,
            _keep: vec![src, l],
        }
    }

    /// Runs `warmup + runs` iterations; each submits the kernel, resolves the
    /// timestamps and reads back the full output. Returns medians over `runs`
    /// plus the last output.
    pub fn run(&self, job: &Job, warmup: usize, runs: usize) -> Result<(Timing, Vec<f32>), String> {
        let size = (W * H * 16) as u64;
        let qs = self.device.create_query_set(&wgpu::QuerySetDescriptor {
            label: Some("ts"),
            ty: wgpu::QueryType::Timestamp,
            count: 2,
        });
        let resolve = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("ts-resolve"),
            size: 16,
            usage: wgpu::BufferUsages::QUERY_RESOLVE | wgpu::BufferUsages::COPY_SRC,
            mapped_at_creation: false,
        });
        let ts_read = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("ts-read"),
            size: 16,
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let staging = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("staging"),
            size,
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let mut out = vec![0f32; W * H * 4];
        let (mut gpu, mut wall) = (Vec::new(), Vec::new());
        let scope = self.device.push_error_scope(wgpu::ErrorFilter::Validation);
        for it in 0..warmup + runs {
            let t0 = Instant::now();
            let mut enc = self.device.create_command_encoder(&Default::default());
            {
                let mut pass = enc.begin_compute_pass(&wgpu::ComputePassDescriptor {
                    label: None,
                    timestamp_writes: Some(wgpu::ComputePassTimestampWrites {
                        query_set: &qs,
                        beginning_of_pass_write_index: Some(0),
                        end_of_pass_write_index: Some(1),
                    }),
                });
                for (p, bg) in &job.passes {
                    pass.set_pipeline(p);
                    pass.set_bind_group(0, bg, &[]);
                    pass.dispatch_workgroups((W / 16) as u32, (H / 16) as u32, 1);
                }
            }
            enc.resolve_query_set(&qs, 0..2, &resolve, 0);
            enc.copy_buffer_to_buffer(&resolve, 0, &ts_read, 0, 16);
            enc.copy_buffer_to_buffer(&job.output, 0, &staging, 0, size);
            self.queue.submit([enc.finish()]);
            ts_read
                .slice(..)
                .map_async(wgpu::MapMode::Read, |r| r.expect("map ts"));
            staging
                .slice(..)
                .map_async(wgpu::MapMode::Read, |r| r.expect("map out"));
            self.device
                .poll(wgpu::PollType::wait_indefinitely())
                .map_err(|e| format!("poll: {e}"))?;
            let ts: [u64; 2] = {
                let v = ts_read
                    .slice(..)
                    .get_mapped_range()
                    .map_err(|e| format!("map ts: {e:?}"))?;
                let s: &[u64] = bytemuck::cast_slice(&v);
                [s[0], s[1]]
            };
            {
                let v = staging
                    .slice(..)
                    .get_mapped_range()
                    .map_err(|e| format!("map out: {e:?}"))?;
                out.copy_from_slice(bytemuck::cast_slice(&v));
            }
            ts_read.unmap();
            staging.unmap();
            let w = t0.elapsed().as_secs_f64() * 1e3;
            let g = ts[1].wrapping_sub(ts[0]) as f64 * self.ts_period_ns * 1e-6;
            if it >= warmup {
                gpu.push(g);
                wall.push(w);
            }
        }
        if let Some(e) = pollster::block_on(scope.pop()) {
            return Err(format!("wgpu validation error: {e}"));
        }
        Ok((Timing::from_samples(gpu, wall), out))
    }
}
