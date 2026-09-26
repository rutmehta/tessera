//! GPU-only Develop on straight document-linear RGBA.
//!
//! Capability is deliberately conservative: lens auto-calibration/CA estimation
//! still requires CPU pixels. Default Auto lens settings are declined. Select
//! lens.profile=None and remove_chromatic_aberration=false for this path.
//! Manual lens gains/distortion and crop/transform are supported. Unsupported
//! geometry plans error explicitly, never silently discard controls.
//!
//! Texture, Clarity, Dehaze statistics and procedural local adjustments run on
//! GPU buffers without a host pixel boundary.
use crate::camera_raw::{parse, profile_matrices};
use compositor::render::smart_filters::FilterContext;
use engine_api::{
    EngineError,
    color::WorkingSpace,
    id::ImageId,
    jobs::CancellationToken,
    stage::{MemoKey, ParamHash, StageId},
    tile::{Extent, TILE_SIZE, TileCoord, TileLayout},
};
use engine_api::{EngineResult, recipe::DevelopSettings};
use image_core::{Op, StageOp, resident::LocalToneOptions};
use pipeline_gpu::resident_rgb_optics::RgbOpticsPlan;
use pipeline_gpu::{GpuContext, GpuStageOp};
use std::{collections::HashMap, sync::Arc};
use wgpu::{Buffer, Device, Queue, util::DeviceExt};

fn supported_settings(s: &DevelopSettings) -> EngineResult<bool> {
    Ok(
        s.effects.lens_blur.is_none()
            && RgbOpticsPlan::new(s, Extent::new(64, 64), None)?.is_some(),
    )
}

/// Settings-only capability. Device/frame resource limits are checked by
/// evaluate. Invalid/unsupported engine schema is an error, not a fallback hint.
pub fn supports(value: &serde_json::Value) -> EngineResult<bool> {
    supported_settings(&parse(value)?.settings)
}

