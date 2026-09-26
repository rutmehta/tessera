//! Resident whole-level Texture/Clarity/Dehaze inside a render transaction.
//!
//! Input is either one planar tile covering the whole level (level mode: used
//! directly) or halo-free tiles that are packed once into a planar level.
//! Texture/Clarity run as two fused workgroup-tiled kernels: guide `z` ->
//! self-guided coefficients for every active scale, then coefficient means ->
//! guided outputs -> no-new-extrema presence, written straight to the output
//! rect. Dehaze uses separable min filters and box means.
//! Exact Dehaze order statistics and all intermediate pixels stay on-device.
//!
//! Exact mode reproduces `pipeline_cpu::tone_extra_image` on the level (the
//! reference's radii, clipped normalisation and accumulation order). Preview
//! mode (levels above zero only) computes the wide Clarity guided filter on a
//! 1/4-resolution grid of block moments and upsamples its linear coefficients
//! bilinearly; fine/mid scales stay at full level. Dehaze is always exact: a
//! 1/4-grid transmission measured 8/255 display error on synthetic haze,
//! over the 4/255 preview bound. Bounds: `tests/local_tone_resident.rs`.
use super::Batch;
use engine_api::{
    EngineError, EngineResult,
    recipe::settings::ToneSettings,
    stage::MemoKey,
    tile::{Extent, TILE_SIZE, TileCoord, TileLayout},
};
use image_core::resident::{LocalToneOptions, ResidentTile};
use std::collections::HashMap;

const INACTIVE: u32 = u32::MAX;
const PREVIEW_FACTOR: u32 = 2;
// Load / vertical modes (presence.wgsl).
const LOAD_BUFFERS: u32 = 0;
const LOAD_DARK: u32 = 2;
const LOAD_NORM: u32 = 3;
const V_MEAN: u32 = 0;
const V_SELF: u32 = 1;
const V_CROSS: u32 = 2;
const V_DARK: u32 = 3;
const V_TRANS: u32 = 4;
const V_DEHAZE: u32 = 6;
const F_WIDE_LOW: u32 = 4;
const F_PACK_Z: u32 = 16;
/// Workgroups per grid row for linear kernels (see `Batch::record`).
const ROW: u32 = 65535;

pub(crate) struct Pipelines {
    layout: wgpu::BindGroupLayout,
    box_h: wgpu::ComputePipeline,
    box_v: wgpu::ComputePipeline,
    down: wgpu::ComputePipeline,
    pack: wgpu::ComputePipeline,
    unpack: wgpu::ComputePipeline,
    zpass: wgpu::ComputePipeline,
    pres_coef: wgpu::ComputePipeline,
    pres_apply: wgpu::ComputePipeline,
    stats_sort: wgpu::ComputePipeline,
    stats_candidates: wgpu::ComputePipeline,
    stats_finish: wgpu::ComputePipeline,
}

// Reserved cache storage owned by GpuStageOp; resident statistics are currently
// transaction-local so aborted command encoders cannot publish incomplete data.
pub(crate) type Statistics = std::collections::VecDeque<(MemoKey, wgpu::Buffer)>;

