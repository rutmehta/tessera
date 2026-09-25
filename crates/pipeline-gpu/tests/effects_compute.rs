use engine_api::{
    recipe::settings::{Crop, EffectsSettings},
    stage::StageId,
    tile::{Extent, TileCoord},
};
use image_core::{Op, StageOp};
use pipeline_gpu::{GpuContext, GpuStageOp};
use std::sync::Arc;

#[path = "../src/effects.rs"]
mod effects;

#[test]

fn public_effects_executes_compute() {
    let gpu = GpuStageOp::new(Arc::new(GpuContext::new().unwrap()));
    let image = pipeline_cpu::Image::new(17, 9, vec![vec![0.5; 153]; 3]).unwrap();
    let tile = image.tile(TileCoord::new(0, 0, 0), 2, 1).unwrap();
    let mut s = EffectsSettings::default();
    s.vignette.amount = -70.;
    s.grain.amount = 50.;
    gpu.run(
        StageId::Tone,
        &Op::EffectsInCrop(&s, Extent::new(17, 9), &Crop::default()),
        tile,
    )
    .unwrap();
    assert_eq!(
        gpu.stats().submissions,
        1,
        "effects must execute compute, not CPU fallback"
    );
    assert_eq!(gpu.stats().uploads, 1);
    assert_eq!(gpu.stats().readbacks, 1);
}

