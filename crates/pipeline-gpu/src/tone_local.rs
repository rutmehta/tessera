//! Isolated GPU Texture/Clarity/Dehaze. Curves are intentionally not applied.
//! Host work is restricted to packing and exact global percentile/airlight
//! reduction. Dark-channel morphology and all guided filtering run on GPU.
use engine_api::{EngineError, EngineResult, recipe::settings::ToneSettings};
use pipeline_cpu::Image;
use wgpu::util::DeviceExt;

#[cfg(test)]
#[path = "tone_local_tests.rs"]
mod tests;

pub(crate) fn run(ctx: &crate::GpuContext, input: &Image, s: &ToneSettings) -> EngineResult<Image> {
    if input.planes().len() != 3
        || [s.texture, s.clarity, s.dehaze]
            .iter()
            .any(|v| !v.is_finite())
    {
        return Err(EngineError::invalid(
            "presence",
            "finite settings and RGB image required",
        ));
    }
    if s.texture == 0.0 && s.clarity == 0.0 && s.dehaze == 0.0 {
        return Ok(input.clone());
    }
    let n = input.width() as usize * input.height() as usize;
    let bytes = n as u64 * 16;
    let limits = ctx.device.limits();
    if bytes > limits.max_storage_buffer_binding_size
        || bytes > limits.max_buffer_size
        || input.width().div_ceil(8) > limits.max_compute_workgroups_per_dimension
        || input.height().div_ceil(8) > limits.max_compute_workgroups_per_dimension
    {
        return Err(EngineError::invalid(
            "local tone image",
            "exceeds GPU buffer/dispatch limits",
        ));
    }
    let (pipeline, mean_pipeline) = pipelines(ctx)?;
    run_with_pipelines(ctx, input, s, n, bytes, pipeline, mean_pipeline)
}

// Context ownership isolates devices without global IDs, unsafe HAL pointers,
// or retaining otherwise-unused devices. Only cold compilation holds the lock.
pub(crate) fn pipelines(
    ctx: &crate::GpuContext,
) -> EngineResult<(wgpu::ComputePipeline, wgpu::ComputePipeline)> {
    let mut cache = ctx
        .local_tone_pipelines
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    if let Some(pipelines) = &*cache {
        return Ok(pipelines.clone());
    }
    let scope = ctx.device.push_error_scope(wgpu::ErrorFilter::Validation);
    let module = ctx
        .device
        .create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("local tone"),
            source: wgpu::ShaderSource::Wgsl(include_str!("tone_local.wgsl").into()),
        });
    let pipeline = ctx
        .device
        .create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("local tone"),
            layout: None,
            module: &module,
            entry_point: Some("main"),
            compilation_options: Default::default(),
            cache: None,
        });
    let mean_pipeline = ctx
        .device
        .create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("local tone shared means"),
            layout: None,
            module: &module,
            entry_point: Some("mean"),
            compilation_options: Default::default(),
            cache: None,
        });
    if let Some(e) = pollster::block_on(scope.pop()) {
        return Err(internal(e));
    }
    *cache = Some((pipeline.clone(), mean_pipeline.clone()));
    Ok((pipeline, mean_pipeline))
}