impl Pipelines {
    pub(crate) fn new(ctx: &crate::GpuContext) -> EngineResult<Self> {
        let scope = ctx.device.push_error_scope(wgpu::ErrorFilter::Validation);
        let module = ctx
            .device
            .create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("resident local tone"),
                source: wgpu::ShaderSource::Wgsl(
                    concat!(
                        include_str!("presence.wgsl"),
                        "\n",
                        include_str!("dehaze_stats.wgsl")
                    )
                    .into(),
                ),
            });
        let entries: Vec<_> = (0..9)
            .map(|binding| wgpu::BindGroupLayoutEntry {
                binding,
                visibility: wgpu::ShaderStages::COMPUTE,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Storage {
                        read_only: binding <= 3 || binding == 8,
                    },
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            })
            .collect();
        let layout = ctx
            .device
            .create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("resident local tone"),
                entries: &entries,
            });
        let pipeline_layout = ctx
            .device
            .create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("resident local tone"),
                bind_group_layouts: &[Some(&layout)],
                immediate_size: 0,
            });
        let pipeline = |entry: &str| {
            ctx.device
                .create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                    label: Some(entry),
                    layout: Some(&pipeline_layout),
                    module: &module,
                    entry_point: Some(entry),
                    // Every kernel writes the workgroup memory it reads
                    // before its first barrier; skip the implicit zero fill.
                    compilation_options: wgpu::PipelineCompilationOptions {
                        zero_initialize_workgroup_memory: false,
                        ..Default::default()
                    },
                    cache: None,
                })
        };
        let pipelines = Self {
            box_h: pipeline("box_h"),
            box_v: pipeline("box_v"),
            down: pipeline("down"),
            pack: pipeline("pack"),
            unpack: pipeline("unpack"),
            zpass: pipeline("zpass"),
            pres_coef: pipeline("pres_coef"),
            pres_apply: pipeline("pres_apply"),
            stats_sort: pipeline("stats_sort"),
            stats_candidates: pipeline("stats_candidates"),
            stats_finish: pipeline("stats_finish"),
            layout,
        };
        if let Some(e) = pollster::block_on(scope.pop()) {
            return Err(EngineError::internal(format!("local tone shader: {e}")));
        }
        Ok(pipelines)
    }
}

impl Pipelines {
    pub(crate) fn name(&self, p: &wgpu::ComputePipeline) -> Option<&'static str> {
        [
            (&self.box_h, "local tone box_h"),
            (&self.box_v, "local tone box_v"),
            (&self.down, "local tone down"),
            (&self.pack, "local tone pack"),
            (&self.unpack, "local tone unpack"),
            (&self.zpass, "local tone z"),
            (&self.pres_coef, "presence coefficients"),
            (&self.pres_apply, "presence apply"),
            (&self.stats_sort, "dehaze exact sort"),
            (&self.stats_candidates, "dehaze candidates"),
            (&self.stats_finish, "dehaze statistics"),
        ]
        .into_iter()
        .find(|(q, _)| *q == p)
        .map(|(_, n)| n)
    }
}

/// Whether a level of `frame` fits the device's storage-buffer limits.
pub(crate) fn supported(ctx: &crate::GpuContext, frame: Extent) -> bool {
    let limits = ctx.device.limits();
    let bytes = frame.area() * 16;
    frame.area() > 0
        && limits.max_storage_buffers_per_shader_stage >= 9
        && limits.max_compute_workgroup_storage_size >= 16 << 10
        && bytes <= limits.max_storage_buffer_binding_size
        && bytes <= limits.max_buffer_size
        && frame.width.div_ceil(16) <= limits.max_compute_workgroups_per_dimension
        && frame.height.div_ceil(4) <= limits.max_compute_workgroups_per_dimension
}

#[derive(Default, Clone, Copy)]
struct Bind<'b> {
    src: Option<&'b wgpu::Buffer>,
    ina: Option<&'b wgpu::Buffer>,
    inb: Option<&'b wgpu::Buffer>,
    low: Option<&'b wgpu::Buffer>,
    outa: Option<&'b wgpu::Buffer>,
    outb: Option<&'b wgpu::Buffer>,
    zbuf: Option<&'b wgpu::Buffer>,
    dst: Option<&'b wgpu::Buffer>,
}

/// Parameter block (presence.wgsl header).
#[derive(Clone)]
struct Params([u32; 24]);
impl Params {
    fn new(frame: Extent, mode: u32, radii: [u32; 4]) -> Self {
        let mut p = [0; 24];
        p[0] = frame.width;
        p[1] = frame.height;
        p[2] = mode;
        p[3..7].copy_from_slice(&radii);
        p[9] = frame.width;
        p[10] = frame.height;
        p[23] = frame.width * frame.height;
        Self(p)
    }
    fn rect(mut self, x: u32, y: u32, extent: Extent) -> Self {
        self.0[7] = x;
        self.0[8] = y;
        self.0[9] = extent.width;
        self.0[10] = extent.height;
        self.0[23] = extent.area() as u32;
        self
    }
    fn f(mut self, i: usize, v: f32) -> Self {
        self.0[i] = v.to_bits();
        self
    }
    fn u(mut self, i: usize, v: u32) -> Self {
        self.0[i] = v;
        self
    }
}

