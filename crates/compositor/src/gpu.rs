//! WGSL port of the tile program (blend modes, Blend If, groups, clipping,
//! knockout). Adjustment layers are not ported: programs containing them
//! return [`EngineError::Unsupported`] and callers use the CPU reference.
//! Gated against the CPU reference at ≤ 1e-4 (docs/11 §1.3).

use engine_api::tile::{Tile, TileCoord};
use engine_api::{EngineError, EngineResult};
use wgpu::util::DeviceExt;

use crate::document::Knockout;
use crate::edit::Document;
use crate::render::exec::{FrameKind, Op};
use crate::render::{Compositor, DocRef, unpremultiply};

/// Maximum group nesting the shader's per-pixel stack holds.
pub const MAX_DEPTH: usize = 8;

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct GpuOp {
    kind: u32,
    mode: u32,
    src: u32,
    mask: u32,
    flags: u32,
    opacity: f32,
    fill: f32,
    seed: u32,
    bi: [[f32; 4]; 8],
}

const NO_MASK: u32 = u32::MAX;

#[path = "gpu_resident.rs"]
mod resident;
pub use resident::ResidentComposite;

/// A Metal device with the compositor pipeline.
pub struct GpuCompositor {
    device: wgpu::Device,
    queue: wgpu::Queue,
    pipeline: wgpu::ComputePipeline,
    /// Adapter description.
    pub adapter: String,
}

fn internal(e: impl std::fmt::Display) -> EngineError {
    EngineError::Gpu {
        message: e.to_string(),
    }
}

