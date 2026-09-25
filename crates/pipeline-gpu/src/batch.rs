use crate::{GpuContext, operator::parameters};
use engine_api::{
    EngineError, EngineResult,
    jobs::CancellationToken,
    stage::StageId,
    tile::{TILE_SIZE, Tile},
};
use image_core::{CpuStageOp, Op, StageOp};
use raw_decode::CfaLayout;
use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};
use wgpu::util::DeviceExt;

/// Transfer diagnostics, shared by clones of a backend (not by all contexts).
#[derive(Debug, Default, Clone, Copy)]
pub struct GpuStats {
    pub uploads: u64,
    pub readbacks: u64,
    pub submissions: u64,
}
#[derive(Default)]
struct Counters {
    uploads: AtomicU64,
    readbacks: AtomicU64,
    submissions: AtomicU64,
}

/// Metal M1 operators. X-Trans neighbourhood operators fall back to CPU;
/// remaining contiguous operations still run on GPU.
#[derive(Clone)]
pub struct GpuStageOp {
    context: Arc<GpuContext>,
    counters: Arc<Counters>,
}
impl GpuStageOp {
    pub fn new(context: Arc<GpuContext>) -> Self {
        Self {
            context,
            counters: Arc::default(),
        }
    }
    pub fn context(&self) -> &Arc<GpuContext> {
        &self.context
    }
    pub fn stats(&self) -> GpuStats {
        GpuStats {
            uploads: self.counters.uploads.load(Ordering::Relaxed),
            readbacks: self.counters.readbacks.load(Ordering::Relaxed),
            submissions: self.counters.submissions.load(Ordering::Relaxed),
        }
    }

    fn execute(
        &self,
        chain: &[(StageId, Op<'_>)],
        inputs: &[Tile],
        cancel: &CancellationToken,
    ) -> EngineResult<Vec<Tile>> {
        let ctx = &self.context;
        let mut encoder = ctx.device.create_command_encoder(&Default::default());
        let mut pending = Vec::with_capacity(inputs.len());
        for input in inputs {
            cancel.check()?;
            let mut src = ctx
                .device
                .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("tile upload"),
                    contents: bytemuck::cast_slice(input.samples::<f32>()?),
                    usage: wgpu::BufferUsages::STORAGE,
                });
            let mut layout = input.layout();
            for (index, (_, op)) in chain.iter().enumerate() {
                cancel.check()?;
                if matches!(op, Op::Display { .. }) && index + 1 != chain.len() {
                    return Err(EngineError::invalid(
                        "GPU chain",
                        "display must be last (U8 output)",
                    ));
                }
                let (p, next_layout) =
                    parameters(op, layout, input.coord().pixel_origin(TILE_SIZE))?;
                layout = next_layout;
                let params = ctx
                    .device
                    .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                        label: None,
                        contents: bytemuck::cast_slice(&p),
                        usage: wgpu::BufferUsages::STORAGE,
                    });
                let size = (layout.plane_len() * layout.channels as usize * 4) as u64;
                let dst = ctx.device.create_buffer(&wgpu::BufferDescriptor {
                    label: None,
                    size,
                    usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
                    mapped_at_creation: false,
                });
                let entries: Vec<_> = [&src, &dst, &params]
                    .iter()
                    .enumerate()
                    .map(|(i, b)| wgpu::BindGroupEntry {
                        binding: i as u32,
                        resource: b.as_entire_binding(),
                    })
                    .collect();
                let group = ctx.device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: None,
                    layout: &ctx.pipeline.get_bind_group_layout(0),
                    entries: &entries,
                });
                {
                    let mut pass = encoder.begin_compute_pass(&Default::default());
                    pass.set_pipeline(&ctx.pipeline);
                    pass.set_bind_group(0, &group, &[]);
                    pass.dispatch_workgroups((layout.plane_len() as u32).div_ceil(64), 1, 1);
                }
                // wgpu's encoder retains the resources, even when host handles drop.
                src = dst;
            }
            let size = (layout.plane_len() * layout.channels as usize * 4) as u64;
            let staging = ctx.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("chain readback"),
                size,
                usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            encoder.copy_buffer_to_buffer(&src, 0, &staging, 0, size);
            pending.push((input.coord(), layout, staging));
        }
        cancel.check()?;
        ctx.queue.submit([encoder.finish()]);
        self.counters.submissions.fetch_add(1, Ordering::Relaxed);
        self.counters
            .uploads
            .fetch_add(inputs.len() as u64, Ordering::Relaxed);
        let mut receivers = Vec::new();
        for (_, _, buffer) in &pending {
            let (tx, rx) = std::sync::mpsc::channel();
            buffer.slice(..).map_async(wgpu::MapMode::Read, move |r| {
                let _ = tx.send(r);
            });
            receivers.push(rx);
        }
        ctx.device
            .poll(wgpu::PollType::wait_indefinitely())
            .map_err(internal)?;
        let mut output = Vec::with_capacity(inputs.len());
        for ((coord, layout, buffer), rx) in pending.into_iter().zip(receivers) {
            rx.recv().map_err(internal)?.map_err(internal)?;
            let data: Vec<f32> = {
                let mapped = buffer.slice(..).get_mapped_range().map_err(internal)?;
                bytemuck::cast_slice(&mapped).to_vec()
            };
            buffer.unmap();
            self.counters.readbacks.fetch_add(1, Ordering::Relaxed);
            cancel.check()?;
            let tile = if matches!(chain.last(), Some((_, Op::Display { .. }))) {
                Tile::from_samples(coord, layout, data.into_iter().map(|v| v as u8).collect())?
            } else {
                Tile::from_samples(coord, layout, data)?
            };
            output.push(tile);
        }
        Ok(output)
    }
}
fn internal(e: impl std::fmt::Display) -> EngineError {
    EngineError::internal(e.to_string())
}
fn cpu_fallback(op: &Op<'_>) -> bool {
    matches!(
        op,
        Op::Highlights {
            cfa: CfaLayout::XTrans(_),
            ..
        } | Op::Demosaic {
            cfa: CfaLayout::XTrans(_),
            ..
        }
    )
}
impl StageOp for GpuStageOp {
    fn run(&self, stage: StageId, op: &Op<'_>, input: Tile) -> EngineResult<Tile> {
        self.run_chain_batch(&[(stage, *op)], vec![input], &CancellationToken::new())?
            .pop()
            .ok_or_else(|| EngineError::internal("missing GPU result"))
    }
    fn batch_size(&self) -> usize {
        16
    }
    fn run_chain_batch(
        &self,
        chain: &[(StageId, Op<'_>)],
        mut inputs: Vec<Tile>,
        cancel: &CancellationToken,
    ) -> EngineResult<Vec<Tile>> {
        cancel.check()?;
        if chain.is_empty() || inputs.is_empty() {
            return Ok(inputs);
        }
        // Split only at genuine CPU fallback boundaries, not between GPU stages.
        let mut start = 0;
        while start < chain.len() {
            if cpu_fallback(&chain[start].1) {
                inputs = CpuStageOp.run_chain_batch(&chain[start..start + 1], inputs, cancel)?;
                start += 1;
            } else {
                let end = (start..chain.len())
                    .find(|&i| cpu_fallback(&chain[i].1))
                    .unwrap_or(chain.len());
                let mut output = Vec::with_capacity(inputs.len());
                for batch in inputs.chunks(self.batch_size()) {
                    output.extend(self.execute(&chain[start..end], batch, cancel)?);
                }
                inputs = output;
                start = end;
            }
        }
        Ok(inputs)
    }
}