/// Develop caller-owned, tight interleaved straight RGBA, on the same Metal
/// device/queue. Input must be STORAGE and remain immutable until queue work
/// completes. Returns independent STORAGE|COPY_SRC|COPY_DST interleaved RGBA.
/// Matrices operate on signed/HDR linear RGB; alpha never enters Develop.
/// The actual filter extent is level zero (context.level is not a second mip).
/// No uploads/downloads of pixels, f16 checkpoints, or CPU operator fallbacks.
/// Resident finish currently waits; the final interleave is queue-ordered.
pub fn evaluate(
    device: &Device,
    queue: &Queue,
    input: &Buffer,
    extent: Extent,
    value: &serde_json::Value,
    context: &FilterContext,
) -> EngineResult<Buffer> {
    let params = parse(value)?;
    if !supported_settings(&params.settings)? {
        return Err(EngineError::Unsupported { what: "camera_raw resident lens/geometry (including Auto lens analysis); use CPU evaluator".into() });
    }
    let (forward, backward) = profile_matrices(context)?;
    let bytes = extent
        .area()
        .checked_mul(16)
        .ok_or_else(|| EngineError::invalid("camera_raw GPU", "extent overflow"))?;
    let limits = device.limits();
    if bytes == 0
        || bytes > limits.max_buffer_size
        || bytes > limits.max_storage_buffer_binding_size
        || input.size() < bytes
        || input.size() > limits.max_storage_buffer_binding_size
        || !input.usage().contains(wgpu::BufferUsages::STORAGE)
        || extent.area() > u64::from(u32::MAX / 4)
        || extent.width.div_ceil(16) > limits.max_compute_workgroups_per_dimension
        || extent.height.div_ceil(16) > limits.max_compute_workgroups_per_dimension
    {
        return Err(EngineError::invalid(
            "camera_raw GPU",
            "nonempty tight RGBA STORAGE buffer within device limits required",
        ));
    }
    let gpu = Arc::new(GpuContext::from_device(device, queue)?);
    let backend = GpuStageOp::with_cache_budget(gpu.clone(), 0);
    let mut batch = backend
        .begin_resident()
        .ok_or_else(|| EngineError::Unsupported {
            what: "camera_raw resident backend".into(),
        })?;
    let s = &params.settings;
    let optics = RgbOpticsPlan::new(s, extent, None)?.ok_or_else(|| EngineError::Unsupported {
        what: "camera_raw resident optics".into(),
    })?;
    let output_extent = optics.output_extent();
    let local_tone = s.tone.texture != 0. || s.tone.clarity != 0. || s.tone.dehaze != 0.;
    if !batch.supports_output_level(extent) || (local_tone && !batch.supports_local_tone(extent)) {
        return Err(EngineError::Unsupported {
            what: "camera_raw resident frame exceeds GPU local-tone/output limits".into(),
        });
    }
    let allocate = |size, label| {
        device.create_buffer(&wgpu::BufferDescriptor {
            label: Some(label),
            size,
            usage: wgpu::BufferUsages::STORAGE
                | wgpu::BufferUsages::COPY_SRC
                | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        })
    };
    let output = allocate(bytes, "camera raw RGBA output");
    let planar = allocate(extent.area() * 12, "camera raw RGB input");
    let constants = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("camera raw conversion parameters"),
        contents: bytemuck::cast_slice(&[
            extent.width,
            extent.height,
            params.amount.to_bits(),
            0,
            output_extent.width,
            output_extent.height,
            0,
            0,
        ]),
        usage: wgpu::BufferUsages::UNIFORM,
    });
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("camera raw RGBA bridge"),
        source: wgpu::ShaderSource::Wgsl(BRIDGE.into()),
    });
    let convert = |entry: &str, rgb: &Buffer| {
        let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("camera raw RGBA bridge"),
            layout: None,
            module: &shader,
            entry_point: Some(entry),
            compilation_options: Default::default(),
            cache: None,
        });
        let buffers: Vec<(u32, &Buffer)> = if entry == "unpack" {
            vec![(0, input), (1, rgb), (3, &constants)]
        } else {
            vec![(0, input), (1, rgb), (2, &output), (3, &constants)]
        };
        let entries: Vec<_> = buffers
            .iter()
            .map(|(binding, buffer)| wgpu::BindGroupEntry {
                binding: *binding,
                resource: buffer.as_entire_binding(),
            })
            .collect();
        let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("camera raw RGBA bridge"),
            layout: &pipeline.get_bind_group_layout(0),
            entries: &entries,
        });
        let mut encoder = device.create_command_encoder(&Default::default());
        {
            let mut pass = encoder.begin_compute_pass(&Default::default());
            pass.set_pipeline(&pipeline);
            pass.set_bind_group(0, &group, &[]);
            pass.dispatch_workgroups(extent.width.div_ceil(16), extent.height.div_ceil(16), 1);
        }
        queue.submit([encoder.finish()]);
    };
    // Even amount zero validates settings, profile and resource contracts first.
    // pack selects the original exactly at zero, without touching RGB scratch.
    if params.amount == 0. {
        convert("pack", &planar);
        return Ok(output);
    }
    convert("unpack", &planar);
    let coord = TileCoord::new(0, 0, 0);
    let imported = gpu.import_resident_buffer(
        coord,
        TileLayout {
            extent,
            halo: 0,
            channels: 3,
        },
        &planar,
    )?;
    let wb = pipeline_cpu::white_balance_matrix(
        &s.white_balance,
        WorkingSpace::LinearRec2020.to_xyz(),
        [1.; 4],
    )?;
    let mut tiles = HashMap::new();
    let mut coords = Vec::new();
    // Point/detail kernels encode plane lengths in f32. Tile before applying
    // operators so 24MP+ frames do not cross their exact-integer limit.
    for y in 0..extent.height.div_ceil(TILE_SIZE) {
        for x in 0..extent.width.div_ceil(TILE_SIZE) {
            let coord = TileCoord::new(0, x, y);
            let origin = coord.pixel_origin(TILE_SIZE);
            let size = Extent::new(
                (extent.width - origin.0).min(TILE_SIZE),
                (extent.height - origin.1).min(TILE_SIZE),
            );
            let tile = batch.crop(&imported, coord, origin, size)?;
            let tile = batch.run(&Op::Matrix(forward), &tile)?;
            let tile = batch.run(&Op::Matrix(wb), &tile)?;
            coords.push(coord);
            tiles.insert(coord, tile);
        }
    }
    if s.lens.manual_vignetting != 0. {
        let whole = batch.gather_level(extent, coord, 0, &tiles)?;
        let gained = optics.after_white_balance(batch.as_mut(), &whole)?;
        for &coord in &coords {
            let size = tiles[&coord].layout.extent;
            tiles.insert(
                coord,
                batch.crop(&gained, coord, coord.pixel_origin(TILE_SIZE), size)?,
            );
        }
    }
    let halo = pipeline_cpu::detail_halo(&s.detail);
    let mut toned = HashMap::new();
    for &coord in &coords {
        let tile = batch.gather(extent, coord, halo, 1, &tiles)?;
        let tile = batch.run(&Op::Detail(&s.detail), &tile)?;
        toned.insert(coord, batch.run(&Op::Tone(&s.tone), &tile)?);
    }
    drop(tiles);
    if local_tone {
        // Backend is private to this evaluation: this statistics key cannot
        // collide with a different input or reuse stale dehaze statistics.
        toned = batch.local_tone(
            &s.tone,
            extent,
            &toned,
            &coords,
            &LocalToneOptions {
                preview: false,
                statistics_key: MemoKey {
                    image_id: ImageId(0),
                    stage: StageId::Tone,
                    params_hash: ParamHash::default(),
                    tile: coord,
                },
            },
        )?;
    }
    let mut curves = s.tone.clone();
    curves.texture = 0.;
    curves.clarity = 0.;
    curves.dehaze = 0.;
    let mut colored = HashMap::new();
    for &coord in &coords {
        let tile = batch.run_chain(
            &[Op::ToneExtra(&curves), Op::Color(&s.color)],
            &toned[&coord],
        )?;
        colored.insert(coord, tile);
    }
    if !s.locals.adjustments.is_empty() {
        let whole = batch.gather_level(extent, coord, 0, &colored)?;
        let view = gpu.resident_buffer(&whole)?;
        batch.finish(vec![], false, None, &CancellationToken::new())?;
        let local = gpu.locals_resident(view.buffer(), extent, &s.locals.adjustments)?;
        let whole = gpu.import_resident_buffer(coord, whole.layout, local.buffer())?;
        batch = backend.begin_resident().unwrap();
        for &coord in &coords {
            let origin = coord.pixel_origin(TILE_SIZE);
            let tile = batch.crop(&whole, coord, origin, colored[&coord].layout.extent)?;
            colored.insert(coord, tile);
        }
        // Imported buffers retain ownership through their ResidentTile guards.
    }
    let mut developed = HashMap::new();
    for &coord in &coords {
        let tile = batch.run_chain(
            &[Op::EffectsInCrop(&s.effects, extent, &s.geometry.crop)],
            &colored[&coord],
        )?;
        // Matrices are outside the fused tone/curve/color/effects slot order.
        let tile = batch.run(&Op::Matrix(backward), &tile)?;
        developed.insert(coord, tile);
    }
    let final_tile = batch.gather_level(extent, coord, 0, &developed)?;
    let final_tile = if s.lens.manual_distortion != 0. || s.geometry != Default::default() {
        optics.after_effects(batch.as_mut(), &final_tile)?
    } else {
        final_tile
    };
    let view = gpu.resident_buffer(&final_tile)?;
    if view.packed() {
        return Err(EngineError::internal(
            "camera_raw resident unexpectedly exported f16",
        ));
    }
    let completion = batch.finish(vec![], false, None, &CancellationToken::new())?;
    debug_assert!(completion.tiles.is_empty());
    debug_assert_eq!(backend.stats().uploads, 0);
    debug_assert_eq!(backend.stats().pixel_readback_bytes, 0);
    debug_assert_eq!(backend.stats().readbacks, 0);
    // Keep the owning resident view through submission, not only a cloned
    // buffer handle. This prevents scratch-pool aliasing of the export.
    convert("pack", view.buffer());
    Ok(output)
}