/// Linear kernels: rows of at most [`ROW`] workgroups of 64.
fn linear(n: u32) -> [u32; 2] {
    let groups = n.div_ceil(64);
    [groups.min(ROW), groups.div_ceil(ROW)]
}

struct Run<'r, 'a> {
    batch: &'r mut Batch<'a>,
    pipelines: std::sync::Arc<Pipelines>,
    unused_read: wgpu::Buffer,
    unused_write: [wgpu::Buffer; 4],
    frame: Extent,
}

impl Run<'_, '_> {
    fn dispatch(
        &mut self,
        pipeline: &wgpu::ComputePipeline,
        params: Params,
        b: Bind<'_>,
        groups: [u32; 2],
    ) {
        let p = self.batch.host_buffer(
            Some("local tone parameters"),
            bytemuck::cast_slice(&params.0),
            wgpu::BufferUsages::STORAGE,
        );
        let read = &self.unused_read;
        let w = &self.unused_write;
        let buffers = [
            b.src.unwrap_or(read),
            b.ina.unwrap_or(read),
            b.inb.unwrap_or(read),
            b.low.unwrap_or(read),
            b.outa.unwrap_or(&w[0]),
            b.outb.unwrap_or(&w[1]),
            b.zbuf.unwrap_or(&w[2]),
            b.dst.unwrap_or(&w[3]),
            &p,
        ];
        let entries: Vec<_> = buffers
            .iter()
            .enumerate()
            .map(|(i, buffer)| wgpu::BindGroupEntry {
                binding: i as u32,
                resource: buffer.as_entire_binding(),
            })
            .collect();
        let group = self
            .batch
            .gpu
            .context()
            .device
            .create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("resident local tone"),
                layout: &self.pipelines.layout,
                entries: &entries,
            });
        self.batch.record_2d(pipeline, group, groups);
    }
    /// One vec4 per pixel of `frame`.
    fn vec4_buffer(&self, frame: Extent) -> EngineResult<wgpu::Buffer> {
        self.batch.buffer(frame.area() as usize * 16)
    }
    fn release(&self, buffers: impl IntoIterator<Item = wgpu::Buffer>) {
        self.batch.pool.lock().unwrap().free.extend(buffers);
    }
    fn h(&mut self, frame: Extent, params: Params, b: Bind<'_>) {
        let groups = [frame.width.div_ceil(64), frame.height.div_ceil(4)];
        let pipeline = self.pipelines.box_h.clone();
        self.dispatch(&pipeline, params, b, groups);
    }
    fn v(&mut self, params: Params, b: Bind<'_>) {
        let groups = [params.0[9].div_ceil(16), params.0[10].div_ceil(16)];
        let pipeline = self.pipelines.box_v.clone();
        self.dispatch(&pipeline, params, b, groups);
    }
    /// Self-guided coefficients (A, B) of z on the 1/4 grid of block moments
    /// (radius scaled with the grid): the preview Clarity approximation.
    fn low_coefficients(
        &mut self,
        zbuf: &wgpu::Buffer,
        radius: u32,
    ) -> EngineResult<(wgpu::Buffer, Extent)> {
        let f = PREVIEW_FACTOR;
        let low = Extent::new(self.frame.width.div_ceil(f), self.frame.height.div_ceil(f));
        let r = radius.div_ceil(f);
        let a = self.vec4_buffer(low)?;
        let b = self.vec4_buffer(low)?;
        let c = self.vec4_buffer(low)?;
        let d = self.vec4_buffer(low)?;
        let params = Params::new(self.frame, 0, [INACTIVE; 4])
            .u(19, low.width)
            .u(20, low.height)
            .u(21, f);
        let pipeline = self.pipelines.down.clone();
        self.dispatch(
            &pipeline,
            params,
            Bind {
                zbuf: Some(zbuf),
                outa: Some(&a),
                ..Default::default()
            },
            linear(low.area() as u32),
        );
        let radii = [r, INACTIVE, INACTIVE, INACTIVE];
        for mode in [V_SELF, V_MEAN] {
            self.h(
                low,
                Params::new(low, LOAD_BUFFERS, radii),
                Bind {
                    ina: Some(&a),
                    inb: Some(&a),
                    outa: Some(&b),
                    outb: Some(&c),
                    ..Default::default()
                },
            );
            self.v(
                Params::new(low, mode, radii),
                Bind {
                    ina: Some(&b),
                    inb: Some(&c),
                    outa: Some(&a),
                    outb: Some(&d),
                    ..Default::default()
                },
            );
        }
        self.release([b, c, d]);
        Ok((a, low))
    }
    /// The identity output of `current` (neutral or statistics-free Dehaze).
    fn copy_outputs(
        &mut self,
        current: &wgpu::Buffer,
        whole: bool,
        outputs: &[TileCoord],
    ) -> EngineResult<HashMap<TileCoord, ResidentTile>> {
        let unpack = self.pipelines.unpack.clone();
        let frame = self.frame;
        write_outputs(
            self,
            whole,
            outputs,
            Params::new(frame, 0, [INACTIVE; 4]),
            Bind {
                src: Some(current),
                ..Default::default()
            },
            Output::Linear(&unpack),
        )
    }
}