use engine_api::{
    recipe::settings::{LensBlur, NormalizedRect, VignetteStyle},
    tile::{Tile, TileLayout},
};
fn compare(
    ctx: &GpuContext,
    tile: &Tile,
    s: &EffectsSettings,
    extent: Extent,
    crop: &Crop,
) -> Tile {
    let expected = image_core::CpuStageOp
        .run(
            StageId::Tone,
            &Op::EffectsInCrop(s, extent, crop),
            tile.clone(),
        )
        .unwrap();
    let actual = effects::run(ctx, tile, s, extent, crop).unwrap();
    assert_eq!(actual.layout(), tile.layout());
    assert_eq!(actual.coord(), tile.coord());
    let mut max = 0f32;
    for (i, (&a, &b)) in actual
        .samples::<f32>()
        .unwrap()
        .iter()
        .zip(expected.samples::<f32>().unwrap())
        .enumerate()
    {
        max = max.max((a - b).abs());
        assert!(
            a.is_finite() && (a - b).abs() <= 1e-4,
            "sample {i}: GPU {a} CPU {b}, settings {s:?}"
        );
    }
    eprintln!("effects max error {max}");
    actual
}
#[test]
fn isolated_styles_signed_hdr_crop_halos_levels_and_neutral() {
    let ctx = GpuContext::new().unwrap();
    let image = pipeline_cpu::Image::new(
        520,
        17,
        (0..3)
            .map(|c| {
                (0..520 * 17)
                    .map(|i| ((i * 7 + c * 13) % 113) as f32 / 21. - 1.5)
                    .collect()
            })
            .collect(),
    )
    .unwrap();
    for style in [
        VignetteStyle::HighlightPriority,
        VignetteStyle::ColorPriority,
        VignetteStyle::PaintOverlay,
    ] {
        for amount in [-100., -43., 0., 67., 100.] {
            for angle in [0., 31.] {
                let crop = Crop {
                    angle,
                    rect: NormalizedRect {
                        left: 0.13,
                        top: 0.21,
                        right: 0.83,
                        bottom: 0.92,
                    },
                    ..Default::default()
                };
                let mut s = EffectsSettings::default();
                s.vignette.style = style;
                s.vignette.amount = amount;
                s.vignette.highlights = 37.;
                s.vignette.midpoint = 23.;
                s.vignette.roundness = -73.;
                s.vignette.feather = if amount == 100. { 0. } else { 77. };
                s.grain.amount = 81.;
                s.grain.size = 0.;
                s.grain.roughness = 93.;
                for x in [0, 1, 2] {
                    let tile = image.tile(TileCoord::new(0, x, 0), 2, 1).unwrap();
                    compare(&ctx, &tile, &s, Extent::new(520, 17), &crop);
                }
                let layout = TileLayout {
                    extent: Extent::new(4, 9),
                    halo: 2,
                    channels: 3,
                };
                let tile = Tile::from_samples(
                    TileCoord::new(1, 1, 0),
                    layout,
                    vec![0.5f32; layout.plane_len() * 3],
                )
                .unwrap();
                compare(&ctx, &tile, &s, Extent::new(520, 17), &crop);
            }
        }
    }
    let layout = TileLayout {
        extent: Extent::new(2, 1),
        halo: 0,
        channels: 3,
    };
    let tile = Tile::from_samples(
        TileCoord::new(0, 0, 0),
        layout,
        vec![-0.0f32, 2., -1., f32::MAX, f32::MIN_POSITIVE, 0.],
    )
    .unwrap();
    let out = compare(
        &ctx,
        &tile,
        &EffectsSettings::default(),
        Extent::new(2, 1),
        &Crop::default(),
    );
    assert_eq!(
        out.samples::<f32>()
            .unwrap()
            .iter()
            .map(|v| v.to_bits())
            .collect::<Vec<_>>(),
        tile.samples::<f32>()
            .unwrap()
            .iter()
            .map(|v| v.to_bits())
            .collect::<Vec<_>>()
    );
}
#[test]
fn isolated_validation_matches_cpu() {
    let ctx = GpuContext::new().unwrap();
    let l = TileLayout {
        extent: Extent::new(2, 1),
        halo: 0,
        channels: 3,
    };
    let base = Tile::from_samples(TileCoord::new(0, 0, 0), l, vec![0.5f32; 6]).unwrap();
    for case in 0..17 {
        let mut s = EffectsSettings::default();
        let mut crop = Crop::default();
        let mut e = Extent::new(2, 1);
        let mut tile = base.clone();
        match case {
            0..=7 => {
                let controls = [
                    &mut s.vignette.amount,
                    &mut s.vignette.midpoint,
                    &mut s.vignette.roundness,
                    &mut s.vignette.feather,
                    &mut s.vignette.highlights,
                    &mut s.grain.amount,
                    &mut s.grain.size,
                    &mut s.grain.roughness,
                ];
                *controls[case] = f32::NAN;
            }
            8 => s.lens_blur = Some(LensBlur::default()),
            9 => crop.angle = f32::INFINITY,
            10 => crop.rect.right = 0.,
            11 => e.width = 0,
            12 => e.height = 0,
            13 => tile.samples_mut::<f32>().unwrap()[0] = f32::INFINITY,
            14 => tile = Tile::from_samples(TileCoord::new(0, 1, 0), l, vec![0.5f32; 6]).unwrap(),
            15 => {
                tile = Tile::from_samples(
                    TileCoord::new(0, 0, 0),
                    TileLayout { channels: 1, ..l },
                    vec![0.5f32; 2],
                )
                .unwrap()
            }
            _ => tile = Tile::from_samples(TileCoord::new(0, 0, 0), l, vec![1u8; 6]).unwrap(),
        }
        let cpu = image_core::CpuStageOp
            .run(
                StageId::Tone,
                &Op::EffectsInCrop(&s, e, &crop),
                tile.clone(),
            )
            .unwrap_err();
        let gpu = effects::run(&ctx, &tile, &s, e, &crop).unwrap_err();
        assert_eq!(gpu.to_string(), cpu.to_string(), "case {case}");
    }
}
#[test]
fn isolated_seams_are_bit_exact() {
    let ctx = GpuContext::new().unwrap();
    let image = pipeline_cpu::Image::new(520, 17, vec![vec![0.5; 520 * 17]; 3]).unwrap();
    let mut s = EffectsSettings::default();
    s.grain.amount = 100.;
    s.vignette.amount = -55.;
    let crop = Crop {
        angle: -27.,
        ..Default::default()
    };
    let left = compare(
        &ctx,
        &image.tile(TileCoord::new(0, 0, 0), 2, 1).unwrap(),
        &s,
        Extent::new(520, 17),
        &crop,
    );
    let right = compare(
        &ctx,
        &image.tile(TileCoord::new(0, 1, 0), 2, 1).unwrap(),
        &s,
        Extent::new(520, 17),
        &crop,
    );
    for y in 0..17 {
        for x in -2..2 {
            for c in 0..3 {
                assert_eq!(
                    left.samples::<f32>().unwrap()[left.layout().index(c, 256 + x, y).unwrap()]
                        .to_bits(),
                    right.samples::<f32>().unwrap()[right.layout().index(c, x, y).unwrap()]
                        .to_bits()
                );
            }
        }
    }
}

