//! GPU smart-object sampling into the resident pool's straight planar pages.
//!
//! Geometry follows `Compositor::smart_tile` in f64 on the host: WGSL has
//! no portable f64, and rounding an inverse matrix to f32 moves sample
//! footprints at large coordinates. Only indices/weights are uploaded; no
//! child pixels are downloaded or resampled on the CPU. Cache plans by
//! transform, child canvas, parent canvas and tile coordinate.
//!
//! The caller owns child-renderer and page lifetimes. Render the child on
//! the same queue before submitting the encoder containing `encode`, and
//! pass its resident interleaved premultiplied level buffer directly.
use engine_api::tile::{Extent, TileCoord};
use engine_api::{EngineError, EngineResult};
use wgpu::util::DeviceExt;

use crate::geom::{Affine, Rect};
use crate::render::MAX_LEVEL;

const MISSING: u32 = u32::MAX;

/// Smart-object reconstruction policy. Existing RGBA8 output remains unchanged
/// unless the caller explicitly selects the quality path.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum SmartQuality {
    /// CPU-compatible bilinear reconstruction at every output level.
    #[default]
    LegacyBilinear,
    /// Normalized Lanczos-3 at output level zero; bilinear at higher levels.
    /// Filters premultiplied RGBA with transparent zero extension. Negative
    /// lobes are preserved (including alpha); no intermediate clamping.
    Lanczos3,
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct LanczosFootprint {
    indices: [[u32; 6]; 6],
    wx: [f32; 6],
    wy: [f32; 6],
}

