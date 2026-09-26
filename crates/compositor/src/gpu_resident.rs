//! Explicit resident flat-layer API. No CPU pixel work after upload.
use super::*;
use crate::BlendMode;
use std::collections::HashMap;

struct Layer {
    revision: u64,
    levels: Vec<wgpu::Buffer>,
    // One damage rectangle per allocated level. Higher levels remain dirty
    // when a frame requests only a lower one.
    damage: Vec<crate::Rect>,
}
/// Device-owned planar straight-RGBA sources and premultiplied outputs.
/// IDs/revisions are supplied by the document adapter. A scene must only be
/// used with the compositor that created it. No implicit CPU fallback.
pub struct ResidentComposite {
    width: u32,
    height: u32,
    layers: HashMap<u64, Layer>,
    outputs: HashMap<u8, wgpu::Buffer>,
    pipeline: wgpu::ComputePipeline,
    mip_pipeline: wgpu::ComputePipeline,
    steps: wgpu::Buffer,
    capacity: usize,
    uploads: u64,
    mip_texels: u64,
}
impl GpuCompositor {
    /// Create an independently owned resident scene.
    pub fn resident(&self, width: u32, height: u32) -> EngineResult<ResidentComposite> {
        let bytes = u64::from(width) * u64::from(height) * 16;
        if bytes == 0 || bytes > self.device.limits().max_storage_buffer_binding_size {
            return Err(EngineError::Unsupported {
                what: "resident extent exceeds storage binding limit".into(),
            });
        }
        let source = format!(
            "{}\n{}\n{}",
            include_str!("composite.wgsl")
                .split("@compute")
                .next()
                .unwrap(),
            include_str!("resident_adjust.wgsl"),
            include_str!("resident_composite.wgsl")
        );
        let module = self
            .device
            .create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("resident composite"),
                source: wgpu::ShaderSource::Wgsl(source.into()),
            });
        let pipeline = self
            .device
            .create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                label: Some("resident composite"),
                layout: None,
                module: &module,
                entry_point: Some("resident_main"),
                compilation_options: Default::default(),
                cache: None,
            });
        let module = self
            .device
            .create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("resident mip"),
                source: wgpu::ShaderSource::Wgsl(include_str!("resident_mip.wgsl").into()),
            });
        let mip_pipeline = self
            .device
            .create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                label: Some("resident mip"),
                layout: None,
                module: &module,
                entry_point: Some("main"),
                compilation_options: Default::default(),
                cache: None,
            });
        Ok(ResidentComposite {
            width,
            height,
            mip_pipeline,
            layers: HashMap::new(),
            outputs: HashMap::new(),
            pipeline,
            steps: buffer(self, 256, "persistent steps"),
            capacity: 1,
            uploads: 0,
            mip_texels: 0,
        })
    }
}
fn buffer(gpu: &GpuCompositor, size: u64, label: &str) -> wgpu::Buffer {
    gpu.device.create_buffer(&wgpu::BufferDescriptor {
        label: Some(label),
        size,
        usage: wgpu::BufferUsages::STORAGE
            | wgpu::BufferUsages::COPY_DST
            | wgpu::BufferUsages::COPY_SRC,
        mapped_at_creation: false,
    })
}
impl ResidentComposite {
    /// Actual source uploads, excluding step metadata.
    pub fn upload_count(&self) -> u64 {
        self.uploads
    }
    /// Mip pixels submitted for recomputation, excluding padded invocations.
    pub fn mip_texel_count(&self) -> u64 {
        self.mip_texels
    }
    /// Upload once per ID/revision. Samples are four whole-image planes.
    pub fn upload(
        &mut self,
        gpu: &GpuCompositor,
        id: u64,
        revision: u64,
        samples: &[f32],
    ) -> EngineResult<bool> {
        if self.layers.get(&id).is_some_and(|l| l.revision == revision) {
            return Ok(false);
        }
        if samples.len() != self.width as usize * self.height as usize * 4 {
            return Err(EngineError::invalid(
                "samples",
                "expected planar RGBA extent",
            ));
        }
        let b = buffer(gpu, samples.len() as u64 * 4, "resident layer");
        gpu.queue.write_buffer(&b, 0, bytemuck::cast_slice(samples));
        self.layers.insert(
            id,
            Layer {
                revision,
                levels: vec![b],
                damage: vec![crate::Rect::default()],
            },
        );
        self.uploads += 1;
        Ok(true)
    }
    /// Update planar RGBA samples in a level-zero rectangle, invalidating mips.
    pub fn upload_region(
        &mut self,
        gpu: &GpuCompositor,
        id: u64,
        revision: u64,
        rect: [u32; 4],
        samples: &[f32],
    ) -> EngineResult<bool> {
        let [x, y, w, h] = rect;
        if w == 0
            || h == 0
            || x.checked_add(w).is_none_or(|v| v > self.width)
            || y.checked_add(h).is_none_or(|v| v > self.height)
            || samples.len() as u64 != u64::from(w) * u64::from(h) * 4
        {
            return Err(EngineError::invalid("region", "invalid extent or samples"));
        }
        let layer = self
            .layers
            .get_mut(&id)
            .ok_or_else(|| EngineError::invalid("layer", "upload full layer first"))?;
        if layer.revision == revision {
            return Ok(false);
        }
        for c in 0..4usize {
            for row in 0..h as usize {
                let start = (c * h as usize + row) * w as usize;
                let offset = ((c * self.height as usize + y as usize + row) * self.width as usize
                    + x as usize) as u64
                    * 4;
                gpu.queue.write_buffer(
                    &layer.levels[0],
                    offset,
                    bytemuck::cast_slice(&samples[start..start + w as usize]),
                );
            }
        }
        let dirty = crate::Rect::new(x.into(), y.into(), (x + w).into(), (y + h).into());
        for (level, damage) in layer.damage.iter_mut().enumerate().skip(1) {
            *damage = damage.union(&dirty.to_level(level as u8));
        }
        layer.revision = revision;
        self.uploads += 1;
        Ok(true)
    }
    /// Submit a complete flat stack in one compute pass; no wait or readback.
    /// Dirty rectangles are [x, y, width, height] in the requested mip domain.
    pub fn composite(
        &mut self,
        gpu: &GpuCompositor,
        level: u8,
        steps: &[(u64, BlendMode, f32)],
        dirty: Option<[u32; 4]>,
    ) -> EngineResult<()> {
        if level > 31 {
            return Err(EngineError::invalid("level", "must be at most 31"));
        }
        let scale = 1u32 << level;
        let (w, h) = (self.width.div_ceil(scale), self.height.div_ceil(scale));
        let [x, y, rw, rh] = dirty.unwrap_or([0, 0, w, h]);
        if rw == 0
            || rh == 0
            || x.checked_add(rw).is_none_or(|v| v > w)
            || y.checked_add(rh).is_none_or(|v| v > h)
        {
            return Err(EngineError::invalid("dirty", "outside extent"));
        }
        if dirty.is_some() && !self.outputs.contains_key(&level) {
            return Err(EngineError::invalid(
                "dirty",
                "full composite required first",
            ));
        }
        if steps.is_empty() {
            return Err(EngineError::invalid("steps", "empty stack"));
        }
        for (id, _, opacity) in steps {
            if !self.layers.contains_key(id)
                || !opacity.is_finite()
                || !(0.0..=1.0).contains(opacity)
            {
                return Err(EngineError::invalid(
                    "steps",
                    "missing layer or invalid opacity",
                ));
            }
        }
        // Lazy GPU mip chains survive every composite and parameter change.
        let mut mip_encoder = gpu.device.create_command_encoder(&Default::default());
        for (id, _, _) in steps {
            let layer = self.layers.get_mut(id).unwrap();
            for target in 1..=level as usize {
                let previous = target - 1;
                let sw = self.width.div_ceil(1u32 << previous);
                let sh = self.height.div_ceil(1u32 << previous);
                if layer.levels.len() == target {
                    layer.levels.push(buffer(
                        gpu,
                        u64::from(sw.div_ceil(2)) * u64::from(sh.div_ceil(2)) * 16,
                        "resident mip",
                    ));
                    layer.damage.push(crate::Rect::new(
                        0,
                        0,
                        sw.div_ceil(2).into(),
                        sh.div_ceil(2).into(),
                    ));
                }
                let dirty = layer.damage[target];
                if dirty.is_empty() {
                    continue;
                }
                let dims = gpu
                    .device
                    .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                        label: None,
                        contents: bytemuck::cast_slice(&[
                            sw,
                            sh,
                            dirty.x0 as u32,
                            dirty.y0 as u32,
                            dirty.x1 as u32,
                            dirty.y1 as u32,
                        ]),
                        usage: wgpu::BufferUsages::STORAGE,
                    });
                let bindings = [&dims, &layer.levels[previous], &layer.levels[target]];
                let entries: Vec<_> = bindings
                    .iter()
                    .enumerate()
                    .map(|(i, b)| wgpu::BindGroupEntry {
                        binding: i as u32,
                        resource: b.as_entire_binding(),
                    })
                    .collect();
                let group = gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: None,
                    layout: &self.mip_pipeline.get_bind_group_layout(0),
                    entries: &entries,
                });
                {
                    let mut pass = mip_encoder.begin_compute_pass(&Default::default());
                    pass.set_pipeline(&self.mip_pipeline);
                    pass.set_bind_group(0, &group, &[]);
                    pass.dispatch_workgroups(
                        (dirty.width() as u32).div_ceil(8),
                        (dirty.height() as u32).div_ceil(8),
                        1,
                    );
                }
                layer.damage[target] = crate::Rect::default();
                self.mip_texels += dirty.area() as u64;
            }
        }
        gpu.queue.submit([mip_encoder.finish()]);
        let stride = u64::from(gpu.device.limits().min_storage_buffer_offset_alignment).max(256);
        if steps.len() > self.capacity {
            self.capacity = steps.len().next_power_of_two();
            self.steps = buffer(gpu, stride * self.capacity as u64, "persistent steps");
        }
        let mut data = vec![0u8; stride as usize * steps.len()];
        for (i, (id, mode, opacity)) in steps.iter().enumerate() {
            let op = GpuOp {
                kind: 0,
                mode: mode.index(),
                src: 0,
                mask: NO_MASK,
                flags: 0,
                opacity: *opacity,
                fill: 1.0,
                seed: (*id as u32) ^ ((*id >> 32) as u32) ^ 0x9e37_79b9,
                bi: [[0.0, 0.0, 1.0, 1.0]; 8],
            };
            let bytes = bytemuck::bytes_of(&op);
            data[i * stride as usize..i * stride as usize + bytes.len()].copy_from_slice(bytes);
        }
        gpu.queue.write_buffer(&self.steps, 0, &data);
        let output = self
            .outputs
            .entry(level)
            .or_insert_with(|| buffer(gpu, u64::from(w) * u64::from(h) * 16, "resident output"));
        let groups: Vec<_> = steps
            .iter()
            .enumerate()
            .map(|(i, (id, _, _))| {
                let header = [w * h, w, x, y, 1, u32::from(i > 0), rw, rh];
                let hb = gpu
                    .device
                    .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                        label: Some("resident rectangle"),
                        contents: bytemuck::cast_slice(&header),
                        usage: wgpu::BufferUsages::STORAGE,
                    });
                gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
                        label: None,
                        layout: &self.pipeline.get_bind_group_layout(0),
                        entries: &[
                            wgpu::BindGroupEntry {
                                binding: 0,
                                resource: hb.as_entire_binding(),
                            },
                            wgpu::BindGroupEntry {
                                binding: 1,
                                resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                                    buffer: &self.steps,
                                    offset: i as u64 * stride,
                                    size: std::num::NonZeroU64::new(
                                        std::mem::size_of::<GpuOp>() as u64
                                    ),
                                }),
                            },
                            wgpu::BindGroupEntry {
                                binding: 2,
                                resource: self.layers[id].levels[level as usize]
                                    .as_entire_binding(),
                            },
                            wgpu::BindGroupEntry {
                                binding: 3,
                                resource: output.as_entire_binding(),
                            },
                        ],
                    })
            })
            .collect();
        let mut enc = gpu.device.create_command_encoder(&Default::default());
        {
            let mut pass = enc.begin_compute_pass(&Default::default());
            pass.set_pipeline(&self.pipeline);
            for group in &groups {
                pass.set_bind_group(0, group, &[]);
                pass.dispatch_workgroups(rw.div_ceil(8), rh.div_ceil(8), 1);
            }
        }
        gpu.queue.submit([enc.finish()]);
        Ok(())
    }
    /// Apply an adjustment in place to an already composited level.
    /// Opacity interpolates straight RGB; alpha is unchanged.
    pub fn adjust(
        &mut self,
        gpu: &GpuCompositor,
        level: u8,
        adjustment: &crate::Adjustment,
        opacity: f32,
        dirty: Option<[u32; 4]>,
    ) -> EngineResult<()> {
        let output = self
            .outputs
            .get(&level)
            .ok_or_else(|| EngineError::invalid("level", "not composited"))?;
        let w = self.width.div_ceil(1u32 << level);
        let h = self.height.div_ceil(1u32 << level);
        let [x, y, rw, rh] = dirty.unwrap_or([0, 0, w, h]);
        if rw == 0
            || rh == 0
            || x.checked_add(rw).is_none_or(|v| v > w)
            || y.checked_add(rh).is_none_or(|v| v > h)
            || !opacity.is_finite()
            || !(0.0..=1.0).contains(&opacity)
        {
            return Err(EngineError::invalid(
                "adjustment",
                "invalid rectangle or opacity",
            ));
        }
        let mut op = GpuOp {
            kind: 7,
            mode: 0,
            src: 0,
            mask: NO_MASK,
            flags: 0,
            opacity,
            fill: 1.0,
            seed: 0,
            bi: [[0.0; 4]; 8],
        };
        let mut lut = vec![0.0f32];
        use crate::Adjustment;
        match adjustment {
            Adjustment::Invert => {}
            Adjustment::Exposure {
                exposure,
                offset,
                gamma,
            } => {
                op.mode = 1;
                op.bi[0] = [
                    exposure.exp2(),
                    *offset,
                    if *gamma > 0.0 { 1.0 / gamma } else { 1.0 },
                    0.0,
                ];
            }
            Adjustment::Threshold { level } => {
                op.mode = 2;
                op.bi[0][0] = *level;
            }
            Adjustment::Posterize { levels } => {
                op.mode = 3;
                op.bi[0][0] = (*levels).clamp(2, 255) as f32;
            }
            Adjustment::Levels { .. } | Adjustment::Curves { .. } => {
                op.mode = 4;
                if let crate::adjust::Compiled::Luts(ch, master) = adjustment.compile() {
                    lut = ch.into_iter().flatten().chain(master).collect();
                }
            }
            Adjustment::ChannelMixer {
                matrix,
                constant,
                monochrome,
            } => {
                op.mode = 5;
                for i in 0..3 {
                    let r = if *monochrome { 0 } else { i };
                    op.bi[i] = [matrix[r][0], matrix[r][1], matrix[r][2], constant[r]];
                }
            }
            Adjustment::HueSaturation {
                hue,
                saturation,
                lightness,
                colorize,
            } => {
                op.mode = 6;
                op.bi[0] = [
                    *hue / 360.0,
                    (*saturation / 100.0).clamp(-1.0, 1.0),
                    (*lightness / 100.0).clamp(-1.0, 1.0),
                    if *colorize { 1.0 } else { 0.0 },
                ];
            }
        }

        gpu.queue
            .write_buffer(&self.steps, 0, bytemuck::bytes_of(&op));
        let header = [w * h, w, x, y, 1, 1, rw, rh];
        let hb = gpu
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: None,
                contents: bytemuck::cast_slice(&header),
                usage: wgpu::BufferUsages::STORAGE,
            });
        let dummy = gpu
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("adjustment LUT"),
                contents: bytemuck::cast_slice(&lut),
                usage: wgpu::BufferUsages::STORAGE,
            });
        let bindings = [&hb, &self.steps, &dummy, output];
        let entries: Vec<_> = bindings
            .iter()
            .enumerate()
            .map(|(i, b)| wgpu::BindGroupEntry {
                binding: i as u32,
                resource: b.as_entire_binding(),
            })
            .collect();
        let group = gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None,
            layout: &self.pipeline.get_bind_group_layout(0),
            entries: &entries,
        });
        let mut enc = gpu.device.create_command_encoder(&Default::default());
        {
            let mut pass = enc.begin_compute_pass(&Default::default());
            pass.set_pipeline(&self.pipeline);
            pass.set_bind_group(0, &group, &[]);
            pass.dispatch_workgroups(rw.div_ceil(8), rh.div_ceil(8), 1);
        }
        gpu.queue.submit([enc.finish()]);
        Ok(())
    }

    /// Explicit completion barrier; useful for measuring without CPU readback.
    pub fn wait(&self, gpu: &GpuCompositor) -> EngineResult<()> {
        gpu.device
            .poll(wgpu::PollType::wait_indefinitely())
            .map_err(internal)?;
        Ok(())
    }
    /// Explicit, opt-in whole-level readback of premultiplied RGBA planes.
    pub fn readback(&self, gpu: &GpuCompositor, level: u8) -> EngineResult<Vec<f32>> {
        let src = self
            .outputs
            .get(&level)
            .ok_or_else(|| EngineError::invalid("level", "not composited"))?;
        let staging = gpu.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("resident readback"),
            size: src.size(),
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let mut enc = gpu.device.create_command_encoder(&Default::default());
        enc.copy_buffer_to_buffer(src, 0, &staging, 0, src.size());
        gpu.queue.submit([enc.finish()]);
        let (tx, rx) = std::sync::mpsc::channel();
        staging.slice(..).map_async(wgpu::MapMode::Read, move |r| {
            let _ = tx.send(r);
        });
        self.wait(gpu)?;
        rx.recv().map_err(internal)?.map_err(internal)?;
        let data = {
            let mapped = staging.slice(..).get_mapped_range().map_err(internal)?;
            bytemuck::cast_slice::<u8, f32>(&mapped).to_vec()
        };
        staging.unmap();
        Ok(data)
    }
}