#[test]
fn isolated_hash_exact_u64_pairs() {
    use wgpu::util::DeviceExt;
    let ctx = GpuContext::new().unwrap();
    let values = [
        0i64,
        1,
        -1,
        19,
        -71,
        i32::MAX as i64,
        i32::MIN as i64,
        1i64 << 34,
        -(1i64 << 40),
        i64::MAX,
        i64::MIN,
    ];
    let mut input = Vec::<u32>::new();
    let mut expected = Vec::<u32>::new();
    for x in values {
        for y in values {
            input.extend([
                x as u32,
                (x as u64 >> 32) as u32,
                y as u32,
                (y as u64 >> 32) as u32,
            ]);
            let mut z = (x as u64).wrapping_mul(0x9e3779b97f4a7c15)
                ^ (y as u64).wrapping_mul(0xbf58476d1ce4e5b9)
                ^ 0x5445535345524132;
            z = (z ^ (z >> 30)).wrapping_mul(0xbf58476d1ce4e5b9);
            z = (z ^ (z >> 27)).wrapping_mul(0x94d049bb133111eb);
            z ^= z >> 31;
            expected.extend([z as u32, (z >> 32) as u32]);
        }
    }
    let source = format!(
        "{}\n{}",
        include_str!("../src/effects.wgsl"),
        r#"
 @compute @workgroup_size(1) fn hash_test(@builtin(global_invocation_id) id:vec3<u32>){
 let i=id.x;let z=hash64(vec2(src[4u*i],src[4u*i+1u]),vec2(src[4u*i+2u],src[4u*i+3u]));dst[2u*i]=z.x;dst[2u*i+1u]=z.y;
 }"#
    );
    let shader = ctx
        .device
        .create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("hash test"),
            source: wgpu::ShaderSource::Wgsl(source.into()),
        });
    let pipeline = ctx
        .device
        .create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: None,
            layout: None,
            module: &shader,
            entry_point: Some("hash_test"),
            compilation_options: Default::default(),
            cache: None,
        });
    let src = ctx
        .device
        .create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: None,
            contents: bytemuck::cast_slice(&input),
            usage: wgpu::BufferUsages::STORAGE,
        });
    let size = (expected.len() * 4) as u64;
    let buffer = |usage| {
        ctx.device.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size,
            usage,
            mapped_at_creation: false,
        })
    };
    let dst = buffer(wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC);
    let staging = buffer(wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST);
    let entries = [
        wgpu::BindGroupEntry {
            binding: 0,
            resource: src.as_entire_binding(),
        },
        wgpu::BindGroupEntry {
            binding: 1,
            resource: dst.as_entire_binding(),
        },
    ];
    let group = ctx.device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: None,
        layout: &pipeline.get_bind_group_layout(0),
        entries: &entries,
    });
    let mut encoder = ctx.device.create_command_encoder(&Default::default());
    {
        let mut pass = encoder.begin_compute_pass(&Default::default());
        pass.set_pipeline(&pipeline);
        pass.set_bind_group(0, &group, &[]);
        pass.dispatch_workgroups((expected.len() / 2) as u32, 1, 1);
    }
    encoder.copy_buffer_to_buffer(&dst, 0, &staging, 0, size);
    ctx.queue.submit([encoder.finish()]);
    let (tx, rx) = std::sync::mpsc::channel();
    staging
        .slice(..)
        .map_async(wgpu::MapMode::Read, move |r| tx.send(r).unwrap());
    ctx.device
        .poll(wgpu::PollType::wait_indefinitely())
        .unwrap();
    rx.recv().unwrap().unwrap();
    {
        let mapped = staging.slice(..).get_mapped_range().unwrap();
        assert_eq!(bytemuck::cast_slice::<u8, u32>(&mapped), expected);
    }
    staging.unmap();
}
#[test]
fn isolated_extremes_finite_and_clamped_controls() {
    let ctx = GpuContext::new().unwrap();
    let l = TileLayout {
        extent: Extent::new(2, 2),
        halo: 0,
        channels: 3,
    };
    let tile = Tile::from_samples(
        TileCoord::new(0, 0, 0),
        l,
        vec![
            -f32::MAX,
            f32::MAX,
            -1.,
            4.,
            f32::MAX,
            -f32::MAX,
            0.,
            2.,
            -f32::MAX,
            f32::MAX,
            1.,
            -2.,
        ],
    )
    .unwrap();
    for style in [
        VignetteStyle::HighlightPriority,
        VignetteStyle::ColorPriority,
        VignetteStyle::PaintOverlay,
    ] {
        for amount in [-100., 100.] {
            let mut s = EffectsSettings::default();
            s.vignette.amount = amount;
            s.vignette.style = style;
            s.vignette.feather = 0.;
            s.vignette.midpoint = 0.;
            s.grain.amount = 100.;
            let out = effects::run(&ctx, &tile, &s, Extent::new(2, 2), &Crop::default()).unwrap();
            assert!(out.samples::<f32>().unwrap().iter().all(|x| x.is_finite()));
        }
    }
    let tile = Tile::from_samples(TileCoord::new(0, 0, 0), l, vec![0.3f32; 12]).unwrap();
    let mut s = EffectsSettings::default();
    s.vignette.amount = -200.;
    s.vignette.roundness = 300.;
    s.vignette.midpoint = -10.;
    s.vignette.feather = 200.;
    s.vignette.highlights = 400.;
    s.grain.amount = 300.;
    s.grain.size = -3.;
    s.grain.roughness = 400.;
    let crop = Crop {
        angle: 91.,
        aspect: Some([0, 0]),
        ..Default::default()
    };
    let out = compare(&ctx, &tile, &s, Extent::new(2, 2), &crop);
    let repeat = effects::run(&ctx, &tile, &s, Extent::new(2, 2), &crop).unwrap();
    assert_eq!(
        out.samples::<f32>().unwrap(),
        repeat.samples::<f32>().unwrap()
    );
}
