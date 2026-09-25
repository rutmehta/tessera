use super::*;
use image_core::PixelRect;

/// A horizontal output band in the final image. Support is computed in the
/// global source frame, so band boundaries do not change the Lanczos weights.
#[derive(Clone, Copy, Debug)]
pub struct ExportResize {
    pub source: Extent,
    pub destination: Extent,
    pub top: u32,
    pub rows: u32,
}
impl ExportResize {
    pub fn source_rect(self) -> EngineResult<PixelRect> {
        if self.source.area() == 0
            || self.destination.area() == 0
            || self.rows == 0
            || u64::from(self.top) + u64::from(self.rows) > u64::from(self.destination.height)
            || self.source.area() > 100_000_000
            || self.destination.area() > 100_000_000
        {
            return Err(EngineError::invalid("resize", "invalid export band"));
        }
        let ratio = f64::from(self.source.height) / f64::from(self.destination.height);
        let radius = 3.0 * ratio.max(1.0);
        let first = (((f64::from(self.top) + 0.5) * ratio - 0.5 - radius)
            .ceil()
            .max(0.0) as u32)
            .min(self.source.height - 1);
        let last = (((f64::from(self.top + self.rows - 1) + 0.5) * ratio - 0.5 + radius)
            .floor()
            .max(0.0) as u32)
            .min(self.source.height - 1);
        let first = first / TILE_SIZE * TILE_SIZE;
        let end = (last + 1)
            .div_ceil(TILE_SIZE)
            .saturating_mul(TILE_SIZE)
            .min(self.source.height);
        Ok(PixelRect::new(0, first, self.source.width, end - first))
    }
}

fn weights(src: u32, dst: u32, start: u32, len: u32, origin: u32) -> (Vec<u32>, Vec<u32>) {
    let ratio = f64::from(src) / f64::from(dst);
    let scale = ratio.max(1.0);
    let sinc = |x: f64| {
        if x.abs() < 1e-12 {
            1.0
        } else {
            let p = std::f64::consts::PI * x;
            p.sin() / p
        }
    };
    let mut offsets = vec![0];
    let mut taps = Vec::new();
    for i in start..start + len {
        let center = (f64::from(i) + 0.5) * ratio - 0.5;
        let left = (center - 3.0 * scale).ceil() as i64;
        let right = (center + 3.0 * scale).floor() as i64;
        let mut row: Vec<_> = (left..=right)
            .map(|j| {
                let x = (j as f64 - center) / scale;
                (
                    (j.clamp(0, i64::from(src) - 1) as u32) - origin,
                    (sinc(x) * sinc(x / 3.0)) as f32,
                )
            })
            .collect();
        let sum: f32 = row.iter().map(|v| v.1).sum();
        for (index, weight) in &mut row {
            taps.extend([*index, (*weight / sum).to_bits()]);
        }
        offsets.push(taps.len() as u32 / 2);
    }
    (offsets, taps)
}