fn lanczos_axis(fraction: f64) -> [f32; 6] {
    let weights: [f64; 6] = std::array::from_fn(|k| {
        let x = fraction + 2.0 - k as f64;
        if x == 0.0 {
            1.0
        } else if x.abs() >= 3.0 {
            0.0
        } else {
            let p = std::f64::consts::PI * x;
            (p.sin() / p) * ((p / 3.0).sin() / (p / 3.0))
        }
    });
    let sum: f64 = weights.iter().sum();
    weights.map(|w| (w / sum) as f32)
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct Footprint {
    indices: [u32; 4],
    weights: [f32; 2],
}

/// CPU-exact sample geometry, independent of child pixel content.
pub(crate) struct SmartPlan {
    child_level: u8,
    child_extent: Extent,
    child_region: Rect,
    output_extent: Extent,
    samples: Vec<Footprint>,
    lanczos: Vec<LanczosFootprint>,
}

impl SmartPlan {
    /// Builds the same inverse map, determinant-based mip selection and
    /// half-pixel bilinear footprint as the CPU smart-object renderer.
    /// Bounds intersection/transparent-page elision remains the caller's job.
    #[allow(dead_code)] // Also used by standalone legacy reference tests.
    pub(crate) fn new(
        transform: Affine,
        child_canvas: Extent,
        parent_canvas: Extent,
        coord: TileCoord,
    ) -> EngineResult<Self> {
        Self::with_quality(
            transform,
            child_canvas,
            parent_canvas,
            coord,
            SmartQuality::default(),
        )
    }

    pub(crate) fn with_quality(
        transform: Affine,
        child_canvas: Extent,
        parent_canvas: Extent,
        coord: TileCoord,
        quality: SmartQuality,
    ) -> EngineResult<Self> {
        if coord.level >= MAX_LEVEL {
            return Err(EngineError::invalid("level", "outside smart pyramid"));
        }
        let parent = parent_canvas.at_level(coord.level);
        let (cols, rows) = parent.tile_grid(engine_api::tile::TILE_SIZE);
        if coord.x >= cols || coord.y >= rows {
            return Err(EngineError::invalid("coord", "outside parent canvas"));
        }
        let inv = transform
            .inverse()
            .ok_or_else(|| EngineError::invalid("transform", "singular"))?;
        let scale = f64::from(1u32 << coord.level);
        let jac = scale * inv.det().abs().sqrt();
        let child_level = if jac > 1.0 {
            jac.log2().floor().min(f64::from(MAX_LEVEL - 1)) as u8
        } else {
            0
        };
        let child_extent = child_canvas.at_level(child_level);
        let child_pixels = u64::from(child_extent.width) * u64::from(child_extent.height);
        if child_pixels >= u64::from(MISSING) {
            return Err(EngineError::invalid(
                "child canvas",
                "smart indices exceed u32",
            ));
        }
        let cscale = f64::from(1u32 << child_level);
        let rect = Rect::of_tile(coord, parent);
        let output_extent = Extent::new(rect.width() as u32, rect.height() as u32);
        let use_lanczos = quality == SmartQuality::Lanczos3 && coord.level == 0;
        let n = (output_extent.width * output_extent.height) as usize;
        let mut samples = Vec::with_capacity(if use_lanczos { 0 } else { n });
        let mut lanczos = Vec::with_capacity(if use_lanczos { n } else { 0 });
        let index = |x: i64, y: i64| {
            if x < 0
                || y < 0
                || x >= i64::from(child_extent.width)
                || y >= i64::from(child_extent.height)
            {
                MISSING
            } else {
                y as u32 * child_extent.width + x as u32
            }
        };
        for y in 0..output_extent.height {
            for x in 0..output_extent.width {
                let px = (rect.x0 as f64 + f64::from(x) + 0.5) * scale;
                let py = (rect.y0 as f64 + f64::from(y) + 0.5) * scale;
                let (qx, qy) = inv.apply(px, py);
                let (fx, fy) = (qx / cscale - 0.5, qy / cscale - 0.5);
                let (x0, y0) = (fx.floor(), fy.floor());
                let (ix, iy) = (x0 as i64, y0 as i64);
                if use_lanczos {
                    lanczos.push(LanczosFootprint {
                        indices: std::array::from_fn(|y| {
                            std::array::from_fn(|x| {
                                index(
                                    ix.saturating_add(x as i64 - 2),
                                    iy.saturating_add(y as i64 - 2),
                                )
                            })
                        }),
                        wx: lanczos_axis(fx - x0),
                        wy: lanczos_axis(fy - y0),
                    });
                    continue;
                }
                samples.push(Footprint {
                    indices: [
                        index(ix, iy),
                        index(ix.saturating_add(1), iy),
                        index(ix, iy.saturating_add(1)),
                        index(ix.saturating_add(1), iy.saturating_add(1)),
                    ],
                    weights: [(fx - x0) as f32, (fy - y0) as f32],
                });
            }
        }
        let mut child_region = Rect::new(
            i64::from(child_extent.width),
            i64::from(child_extent.height),
            0,
            0,
        );
        for indices in samples
            .iter()
            .map(|s| s.indices.as_slice())
            .chain(lanczos.iter().map(|s| s.indices.as_flattened()))
        {
            for &i in indices {
                if i != MISSING {
                    let x = i64::from(i % child_extent.width);
                    let y = i64::from(i / child_extent.width);
                    child_region.x0 = child_region.x0.min(x);
                    child_region.y0 = child_region.y0.min(y);
                    child_region.x1 = child_region.x1.max(x + 1);
                    child_region.y1 = child_region.y1.max(y + 1);
                }
            }
        }
        Ok(Self {
            child_region,
            child_level,
            child_extent,
            output_extent,
            samples,
            lanczos,
        })
    }

    /// Exact union of nontransparent sampling taps in child-level coordinates.
    #[allow(dead_code)] // Standalone shader tests do not render compact windows.
    pub(crate) fn child_region(&self) -> Rect {
        self.child_region
    }

    /// Rebase absolute tap indices to the compact resident child buffer.
    // Standalone shader tests include this module without the resident caller.
    #[allow(dead_code)]
    pub(crate) fn rebase(&mut self, region: Rect) {
        let width = self.child_extent.width;
        for indices in self
            .samples
            .iter_mut()
            .map(|s| s.indices.as_mut_slice())
            .chain(
                self.lanczos
                    .iter_mut()
                    .map(|s| s.indices.as_flattened_mut()),
            )
        {
            for i in indices {
                if *i != MISSING {
                    let x = i64::from(*i % width) - region.x0;
                    let y = i64::from(*i / width) - region.y0;
                    *i = (y * region.width() + x) as u32;
                }
            }
        }
        self.child_extent = Extent::new(region.width() as u32, region.height() as u32);
    }

    /// Child pyramid level to render before sampling.
    pub(crate) fn child_level(&self) -> u8 {
        self.child_level
    }

    /// Tight output plane dimensions, including partial edge tiles.
    pub(crate) fn output_extent(&self) -> Extent {
        self.output_extent
    }
}

/// Renders a child using the existing shared device, without readback.
/// Cache the returned renderer by child identity/revision and selected level;
/// constructing one per parent tile defeats residency. Nested smart objects
/// follow whatever path the enclosing ResidentRenderer currently implements.
/// The parent resident module can directly borrow `renderer.levels[&level].out`.
// Full-level reference helper used by standalone GPU sampling tests.
#[allow(dead_code)]
pub(crate) fn render_child(
    gpu: &crate::gpu::GpuCompositor,
    so: &crate::document::SmartObject,
    plan: &SmartPlan,
) -> EngineResult<crate::resident::ResidentRenderer> {
    let doc = crate::edit::Document::new((*so.state).clone());
    let mut renderer = crate::resident::ResidentRenderer::new(gpu)?;
    renderer.render(&doc, plan.child_level())?;
    Ok(renderer)
}

/// Resampler compiled on the application's existing shared device.
pub(crate) struct SmartGpu {
    pipeline: wgpu::ComputePipeline,
}

impl SmartGpu {
    /// Compiles once per device; never opens a second adapter/device.
    pub(crate) fn new(device: &wgpu::Device) -> EngineResult<Self> {
        let scope = device.push_error_scope(wgpu::ErrorFilter::Validation);
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("smart GPU resample"),
            source: wgpu::ShaderSource::Wgsl(include_str!("smart_gpu.wgsl").into()),
        });
        let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("smart GPU resample"),
            layout: None,
            module: &module,
            entry_point: Some("main"),
            compilation_options: Default::default(),
            cache: None,
        });
        if let Some(e) = pollster::block_on(scope.pop()) {
            return Err(EngineError::Gpu {
                message: e.to_string(),
            });
        }
        Ok(Self { pipeline })
    }

    /// Encodes sampling without submission, polling or readback. `page_word`
    /// is the *word* offset within the destination smart slab (not a packed
    /// page ID). Destination stores four tight planes, not 256-wide rows.
    pub(crate) fn encode(
        &self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        child_premultiplied: &wgpu::Buffer,
        smart_slab: &wgpu::Buffer,
        page_word: u32,
        plan: &SmartPlan,
    ) -> EngineResult<()> {
        let n = plan.output_extent.width * plan.output_extent.height;
        let child_bytes =
            u64::from(plan.child_extent.width) * u64::from(plan.child_extent.height) * 16;
        let end = u64::from(page_word) + u64::from(n) * 4;
        if child_bytes == 0
            || child_premultiplied.size() < child_bytes
            || end > u64::from(u32::MAX)
            || smart_slab.size() < end * 4
        {
            return Err(EngineError::invalid(
                "smart buffers",
                "buffer extent or offset mismatch",
            ));
        }
        for b in [child_premultiplied, smart_slab] {
            if !b.usage().contains(wgpu::BufferUsages::STORAGE)
                || b.size() > device.limits().max_storage_buffer_binding_size
            {
                return Err(EngineError::invalid(
                    "smart buffers",
                    "requires bindable storage buffers",
                ));
            }
        }
        let footprints = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("smart exact footprints"),
            contents: if plan.lanczos.is_empty() {
                bytemuck::cast_slice(&plan.samples)
            } else {
                bytemuck::cast_slice(&plan.lanczos)
            },
            usage: wgpu::BufferUsages::STORAGE,
        });
        let params = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("smart page destination"),
            contents: bytemuck::cast_slice(&[n, page_word, u32::from(!plan.lanczos.is_empty()), 0]),
            usage: wgpu::BufferUsages::UNIFORM,
        });
        let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("smart GPU resample"),
            layout: &self.pipeline.get_bind_group_layout(0),
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: child_premultiplied.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: footprints.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: smart_slab.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: params.as_entire_binding(),
                },
            ],
        });
        let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
            label: Some("smart GPU resample"),
            timestamp_writes: None,
        });
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &group, &[]);
        pass.dispatch_workgroups(n.div_ceil(64), 1, 1);
        Ok(())
    }
}