const BRIDGE: &str = r#"
struct Params { width: u32, height: u32, amount: f32, pad: u32,
                developed_width: u32, developed_height: u32, pad2: u32, pad3: u32 }
@group(0) @binding(0) var<storage, read> rgba: array<vec4<f32>>;
@group(0) @binding(1) var<storage, read_write> rgb: array<f32>;
@group(0) @binding(2) var<storage, read_write> result: array<vec4<f32>>;
@group(0) @binding(3) var<uniform> p: Params;
@compute @workgroup_size(16,16)
fn unpack(@builtin(global_invocation_id) id: vec3<u32>) {
    if id.x >= p.width || id.y >= p.height { return; }
    let i = id.y * p.width + id.x;
    let n = p.width * p.height;
    rgb[i] = rgba[i].r; rgb[n+i] = rgba[i].g; rgb[2u*n+i] = rgba[i].b;
}
@compute @workgroup_size(16,16)
fn pack(@builtin(global_invocation_id) id: vec3<u32>) {
    if id.x >= p.width || id.y >= p.height { return; }
    let i = id.y * p.width + id.x;
    let n = p.width * p.height;
    let before = rgba[i];
    if p.amount == 0.0 { result[i] = before; return; }
    var developed = vec3<f32>(0.0);
    if id.x < p.developed_width && id.y < p.developed_height {
        let j = id.y * p.developed_width + id.x;
        let count = p.developed_width * p.developed_height;
        developed = vec3<f32>(rgb[j], rgb[count+j], rgb[2u*count+j]);
    }
    result[i] = vec4<f32>(before.rgb + p.amount * (developed - before.rgb), before.a);
}
"#;