impl Batch<'_> {
    pub(super) fn resize_export(
        &mut self,
        tiles: Vec<ResidentTile>,
        request: ExportResize,
    ) -> EngineResult<Vec<ResidentTile>> {
        let rect = request.source_rect()?;
        let frame = Extent::new(rect.width, rect.height);
        let layout = TileLayout {
            extent: frame,
            channels: 3,
            halo: 0,
        };
        let src = self.buffer(layout.len() * 4)?;
        // Finish development before assembly copies and the two filter passes.
        self.encode_compute();
        let mut covered = 0u64;
        for tile in &tiles {
            let (x, y) = tile.coord.pixel_origin(TILE_SIZE);
            let e = tile.layout.extent;
            if tile.layout.halo != 0
                || x + e.width > frame.width
                || y < rect.y
                || y + e.height > rect.y + rect.height
            {
                return Err(EngineError::invalid("resize", "source tiles exceed band"));
            }
            covered += e.area();
            let buffer = self.storage(tile)?.clone();
            for c in 0..3 {
                for row in 0..e.height {
                    let from = (c * tile.layout.plane_len() + (row * e.width) as usize) * 4;
                    let to = (c * layout.plane_len()
                        + ((y - rect.y + row) * frame.width + x) as usize)
                        * 4;
                    self.encoder.copy_buffer_to_buffer(
                        &buffer,
                        from as u64,
                        &src,
                        to as u64,
                        u64::from(e.width) * 4,
                    );
                }
            }
        }
        if covered != frame.area() {
            return Err(EngineError::invalid("resize", "incomplete source band"));
        }
        let module = self
            .gpu
            .context()
            .device
            .create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("export Lanczos-3"),
                source: wgpu::ShaderSource::Wgsl(include_str!("export_resize.wgsl").into()),
            });
        let pipeline =
            self.gpu
                .context()
                .device
                .create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                    label: Some("export Lanczos-3"),
                    layout: None,
                    module: &module,
                    entry_point: Some("main"),
                    compilation_options: Default::default(),
                    cache: None,
                });
        let mid = self.buffer(request.destination.width as usize * frame.height as usize * 12)?;
        let out_layout = TileLayout {
            extent: Extent::new(request.destination.width, request.rows),
            channels: 3,
            halo: 0,
        };
        let out = self.buffer(out_layout.len() * 4)?;
        for axis in 0..2 {
            let (input, output, params, offsets, taps) = if axis == 0 {
                let (o, t) = weights(
                    frame.width,
                    request.destination.width,
                    0,
                    request.destination.width,
                    0,
                );
                (
                    &src,
                    &mid,
                    [
                        frame.width,
                        frame.height,
                        request.destination.width,
                        frame.height,
                        0,
                    ],
                    o,
                    t,
                )
            } else {
                let (o, t) = weights(
                    request.source.height,
                    request.destination.height,
                    request.top,
                    request.rows,
                    rect.y,
                );
                (
                    &mid,
                    &out,
                    [
                        request.destination.width,
                        frame.height,
                        request.destination.width,
                        request.rows,
                        1,
                    ],
                    o,
                    t,
                )
            };
            let buffers = [
                self.host_buffer(
                    None,
                    bytemuck::cast_slice(&params),
                    wgpu::BufferUsages::STORAGE,
                ),
                self.host_buffer(
                    None,
                    bytemuck::cast_slice(&offsets),
                    wgpu::BufferUsages::STORAGE,
                ),
                self.host_buffer(
                    None,
                    bytemuck::cast_slice(&taps),
                    wgpu::BufferUsages::STORAGE,
                ),
            ];
            let bindings = [input, output, &buffers[0], &buffers[1], &buffers[2]];
            let entries: Vec<_> = bindings
                .iter()
                .enumerate()
                .map(|(i, b)| wgpu::BindGroupEntry {
                    binding: i as u32,
                    resource: b.as_entire_binding(),
                })
                .collect();
            let group = self
                .gpu
                .context()
                .device
                .create_bind_group(&wgpu::BindGroupDescriptor {
                    label: None,
                    layout: &pipeline.get_bind_group_layout(0),
                    entries: &entries,
                });
            self.record(&pipeline, group, (params[2] * params[3] * 3).div_ceil(64));
        }
        let whole = self.tile(TileCoord::new(0, 0, 0), out_layout, out);
        let mut result = Vec::new();
        for y in 0..out_layout.extent.height.div_ceil(TILE_SIZE) {
            for x in 0..out_layout.extent.width.div_ceil(TILE_SIZE) {
                let origin = (x * TILE_SIZE, y * TILE_SIZE);
                let extent = Extent::new(
                    (out_layout.extent.width - origin.0).min(TILE_SIZE),
                    (out_layout.extent.height - origin.1).min(TILE_SIZE),
                );
                result.push(self.crop(&whole, TileCoord::new(0, x, y), origin, extent)?);
            }
        }
        Ok(result)
    }
}