impl Batch<'_> {
    fn local_tone_pipelines(&self) -> EngineResult<std::sync::Arc<Pipelines>> {
        self.gpu
            .local_tone
            .get_or_init(|| {
                Pipelines::new(self.gpu.context())
                    .map(std::sync::Arc::new)
                    .map_err(|e| e.to_string())
            })
            .clone()
            .map_err(EngineError::internal)
    }

    pub(super) fn local_tone_impl(
        &mut self,
        s: &ToneSettings,
        frame: Extent,
        tiles: &HashMap<TileCoord, ResidentTile>,
        outputs: &[TileCoord],
        options: &LocalToneOptions,
    ) -> EngineResult<HashMap<TileCoord, ResidentTile>> {
        if [s.texture, s.clarity, s.dehaze]
            .iter()
            .any(|v| !v.is_finite())
        {
            return Err(EngineError::invalid(
                "presence",
                "parameters must be finite",
            ));
        }
        if !supported(self.gpu.context(), frame) {
            return Err(EngineError::invalid(
                "local tone level",
                "exceeds GPU buffer/dispatch limits",
            ));
        }
        for tile in tiles.values() {
            if tile.layout.halo != 0 || tile.layout.channels != 3 {
                return Err(EngineError::invalid(
                    "local tone tile",
                    "halo-free RGB tiles required",
                ));
            }
        }
        // One tile spanning the frame is the level itself (level mode).
        let whole = (tiles.len() == 1)
            .then(|| tiles.values().next().unwrap())
            .filter(|t| t.layout.extent == frame);
        let pipelines = self.local_tone_pipelines()?;
        let device = self.gpu.context().device.clone();
        let small = |label| {
            device.create_buffer(&wgpu::BufferDescriptor {
                label: Some(label),
                size: 16,
                usage: wgpu::BufferUsages::STORAGE,
                mapped_at_creation: false,
            })
        };
        let mut run = Run {
            batch: self,
            pipelines: pipelines.clone(),
            unused_read: small("local tone unused input"),
            unused_write: [
                small("unused a"),
                small("unused b"),
                small("unused z"),
                small("unused output"),
            ],
            frame,
        };
        let planar = frame.area() as usize * 12;
        let presence = s.texture != 0.0 || s.clarity != 0.0;
        let dehaze = s.dehaze != 0.0;
        let zbuf = if presence {
            Some(run.batch.buffer(frame.area() as usize * 4)?)
        } else {
            None
        };
        // 1. The planar level: the level tile itself, or packed tiles.
        let (mut current, mut owned) = match whole {
            Some(t) => (run.batch.storage(t)?.clone(), false),
            None => {
                let level = run.batch.buffer(planar)?;
                let pack = pipelines.pack.clone();
                for (&coord, tile) in tiles {
                    let (x, y) = coord.pixel_origin(TILE_SIZE);
                    let e = tile.layout.extent;
                    if x + e.width > frame.width || y + e.height > frame.height {
                        return Err(EngineError::invalid("local tone tile", "outside level"));
                    }
                    let src = run.batch.storage(tile)?.clone();
                    run.dispatch(
                        &pack,
                        Params::new(frame, 0, [INACTIVE; 4])
                            .rect(x, y, e)
                            .u(22, if presence { F_PACK_Z } else { 0 }),
                        Bind {
                            src: Some(&src),
                            dst: Some(&level),
                            zbuf: zbuf.as_ref(),
                            ..Default::default()
                        },
                        [(e.area() as u32).div_ceil(64), 1],
                    );
                }
                (level, true)
            }
        };
        if let (Some(z), true) = (&zbuf, whole.is_some()) {
            let zpass = pipelines.zpass.clone();
            run.dispatch(
                &zpass,
                Params::new(frame, 0, [INACTIVE; 4]),
                Bind {
                    src: Some(&current),
                    zbuf: Some(z),
                    ..Default::default()
                },
                [frame.width.div_ceil(16), frame.height.div_ceil(16)],
            );
        }
        let mut result = None;
        // 2. Texture / Clarity.
        if let Some(zbuf) = &zbuf {
            let texture = s.texture.clamp(-100.0, 100.0) / 100.0;
            let clarity = s.clarity.clamp(-100.0, 100.0) / 100.0;
            let low_wide = options.preview && clarity != 0.0;
            // Compact active scales into slots; roles map fine/mid/wide.
            let mut radii = [INACTIVE; 4];
            let mut roles = [15u32; 3];
            let mut slots = 0;
            for (role, radius, on) in [
                (0, 1, texture != 0.0),
                (1, 3, true),
                (2, 8, clarity != 0.0 && !low_wide),
            ] {
                if on {
                    radii[slots] = radius;
                    roles[role] = slots as u32;
                    slots += 1;
                }
            }
            let roles = roles[0] | roles[1] << 4 | roles[2] << 8;
            let ca = run.vec4_buffer(frame)?;
            let cb = run.vec4_buffer(if slots > 2 { frame } else { Extent::new(1, 1) })?;
            let groups = [frame.width.div_ceil(16), frame.height.div_ceil(16)];
            let coef = pipelines.pres_coef.clone();
            run.dispatch(
                &coef,
                Params::new(frame, 0, radii).u(6, roles),
                Bind {
                    zbuf: Some(zbuf),
                    outa: Some(&ca),
                    outb: Some(&cb),
                    ..Default::default()
                },
                groups,
            );
            let low = if low_wide {
                Some(run.low_coefficients(zbuf, 8)?)
            } else {
                None
            };
            let mut params = Params::new(frame, 0, radii)
                .u(6, roles)
                .f(12, texture)
                .f(13, clarity)
                .u(22, if low_wide { F_WIDE_LOW } else { 0 });
            if let Some((_, l)) = &low {
                params = params.u(19, l.width).u(20, l.height).u(21, PREVIEW_FACTOR);
            }
            let bind = Bind {
                src: Some(&current),
                ina: Some(&ca),
                inb: Some(&cb),
                low: low.as_ref().map(|l| &l.0),
                zbuf: Some(zbuf),
                ..Default::default()
            };
            let apply = pipelines.pres_apply.clone();
            if dehaze {
                let next = run.batch.buffer(planar)?;
                run.dispatch(
                    &apply,
                    params,
                    Bind {
                        dst: Some(&next),
                        ..bind
                    },
                    groups,
                );
                let previous = std::mem::replace(&mut current, next);
                if owned {
                    run.release([previous]);
                }
                owned = true;
            } else {
                result = Some(write_outputs(
                    &mut run,
                    whole.is_some(),
                    outputs,
                    params,
                    bind,
                    Output::Apply(&apply),
                )?);
            }
            run.release([ca, cb]);
            if let Some((l, _)) = low {
                run.release([l]);
            }
        }
        // 3. Dehaze on the presence output.
        if dehaze {
            // Keep statistics transaction-local: publishing an unsubmitted GPU
            // buffer in the shared cache would poison later/cancelled batches.
            let stats = statistics(&mut run, &current)?;
            result = Some({
                let base = |mode, radii| {
                    Params::new(frame, mode, radii)
                        .f(17, s.dehaze.abs().min(100.0) / 100.0)
                        .f(18, s.dehaze)
                };
                let h = run.vec4_buffer(frame)?;
                let hb = run.vec4_buffer(frame)?;
                let m = run.vec4_buffer(frame)?;
                let r3 = [3, INACTIVE, INACTIVE, INACTIVE];
                run.h(
                    frame,
                    base(LOAD_NORM, r3),
                    Bind {
                        src: Some(&current),
                        low: Some(&stats),
                        outa: Some(&h),
                        outb: Some(&hb),
                        ..Default::default()
                    },
                );
                run.v(
                    base(V_TRANS, r3),
                    Bind {
                        src: Some(&current),
                        low: Some(&stats),
                        ina: Some(&h),
                        inb: Some(&h),
                        outa: Some(&m),
                        ..Default::default()
                    },
                );
                let r4 = [4, 4, INACTIVE, INACTIVE];
                run.h(
                    frame,
                    Params::new(frame, LOAD_BUFFERS, r4),
                    Bind {
                        ina: Some(&m),
                        inb: Some(&m),
                        outa: Some(&h),
                        outb: Some(&hb),
                        ..Default::default()
                    },
                );
                run.v(
                    Params::new(frame, V_CROSS, r4),
                    Bind {
                        ina: Some(&h),
                        inb: Some(&hb),
                        outa: Some(&m),
                        ..Default::default()
                    },
                );
                let r = [4, INACTIVE, INACTIVE, INACTIVE];
                run.h(
                    frame,
                    Params::new(frame, LOAD_BUFFERS, r),
                    Bind {
                        ina: Some(&m),
                        inb: Some(&m),
                        outa: Some(&h),
                        outb: Some(&hb),
                        ..Default::default()
                    },
                );
                let tiles = write_outputs(
                    &mut run,
                    whole.is_some(),
                    outputs,
                    base(V_DEHAZE, r),
                    Bind {
                        src: Some(&current),
                        low: Some(&stats),
                        ina: Some(&h),
                        inb: Some(&h),
                        ..Default::default()
                    },
                    Output::Vertical,
                )?;
                run.release([h, hb, m]);
                tiles
            });
            run.release([stats]);
        }
        let result = match result {
            Some(t) => t,
            // Neutral settings: identity copy of the requested rects.
            None => run.copy_outputs(&current, whole.is_some(), outputs)?,
        };
        if owned {
            run.release([current]);
        }
        run.release(zbuf);
        Ok(result)
    }
}