impl GpuCompositor {
    /// Creates the device and compiles the shader. Fails without Metal.
    pub fn new() -> EngineResult<Self> {
        let mut desc = wgpu::InstanceDescriptor::new_without_display_handle();
        desc.backends = wgpu::Backends::METAL;
        let instance = wgpu::Instance::new(desc);
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            ..Default::default()
        }))
        .map_err(internal)?;
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("tessera compositor"),
            required_limits: wgpu::Limits {
                max_storage_buffer_binding_size: adapter.limits().max_storage_buffer_binding_size,
                max_buffer_size: adapter.limits().max_buffer_size,
                ..Default::default()
            },
            ..Default::default()
        }))
        .map_err(internal)?;
        Self::from_device(device, queue, adapter.get_info().name)
    }

    /// Compile on a caller-owned shared device/queue pair.
    pub fn from_device(
        device: wgpu::Device,
        queue: wgpu::Queue,
        adapter: String,
    ) -> EngineResult<Self> {
        let scope = device.push_error_scope(wgpu::ErrorFilter::Validation);
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("composite"),
            source: wgpu::ShaderSource::Wgsl(include_str!("composite.wgsl").into()),
        });
        let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("composite"),
            layout: None,
            module: &module,
            entry_point: Some("main"),
            compilation_options: Default::default(),
            cache: None,
        });
        if let Some(e) = pollster::block_on(scope.pop()) {
            return Err(internal(e));
        }
        Ok(Self {
            device,
            queue,
            pipeline,
            adapter,
        })
    }

    /// The composite at `coord` (premultiplied f32 RGBA), computed on the
    /// GPU. Sources (layer mips, masks, smart objects, cached groups) are
    /// resolved on the CPU through `comp`'s caches; the blend/group program
    /// runs per pixel in WGSL.
    pub fn render_tile_premultiplied(
        &self,
        comp: &Compositor,
        doc: &Document,
        coord: TileCoord,
    ) -> EngineResult<Tile> {
        let dref = DocRef {
            state: doc.state(),
            key: doc.key(),
        };
        let mut job = comp.job(dref, coord)?;
        job.full = false; // never publish group caches from here
        let ops = job.compile()?;
        let n = job.n;
        let mut srcs: Vec<f32> = Vec::new();
        let mut gops: Vec<GpuOp> = Vec::new();
        let mut buf = vec![0.0f32; 4 * n];
        let mut mask = vec![0.0f32; n];
        let mut depth = 0usize;
        let offset = |len: usize| -> EngineResult<u32> {
            u32::try_from(len).map_err(|_| EngineError::ResourceExhausted {
                resource: "gpu source buffer".into(),
            })
        };
        for op in &ops {
            let mut g = GpuOp {
                kind: 0,
                mode: 0,
                src: 0,
                mask: NO_MASK,
                flags: 0,
                opacity: 1.0,
                fill: 1.0,
                seed: 0,
                bi: [[0.0, 0.0, 1.0, 1.0]; 8],
            };
            let set = |g: &mut GpuOp, p: &crate::render::pixel::Params| {
                g.mode = p.mode.index();
                g.opacity = p.opacity;
                g.fill = p.fill;
                g.seed = p.seed;
                g.flags = u32::from(p.atop)
                    | match p.knockout {
                        Knockout::None => 0,
                        Knockout::Shallow => 2,
                        Knockout::Deep => 4,
                    };
                if let Some(bi) = &p.blend_if {
                    g.flags |= 8;
                    g.bi = bi.packed();
                }
            };
            match op {
                Op::Blend {
                    layer,
                    src,
                    params,
                    mask: use_mask,
                } => {
                    if !job.load_src(src, &mut buf)? {
                        continue;
                    }
                    if *use_mask && job.load_mask(layer, &mut mask)? {
                        for i in 0..n {
                            buf[3 * n + i] *= mask[i];
                        }
                    }
                    set(&mut g, params);
                    g.src = offset(srcs.len())?;
                    srcs.extend_from_slice(&buf);
                }
                Op::Adjust { .. } => {
                    return Err(EngineError::Unsupported {
                        what: "adjustment layers on the GPU compositor".into(),
                    });
                }
                Op::Push(kind) => {
                    depth += 1;
                    if depth >= MAX_DEPTH {
                        return Err(EngineError::Unsupported {
                            what: format!("group nesting deeper than {}", MAX_DEPTH - 1),
                        });
                    }
                    g.kind = if *kind == FrameKind::PassThrough {
                        3
                    } else {
                        2
                    };
                }
                Op::Pop {
                    layer,
                    params,
                    mask: use_mask,
                    pass,
                    ..
                } => {
                    depth = depth.saturating_sub(1);
                    set(&mut g, params);
                    g.kind = if *pass { 5 } else { 4 };
                    if *use_mask && job.load_mask(layer, &mut mask)? {
                        g.mask = offset(srcs.len())?;
                        srcs.extend_from_slice(&mask);
                    }
                }
                Op::SnapshotBackground => g.kind = 6,
            }
            gops.push(g);
        }
        if srcs.is_empty() {
            srcs.push(0.0);
        }
        if gops.is_empty() {
            gops.push(GpuOp {
                kind: 99,
                mode: 0,
                src: 0,
                mask: NO_MASK,
                flags: 0,
                opacity: 0.0,
                fill: 0.0,
                seed: 0,
                bi: [[0.0; 4]; 8],
            });
        }
        let header: [u32; 8] = [
            n as u32,
            job.w as u32,
            job.origin.0,
            job.origin.1,
            gops.len() as u32,
            0,
            0,
            0,
        ];
        let out = self.dispatch(&header, &gops, &srcs, 4 * n)?;
        Tile::from_samples(coord, job.layout(), out)
    }

    /// Straight-alpha variant of [`render_tile_premultiplied`](Self::render_tile_premultiplied).
    pub fn render_tile(
        &self,
        comp: &Compositor,
        doc: &Document,
        coord: TileCoord,
    ) -> EngineResult<Tile> {
        unpremultiply(&self.render_tile_premultiplied(comp, doc, coord)?)
    }

    fn dispatch(
        &self,
        header: &[u32; 8],
        ops: &[GpuOp],
        srcs: &[f32],
        out_len: usize,
    ) -> EngineResult<Vec<f32>> {
        let d = &self.device;
        let scope = d.push_error_scope(wgpu::ErrorFilter::Validation);
        let init = |label: &str, bytes: &[u8]| {
            d.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some(label),
                contents: bytes,
                usage: wgpu::BufferUsages::STORAGE,
            })
        };
        let hb = init("header", bytemuck::cast_slice(header));
        let ob = init("ops", bytemuck::cast_slice(ops));
        let sb = init("srcs", bytemuck::cast_slice(srcs));
        let size = (out_len * 4) as u64;
        let dst = d.create_buffer(&wgpu::BufferDescriptor {
            label: Some("out"),
            size,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
            mapped_at_creation: false,
        });
        let staging = d.create_buffer(&wgpu::BufferDescriptor {
            label: Some("readback"),
            size,
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let entries: Vec<_> = [&hb, &ob, &sb, &dst]
            .iter()
            .enumerate()
            .map(|(i, b)| wgpu::BindGroupEntry {
                binding: i as u32,
                resource: b.as_entire_binding(),
            })
            .collect();
        let group = d.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None,
            layout: &self.pipeline.get_bind_group_layout(0),
            entries: &entries,
        });
        let mut enc = d.create_command_encoder(&Default::default());
        {
            let mut pass = enc.begin_compute_pass(&Default::default());
            pass.set_pipeline(&self.pipeline);
            pass.set_bind_group(0, &group, &[]);
            pass.dispatch_workgroups((header[0]).div_ceil(64), 1, 1);
        }
        enc.copy_buffer_to_buffer(&dst, 0, &staging, 0, size);
        self.queue.submit([enc.finish()]);
        let (tx, rx) = std::sync::mpsc::channel();
        staging.slice(..).map_async(wgpu::MapMode::Read, move |r| {
            let _ = tx.send(r);
        });
        d.poll(wgpu::PollType::wait_indefinitely())
            .map_err(internal)?;
        rx.recv().map_err(internal)?.map_err(internal)?;
        if let Some(e) = pollster::block_on(scope.pop()) {
            return Err(internal(e));
        }
        let data = {
            let mapped = staging.slice(..).get_mapped_range().map_err(internal)?;
            bytemuck::cast_slice::<u8, f32>(&mapped).to_vec()
        };
        staging.unmap();
        Ok(data)
    }
}