fn run_with_pipelines(
    ctx: &crate::GpuContext,
    input: &Image,
    s: &ToneSettings,
    n: usize,
    bytes: u64,
    pipeline: wgpu::ComputePipeline,
    mean_pipeline: wgpu::ComputePipeline,
) -> EngineResult<Image> {
    let mut job = Job {
        ctx,
        pipeline,
        mean_pipeline,
        encoder: ctx.device.create_command_encoder(&Default::default()),
        p: [0.; 12],
        bytes,
        pending: Vec::new(),
    };
    job.p[0] = input.width() as f32;
    job.p[1] = input.height() as f32;
    job.p[4] = s.texture.clamp(-100., 100.) / 100.;
    job.p[5] = s.clarity.clamp(-100., 100.) / 100.;
    let packed: Vec<[f32; 4]> = (0..n)
        .map(|i| {
            [
                input.planes()[0][i],
                input.planes()[1][i],
                input.planes()[2][i],
                0.,
            ]
        })
        .collect();
    let mut rgb = ctx
        .device
        .create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("local tone RGB"),
            contents: bytemuck::cast_slice(&packed),
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
        });
    if s.texture != 0. || s.clarity != 0. {
        rgb = job.presence(&rgb);
    }
    if s.dehaze != 0. {
        let stats = job.pass(7, 3, &[&rgb]);
        let stats = job.read(&stats)?;
        let pixels = job.read(&rgb)?;
        if let Some((air, confidence)) = airlight(&stats, &pixels) {
            job.p[6..9].copy_from_slice(&air);
            job.p[9] = s.dehaze.abs().min(100.) / 100. * confidence;
            job.p[10] = s.dehaze;
            let transmission = job.pass(8, 3, &[&rgb]);
            let guide = job.pass(0, 0, &[&rgb]);
            let transmission = job.guided(&guide, &transmission, 4);
            rgb = job.pass(9, 0, &[&rgb, &transmission]);
        }
    }
    let pixels = job.read(&rgb)?;
    Image::new(
        input.width(),
        input.height(),
        (0..3)
            .map(|c| pixels.iter().map(|v| v[c]).collect())
            .collect(),
    )
}
fn internal(e: impl std::fmt::Display) -> EngineError {
    EngineError::internal(e.to_string())
}
struct Job<'a> {
    ctx: &'a crate::GpuContext,
    pipeline: wgpu::ComputePipeline,
    mean_pipeline: wgpu::ComputePipeline,
    encoder: wgpu::CommandEncoder,
    p: [f32; 12],
    bytes: u64,
    // Metal opens a command buffer per compute pass. Keep dependent dispatches
    // ordered inside one pass until a host reduction actually needs the data.
    pending: Vec<(wgpu::ComputePipeline, wgpu::BindGroup, [u32; 2])>,
}
impl Job<'_> {
    fn presence(&mut self, rgb: &wgpu::Buffer) -> wgpu::Buffer {
        let z = self.pass(0, 0, &[rgb]);
        // Raw moments do not depend on radius. Share their buffer across all
        // scales, without changing the ordered box sums or CPU oracle math.
        let moments = self.pass(1, 0, &[&z, &z]);
        let mid = self.guided_from_moments(&z, &moments, 3);
        let fine = if self.p[4] != 0. {
            self.guided_from_moments(&z, &moments, 1)
        } else {
            // The texture coefficient is zero. Bind mid for both operands of
            // that term rather than computing an unused fine-scale filter.
            mid.clone()
        };
        let wide = if self.p[5] != 0. {
            self.guided_from_moments(&z, &moments, 8)
        } else {
            mid.clone()
        };
        self.pass(6, 0, &[rgb, &z, &fine, &mid, &wide])
    }

    fn pass(&mut self, mode: u32, radius: u32, inputs: &[&wgpu::Buffer]) -> wgpu::Buffer {
        let is_mean = mode == 2 || mode == 3;
        assert!(!is_mean || radius <= 8, "shared mean radius exceeds halo");
        let pipeline = if is_mean {
            &self.mean_pipeline
        } else {
            &self.pipeline
        };
        self.p[2] = mode as f32;
        self.p[3] = radius as f32;
        let params = self
            .ctx
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("local tone parameters"),
                contents: bytemuck::cast_slice(&self.p),
                usage: wgpu::BufferUsages::STORAGE,
            });
        let dst = self.ctx.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("local tone intermediate"),
            size: self.bytes,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
            mapped_at_creation: false,
        });
        let buffers = [
            inputs[0],
            *inputs.get(1).unwrap_or(&inputs[0]),
            *inputs.get(2).unwrap_or(&inputs[0]),
            *inputs.get(3).unwrap_or(&inputs[0]),
            *inputs.get(4).unwrap_or(&inputs[0]),
            &dst,
            &params,
        ];
        let entries: Vec<_> = buffers
            .iter()
            .enumerate()
            .filter(|(i, _)| !is_mean || matches!(i, 0 | 5 | 6))
            .map(|(i, b)| wgpu::BindGroupEntry {
                binding: i as u32,
                resource: b.as_entire_binding(),
            })
            .collect();
        let group = self
            .ctx
            .device
            .create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("local tone"),
                layout: &pipeline.get_bind_group_layout(0),
                entries: &entries,
            });
        let tile = if is_mean { 16 } else { 8 };
        self.pending.push((
            pipeline.clone(),
            group,
            [
                (self.p[0] as u32).div_ceil(tile),
                (self.p[1] as u32).div_ceil(tile),
            ],
        ));
        dst
    }
    fn mean(&mut self, input: &wgpu::Buffer, r: u32) -> wgpu::Buffer {
        let horizontal = self.pass(2, r, &[input]);
        self.pass(3, r, &[&horizontal])
    }
    fn guided(&mut self, guide: &wgpu::Buffer, input: &wgpu::Buffer, r: u32) -> wgpu::Buffer {
        let moments = self.pass(1, r, &[guide, input]);
        self.guided_from_moments(guide, &moments, r)
    }
    fn guided_from_moments(
        &mut self,
        guide: &wgpu::Buffer,
        moments: &wgpu::Buffer,
        r: u32,
    ) -> wgpu::Buffer {
        let means = self.mean(moments, r);
        let coefficients = self.pass(4, r, &[&means]);
        let means = self.mean(&coefficients, r);
        self.pass(5, r, &[&means, guide])
    }
    fn read(&mut self, input: &wgpu::Buffer) -> EngineResult<Vec<[f32; 4]>> {
        if !self.pending.is_empty() {
            let mut pass = self.encoder.begin_compute_pass(&Default::default());
            for (pipeline, group, groups) in &self.pending {
                pass.set_pipeline(pipeline);
                pass.set_bind_group(0, group, &[]);
                pass.dispatch_workgroups(groups[0], groups[1], 1);
            }
            drop(pass);
            self.pending.clear();
        }
        let staging = self.ctx.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("local tone readback"),
            size: self.bytes,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        self.encoder
            .copy_buffer_to_buffer(input, 0, &staging, 0, self.bytes);
        let encoder = std::mem::replace(
            &mut self.encoder,
            self.ctx.device.create_command_encoder(&Default::default()),
        );
        self.ctx.queue.submit([encoder.finish()]);
        let (tx, rx) = std::sync::mpsc::channel();
        staging.slice(..).map_async(wgpu::MapMode::Read, move |r| {
            let _ = tx.send(r);
        });
        self.ctx
            .device
            .poll(wgpu::PollType::wait_indefinitely())
            .map_err(internal)?;
        rx.recv().map_err(internal)?.map_err(internal)?;
        let pixels = {
            let mapped = staging.slice(..).get_mapped_range().map_err(internal)?;
            bytemuck::cast_slice(&mapped).to_vec()
        };
        staging.unmap();
        Ok(pixels)
    }
}
fn percentile(mut values: Vec<f32>, q: f32) -> f32 {
    percentile_in_place(&mut values, q)
}
fn percentile_in_place(values: &mut [f32], q: f32) -> f32 {
    // Exact order statistic, not a histogram approximation. Linear-time
    // selection avoids sorting millions of samples for every dehaze edit.
    let rank = (((values.len() - 1) as f32 * q).round() as usize).min(values.len() - 1);
    *values.select_nth_unstable_by(rank, f32::total_cmp).1
}
// Exact reference quantiles. This is global reduction, not CPU image filtering.
fn airlight(stats: &[[f32; 4]], rgb: &[[f32; 4]]) -> Option<([f32; 3], f32)> {
    let mut ys: Vec<_> = stats.iter().map(|v| v[0]).collect();
    let threshold = percentile(stats.iter().map(|v| v[1]).collect(), 0.90);
    let ceiling = percentile_in_place(&mut ys, 0.99);
    let candidates: Vec<_> = stats
        .iter()
        .enumerate()
        .filter(|(_, v)| v[1] >= threshold && v[0] <= ceiling)
        .map(|(i, _)| i)
        .collect();
    if candidates.is_empty() {
        return None;
    }
    let mut air = std::array::from_fn::<_, 3, _>(|c| {
        percentile(candidates.iter().map(|&i| rgb[i][c].max(0.)).collect(), 0.5)
    });
    let ay = (0.2627 * air[0] + 0.678 * air[1] + 0.0593 * air[2]).clamp(-f32::MAX, f32::MAX);
    if ay < 1e-8 {
        return None;
    }
    air = air.map(|v| v.clamp(0.75 * ay, (1.25 * ay).min(f32::MAX)).max(1e-8));
    let spread = percentile_in_place(&mut ys, 0.90) - percentile_in_place(&mut ys, 0.10);
    let t = ((spread / ay - 0.05) / 0.20).clamp(0., 1.);
    let confidence = t * t * (3. - 2. * t);
    (confidence != 0.).then_some((air, confidence))
}