/// Exact global airlight/confidence: GPU dark channel and merge order statistics.
fn statistics(run: &mut Run<'_, '_>, current: &wgpu::Buffer) -> EngineResult<wgpu::Buffer> {
    let frame = run.frame;
    let h = run.vec4_buffer(frame)?;
    let unused = run.vec4_buffer(frame)?;
    let d = run.vec4_buffer(frame)?;
    let radii = [3, INACTIVE, INACTIVE, INACTIVE];
    run.h(
        frame,
        Params::new(frame, LOAD_DARK, radii),
        Bind {
            src: Some(current),
            outa: Some(&h),
            outb: Some(&unused),
            ..Default::default()
        },
    );
    run.v(
        Params::new(frame, V_DARK, radii),
        Bind {
            src: Some(current),
            ina: Some(&h),
            inb: Some(&h),
            outa: Some(&d),
            ..Default::default()
        },
    );
    let mut a = h;
    let mut b = unused;
    // Preserve the unsorted dark channel for the candidate predicate.
    let sort = run.pipelines.stats_sort.clone();
    let n = frame.area() as u32;
    let mut input = d.clone();
    let mut width = 1;
    while width < n {
        run.dispatch(
            &sort,
            Params::new(frame, 0, [INACTIVE; 4]).u(11, width),
            Bind {
                ina: Some(&input),
                outa: Some(&a),
                ..Default::default()
            },
            linear(n),
        );
        input = a.clone();
        std::mem::swap(&mut a, &mut b);
        width *= 2;
    }
    let quantiles = run.batch.buffer(16)?;
    let stats = run.batch.buffer(16)?;
    let candidates = run.pipelines.stats_candidates.clone();
    run.dispatch(
        &candidates,
        Params::new(frame, 0, [INACTIVE; 4]),
        Bind {
            src: Some(current),
            ina: Some(&input),
            inb: Some(&d),
            outa: Some(&a),
            outb: Some(&quantiles),
            ..Default::default()
        },
        linear(n),
    );
    input = a.clone();
    std::mem::swap(&mut a, &mut b);
    width = 1;
    while width < n {
        run.dispatch(
            &sort,
            Params::new(frame, 0, [INACTIVE; 4]).u(11, width),
            Bind {
                ina: Some(&input),
                outa: Some(&a),
                ..Default::default()
            },
            linear(n),
        );
        input = a.clone();
        std::mem::swap(&mut a, &mut b);
        width *= 2;
    }
    let finish = run.pipelines.stats_finish.clone();
    run.dispatch(
        &finish,
        Params::new(frame, 0, [INACTIVE; 4]),
        Bind {
            ina: Some(&input),
            inb: Some(&quantiles),
            outa: Some(&stats),
            ..Default::default()
        },
        [1, 1],
    );
    run.release([a, b, d, quantiles]);
    Ok(stats)
}

/// Final kernel shape per output rect.
enum Output<'p> {
    /// 16×16 workgroups over the rect (fused presence apply).
    Apply(&'p wgpu::ComputePipeline),
    /// The vertical box kernel (16×16 workgroups).
    Vertical,
    /// One invocation per output pixel.
    Linear(&'p wgpu::ComputePipeline),
}

fn write_outputs(
    run: &mut Run<'_, '_>,
    whole: bool,
    outputs: &[TileCoord],
    params: Params,
    bind: Bind<'_>,
    kernel: Output<'_>,
) -> EngineResult<HashMap<TileCoord, ResidentTile>> {
    let frame = run.frame;
    let mut out = HashMap::new();
    for &coord in outputs {
        let (x, y) = if whole {
            (0, 0)
        } else {
            coord.pixel_origin(TILE_SIZE)
        };
        if x >= frame.width || y >= frame.height {
            return Err(EngineError::invalid("local tone output", "outside level"));
        }
        let extent = if whole {
            frame
        } else {
            Extent::new(
                (frame.width - x).min(TILE_SIZE),
                (frame.height - y).min(TILE_SIZE),
            )
        };
        let layout = TileLayout {
            extent,
            halo: 0,
            channels: 3,
        };
        let dst = run.batch.buffer(layout.len() * 4)?;
        let p = params.clone().rect(x, y, extent);
        let bind = Bind {
            dst: Some(&dst),
            ..bind
        };
        match kernel {
            Output::Apply(pipeline) => {
                let groups = [extent.width.div_ceil(16), extent.height.div_ceil(16)];
                run.dispatch(pipeline, p, bind, groups);
            }
            Output::Vertical => run.v(p, bind),
            Output::Linear(pipeline) => {
                run.dispatch(pipeline, p, bind, linear(extent.area() as u32));
            }
        }
        out.insert(coord, run.batch.tile(coord, layout, dst));
    }
    Ok(out)
}
