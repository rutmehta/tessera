#![cfg(feature = "camera-raw-filter")]
use compositor::{
    Rect,
    raster::{Depth, Raster},
    render::smart_filters::FilterContext,
};
use engine_api::recipe::{DevelopSettings, settings::LensProfileSource};
use engine_api::tile::Extent;
use filters::camera_raw_gpu;
use serde_json::json;
use wgpu::util::DeviceExt;

fn pixels(extent: Extent) -> (Raster, Vec<[f32; 4]>) {
    let data: Vec<_> = (0..extent.area())
        .map(|i| {
            let x = i % u64::from(extent.width);
            let y = i / u64::from(extent.width);
            [
                ((x * 7 + y * 11) % 257) as f32 / 256.,
                ((x * 3 + y * 19) % 251) as f32 / 250.,
                ((x * 13 + y * 5) % 241) as f32 / 240.,
                (x % 5) as f32 / 4.,
            ]
        })
        .collect();
    let mut raster = Raster::new(extent, 4, Depth::F32, 0.);
    raster
        .edit_region(Rect::of_extent(extent), 1, |x, y, p| {
            *p = data[(y * extent.width + x) as usize];
        })
        .unwrap();
    (raster, data)
}

// Readback is ONLY in tests, never in the evaluator.
fn readback(gpu: &pipeline_gpu::GpuContext, buffer: &wgpu::Buffer, size: u64) -> Vec<[f32; 4]> {
    let staging = gpu.device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("camera raw test readback"),
        size,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let mut encoder = gpu.device.create_command_encoder(&Default::default());
    encoder.copy_buffer_to_buffer(buffer, 0, &staging, 0, size);
    gpu.queue.submit([encoder.finish()]);
    let (tx, rx) = std::sync::mpsc::channel();
    staging
        .slice(..)
        .map_async(wgpu::MapMode::Read, move |v| tx.send(v).unwrap());
    gpu.device
        .poll(wgpu::PollType::wait_indefinitely())
        .unwrap();
    rx.recv().unwrap().unwrap();
    let result = bytemuck::cast_slice(&staging.slice(..).get_mapped_range().unwrap()).to_vec();
    staging.unmap();
    result
}

// Independent whole-image scalar pipeline, not a second GPU call or a mock.
fn cpu_reference(input: &Raster, value: &serde_json::Value, context: &FilterContext) -> Raster {
    let p = filters::camera_raw::parse(value).unwrap();
    let (forward, backward) = filters::camera_raw::profile_matrices(context).unwrap();
    // B5-28: samples are encoded in the document profile's transfer curve.
    let curves = filters::camera_raw::profile_curves(context).unwrap();
    if p.amount == 0. {
        return input.clone();
    }
    let e = input.extent();
    let mut planes: Vec<_> = (0..3)
        .map(|_| Vec::with_capacity(e.area() as usize))
        .collect();
    for y in 0..e.height {
        for x in 0..e.width {
            let pixel = input.pixel(x, y);
            let linear = curves.decode([pixel[0], pixel[1], pixel[2]]);
            let rgb = forward.apply(linear.map(f64::from));
            for c in 0..3 {
                planes[c].push(rgb[c] as f32);
            }
        }
    }
    let image = pipeline_cpu::Image::new(e.width, e.height, planes).unwrap();
    let developed = pipeline_cpu::render_linear_scaled(
        &p.settings,
        &pipeline_cpu::RenderSource::Rgb(&image),
        1,
    )
    .unwrap();
    let mut out = input.clone();
    out.edit_region(Rect::of_extent(e), 1, |x, y, pixel| {
        let rgb = if x < developed.width() && y < developed.height() {
            let i = (y * developed.width() + x) as usize;
            backward.apply(std::array::from_fn(|c| developed.planes()[c][i] as f64))
        } else {
            [0.; 3]
        };
        let rgb = curves.encode(rgb.map(|v| v as f32));
        for c in 0..3 {
            pixel[c] += p.amount * (rgb[c] - pixel[c]);
        }
    })
    .unwrap();
    out
}

fn rich_settings() -> DevelopSettings {
    use engine_api::recipe::settings::{Curve, CurvePoint, WhiteBalanceMode};
    let mut s = resident_settings();
    s.white_balance.mode = WhiteBalanceMode::Custom;
    s.white_balance.temperature = 6100.;
    s.white_balance.tint = 11.;
    s.detail.sharpening.amount = 57.;
    s.detail.noise_reduction.luminance = 17.;
    s.tone.exposure = 0.3;
    s.tone.contrast = 11.;
    s.tone.highlights = -17.;
    s.tone.shadows = 9.;
    s.tone.whites = 5.;
    s.tone.blacks = -3.;
    s.tone.texture = 12.;
    s.tone.clarity = 8.;
    s.tone.dehaze = 9.;
    s.tone.curves.rgb = Curve(vec![
        CurvePoint { x: 0., y: 0. },
        CurvePoint { x: 0.5, y: 0.55 },
        CurvePoint { x: 1., y: 1. },
    ]);
    s.color.saturation = -6.;
    s.color.vibrance = 19.;
    s.color.hsl.hue.red = 13.;
    s.effects.vignette.amount = -12.;
    s.effects.grain.amount = 8.;
    s.locals
        .adjustments
        .push(engine_api::recipe::LocalAdjustment {
            components: vec![engine_api::recipe::MaskComponent::new(
                engine_api::recipe::MaskKind::Linear {
                    start: [0., 0.],
                    end: [1., 1.],
                },
            )],
            params: engine_api::recipe::mask::LocalParams {
                exposure: 0.3,
                saturation: 7.,
                ..Default::default()
            },
            ..Default::default()
        });
    s
}

// A single input ulp must not become a visible jump after sharpen + presence.
#[test]
fn signed_luminance_chain_is_stable_under_one_input_ulp() {
    let extent = Extent::new(259, 263);
    let (raster, _) = pixels(extent);
    let context = FilterContext {
        profile: None,
        level: 2,
        canvas: extent,
    };
    let value = json!({"settings": rich_settings()});
    let original = cpu_reference(&raster, &value, &context).pixel(84, 41)[2];
    let mut perturbed = raster.clone();
    perturbed
        .edit_region(Rect::of_extent(extent), 1, |x, y, p| {
            if (x, y) == (84, 41) {
                p[2] = p[2].next_up();
            }
        })
        .unwrap();
    let changed = cpu_reference(&perturbed, &value, &context).pixel(84, 41)[2];
    let gap = (changed - original).abs();
    eprintln!("one input blue ulp: {original} -> {changed}, gap={gap}");
    assert!(gap < 0.01, "ill-conditioned presence gain: {gap}");
}

#[test]
fn resident_chain_matches_cpu_across_tile_edges_and_preserves_alpha() {
    let gpu = pipeline_gpu::GpuContext::new().unwrap();
    let extent = Extent::new(259, 263);
    let (raster, data) = pixels(extent);
    let context = FilterContext {
        profile: None,
        level: 2,
        canvas: extent,
    };
    let buffer = gpu
        .device
        .create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: None,
            contents: bytemuck::cast_slice(&data),
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
        });
    let mut failures = Vec::new();
    for (amount, presence) in [(0., true), (0.35, true), (1., true), (1., false)] {
        let mut settings = rich_settings();
        if !presence {
            settings.tone.texture = 0.;
            settings.tone.clarity = 0.;
        }
        let value = json!({"settings": settings, "amount": amount});
        let cpu = cpu_reference(&raster, &value, &context);
        let output =
            camera_raw_gpu::evaluate(&gpu.device, &gpu.queue, &buffer, extent, &value, &context)
                .unwrap();
        let actual = readback(&gpu, &output, extent.area() * 16);
        // Retain both the scaled chain bound and absolute ceiling.
        let mut max_absolute = 0.0_f32;
        let mut max_scaled = 0.0_f32;
        for (i, p) in actual.iter().enumerate() {
            let expected = cpu.pixel(i as u32 % extent.width, i as u32 / extent.width);
            assert_eq!(p[3].to_bits(), data[i][3].to_bits());
            for c in 0..3 {
                assert!(p[c].is_finite());
                assert!(expected[c].is_finite());
                let gap = (p[c] - expected[c]).abs();
                max_absolute = max_absolute.max(gap);
                max_scaled = max_scaled.max(gap / expected[c].abs().max(1.));
                if amount == 0. {
                    assert_eq!(p[c].to_bits(), data[i][c].to_bits());
                }
            }
        }
        let target_cpu = cpu.pixel(84, 41);
        eprintln!(
            "amount {amount}, presence {presence}: max absolute {max_absolute}, max scaled {max_scaled}; pixel 10703 GPU {:?} CPU {:?}",
            actual[10703], target_cpu
        );
        if max_absolute >= 0.01 || max_scaled >= 0.002 {
            failures.push((amount, presence, max_absolute, max_scaled));
        }
    }
    assert!(
        failures.is_empty(),
        "chain absolute < 0.01 / scaled < 0.002: {failures:?}"
    );
}

/// Compare individual stages with identical sRGB primaries and either the
/// B5-28 transfer curve or its linear twin. Collect all cases before asserting
/// so a failure still identifies which stage/curve combination first diverges.
/// Every rich-chain stage asserts the conditioned absolute 0.01 bound.
#[test]
fn bright_value_stage_isolation_with_and_without_profile_curve() {
    use color_mgmt::{Builtin, Registry};
    use compositor::document::ColorProfile;
    let gpu = pipeline_gpu::GpuContext::new().unwrap();
    let extent = Extent::new(259, 263);
    let (raster, data) = pixels(extent);
    let input = upload(&gpu, &data);
    let mut registry = Registry::new();
    let srgb = registry.builtin(Builtin::Srgb).unwrap();
    let linear = registry.linearized_rgb(&srgb).unwrap().unwrap();
    let mut failures = Vec::new();
    for curve in [false, true] {
        let context = FilterContext {
            profile: Some(ColorProfile::from_icc(
                "sRGB stage isolation",
                if curve {
                    srgb.icc_bytes()
                } else {
                    linear.icc_bytes()
                }
                .to_vec(),
            )),
            level: 0,
            canvas: extent,
        };
        assert_eq!(
            filters::camera_raw::profile_curves(&context)
                .unwrap()
                .is_identity(),
            !curve
        );
        for stage in [
            "neutral",
            "sharpen",
            "dehaze",
            "sharpen+dehaze",
            "rich-no-sharpen",
            "rich-no-dehaze",
            "rich-no-presence",
            "rich",
        ] {
            let mut settings = resident_settings();
            settings.detail.sharpening.amount = 0.;
            settings.detail.noise_reduction.color = 0.;
            if stage.contains("sharpen") {
                settings.detail.sharpening.amount = 57.;
            }
            if stage.contains("dehaze") {
                settings.tone.dehaze = 9.;
            }
            if stage.starts_with("rich") {
                settings = rich_settings();
                if stage == "rich-no-sharpen" {
                    settings.detail.sharpening.amount = 0.;
                }
                if stage == "rich-no-presence" {
                    settings.tone.texture = 0.;
                    settings.tone.clarity = 0.;
                }
                if stage == "rich-no-dehaze" {
                    settings.tone.dehaze = 0.;
                }
            }
            let value = json!({"settings": settings});
            let cpu = cpu_reference(&raster, &value, &context);
            let output =
                camera_raw_gpu::evaluate(&gpu.device, &gpu.queue, &input, extent, &value, &context)
                    .unwrap();
            let actual = readback(&gpu, &output, extent.area() * 16);
            let mut worst = (0f32, 0usize, 0usize, 0f32, 0f32);
            for (i, p) in actual.iter().enumerate() {
                let expected = cpu.pixel(i as u32 % extent.width, i as u32 / extent.width);
                assert_eq!(p[3].to_bits(), data[i][3].to_bits());
                for c in 0..3 {
                    assert!(p[c].is_finite());
                    let gap = (p[c] - expected[c]).abs();
                    if gap > worst.0 {
                        worst = (gap, i, c, p[c], expected[c]);
                    }
                }
            }
            eprintln!(
                "curve={curve} stage={stage}: max={} pixel={} channel={} GPU={} CPU={}",
                worst.0, worst.1, worst.2, worst.3, worst.4
            );
            // Standalone stages meet this bound on unchanged main numerics.
            // Rich combinations retain the absolute resident-chain ceiling.
            let bound = if stage.starts_with("rich") {
                0.01
            } else {
                5e-6
            };
            if worst.0 >= bound {
                failures.push(format!("curve={curve} stage={stage}: {worst:?}"));
            }
        }
    }
    assert!(
        failures.is_empty(),
        "stage parity failures (standalone 5e-6, rich 0.01): {failures:#?}"
    );
}

#[test]
fn display_p3_signed_hdr_and_transparent_rgb_match_cpu() {
    use color_mgmt::{Builtin, Registry};
    use compositor::document::ColorProfile;
    let gpu = pipeline_gpu::GpuContext::new().unwrap();
    let extent = Extent::new(17, 9);
    let mut registry = Registry::new();
    let profile = registry.builtin(Builtin::DisplayP3).unwrap();
    let context = FilterContext {
        profile: Some(ColorProfile::from_icc(
            "Display P3",
            profile.icc_bytes().to_vec(),
        )),
        level: 0,
        canvas: extent,
    };
    let data: Vec<[f32; 4]> = (0..extent.area())
        .map(|i| [-0.15 + (i % 7) as f32 / 10., 1.7, 0.2, (i % 4) as f32 / 3.])
        .collect();
    let mut raster = Raster::new(extent, 4, Depth::F32, 0.);
    raster
        .edit_region(Rect::of_extent(extent), 1, |x, y, p| {
            *p = data[(y * extent.width + x) as usize]
        })
        .unwrap();
    let buffer = gpu
        .device
        .create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: None,
            contents: bytemuck::cast_slice(&data),
            usage: wgpu::BufferUsages::STORAGE,
        });
    let mut settings = resident_settings();
    settings.detail.sharpening.amount = 0.;
    settings.detail.noise_reduction.color = 0.;
    let value = json!({"settings": settings, "amount": 0.7});
    let expected = cpu_reference(&raster, &value, &context);
    let output =
        camera_raw_gpu::evaluate(&gpu.device, &gpu.queue, &buffer, extent, &value, &context)
            .unwrap();
    let actual = readback(&gpu, &output, extent.area() * 16);
    for (i, p) in actual.iter().enumerate() {
        let reference = expected.pixel(i as u32 % extent.width, i as u32 / extent.width);
        assert_eq!(p[3].to_bits(), data[i][3].to_bits());
        for c in 0..3 {
            assert!(
                (p[c] - reference[c]).abs() < 0.0001,
                "{i}/{c}: {p:?} vs {reference:?}"
            );
        }
    }
    assert!(actual.iter().any(|p| p[0] < 0.));
    assert!(actual.iter().any(|p| p[1] > 1.));
    assert!(actual.iter().any(|p| p[3] == 0. && p[1] > 1.));
}

#[test]
fn manual_optics_crop_and_straighten_match_cpu_with_canvas_padding() {
    let gpu = pipeline_gpu::GpuContext::new().unwrap();
    let extent = Extent::new(71, 59);
    let (raster, data) = pixels(extent);
    let context = FilterContext {
        profile: None,
        level: 0,
        canvas: extent,
    };
    let mut s = rich_settings();
    s.lens.manual_distortion = 7.;
    s.lens.manual_vignetting = 17.;
    s.geometry.crop.rect = engine_api::recipe::settings::NormalizedRect {
        left: 0.1,
        top: 0.1,
        right: 0.9,
        bottom: 0.9,
    };
    s.geometry.crop.angle = 3.;
    let value = json!({"settings": s, "amount": 0.65});
    let input = gpu
        .device
        .create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: None,
            contents: bytemuck::cast_slice(&data),
            usage: wgpu::BufferUsages::STORAGE,
        });
    let expected = cpu_reference(&raster, &value, &context);
    let output =
        camera_raw_gpu::evaluate(&gpu.device, &gpu.queue, &input, extent, &value, &context)
            .unwrap();
    let actual = readback(&gpu, &output, extent.area() * 16);
    for (i, pixel) in actual.iter().enumerate() {
        let reference = expected.pixel(i as u32 % extent.width, i as u32 / extent.width);
        assert_eq!(pixel[3], data[i][3]);
        for c in 0..3 {
            assert!(
                (pixel[c] - reference[c]).abs() <= 0.002,
                "{i}/{c}: {pixel:?} vs {reference:?}"
            );
        }
    }
}

#[test]
fn invalid_buffer_and_unsupported_settings_fail_without_dispatch() {
    let gpu = pipeline_gpu::GpuContext::new().unwrap();
    let extent = Extent::new(8, 8);
    let context = FilterContext {
        profile: None,
        level: 0,
        canvas: extent,
    };
    let short = gpu.device.create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size: 16,
        usage: wgpu::BufferUsages::STORAGE,
        mapped_at_creation: false,
    });
    let value = json!({"settings": resident_settings()});
    assert!(
        camera_raw_gpu::evaluate(&gpu.device, &gpu.queue, &short, extent, &value, &context)
            .is_err()
    );
    assert!(
        camera_raw_gpu::evaluate(
            &gpu.device,
            &gpu.queue,
            &short,
            Extent::new(0, 0),
            &value,
            &context
        )
        .is_err()
    );
    let value = json!({"settings": DevelopSettings::default(), "amount": 0});
    assert!(matches!(
        camera_raw_gpu::evaluate(&gpu.device, &gpu.queue, &short, extent, &value, &context),
        Err(engine_api::EngineError::Unsupported { .. })
    ));
}

/// Bound the existing tone gap and check presence does not increase it.
/// Keep the original sampled scaled guard as an additional regression check.
/// Unconditioned presence had a baseline full-frame maximum of 6.14.
#[test]
#[ignore = "24MP CPU/GPU timing; run explicitly in release on Metal"]
fn bench_24mp_cpu_gpu() {
    let gpu = pipeline_gpu::GpuContext::new().unwrap();
    eprintln!("24MP adapter={:?}", gpu.adapter_info);
    let extent = Extent::new(6000, 4000);
    let (raster, data) = pixels(extent);
    let context = FilterContext {
        profile: None,
        level: 0,
        canvas: extent,
    };
    let buffer = gpu
        .device
        .create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: None,
            contents: bytemuck::cast_slice(&data),
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
        });
    gpu.queue.submit([]);
    gpu.device
        .poll(wgpu::PollType::wait_indefinitely())
        .unwrap();
    let measure = |presence: &str, settings: DevelopSettings| {
        let value = json!({"settings": settings});
        let start = std::time::Instant::now();
        let cpu = cpu_reference(&raster, &value, &context);
        let cpu_time = start.elapsed();
        let start = std::time::Instant::now();
        let output =
            camera_raw_gpu::evaluate(&gpu.device, &gpu.queue, &buffer, extent, &value, &context)
                .unwrap();
        gpu.device
            .poll(wgpu::PollType::wait_indefinitely())
            .unwrap();
        let gpu_time = start.elapsed();
        eprintln!(
            "24MP presence={presence} CPU={cpu_time:?} GPU={gpu_time:?} (cold pipelines included, upload/readback excluded)"
        );
        let actual = readback(&gpu, &output, extent.area() * 16);
        let mut max_absolute = 0f32;
        let mut worst = (0usize, 0usize, 0f32, 0f32);
        for (i, pixel) in actual.iter().enumerate() {
            let expected = cpu.pixel(i as u32 % extent.width, i as u32 / extent.width);
            assert_eq!(pixel[3].to_bits(), data[i][3].to_bits());
            assert_eq!(expected[3].to_bits(), data[i][3].to_bits());
            for c in 0..3 {
                assert!(pixel[c].is_finite());
                assert!(expected[c].is_finite());
                let gap = (pixel[c] - expected[c]).abs();
                if i % 997 == 0 {
                    let scale = expected[c].abs().max(1.);
                    assert!(gap / scale < 0.002, "{i}/{c}: {pixel:?} vs {expected:?}");
                }
                if gap > max_absolute {
                    max_absolute = gap;
                    worst = (i, c, pixel[c], expected[c]);
                }
            }
        }
        eprintln!(
            "24MP presence={presence} all-pixel max absolute={max_absolute}, worst={worst:?}"
        );
        eprintln!(
            "24MP presence={presence} target pixel 4999168 GPU {:?} CPU {:?}",
            actual[4999168],
            cpu.pixel(1168, 833)
        );
        max_absolute
    };
    let mut settings = rich_settings();
    settings.tone.texture = 0.;
    settings.tone.clarity = 0.;
    let presence_off = measure("off", settings);
    let presence_on = measure("on", rich_settings());
    assert!(
        presence_off < 0.01,
        "24MP presence-off RGB error {presence_off}"
    );
    assert!(
        presence_on < 0.01,
        "24MP presence-on RGB error {presence_on}"
    );
}

fn resident_settings() -> DevelopSettings {
    let mut settings = DevelopSettings::default();
    settings.lens.profile = LensProfileSource::None;
    settings.lens.remove_chromatic_aberration = false;
    settings
}

#[test]
fn capability_rejects_pixel_analysis_and_unported_barriers() {
    assert!(camera_raw_gpu::supports(&json!({"settings": resident_settings()})).unwrap());
    // Default Auto lens calibration examines pixels on the CPU, even for RGB.
    assert!(!camera_raw_gpu::supports(&json!({"settings": DevelopSettings::default()})).unwrap());
    let mut settings = resident_settings();
    settings.lens.manual_distortion = 12.;
    assert!(camera_raw_gpu::supports(&json!({"settings": settings})).unwrap());
    assert!(camera_raw_gpu::supports(&json!({"settings": {}, "amount": -1})).is_err());
    let mut settings = resident_settings();
    settings.tone.dehaze = 1.;
    assert!(camera_raw_gpu::supports(&json!({"settings": settings})).unwrap());
    let mut settings = resident_settings();
    settings
        .locals
        .adjustments
        .push(engine_api::recipe::LocalAdjustment {
            components: vec![engine_api::recipe::MaskComponent::new(
                engine_api::recipe::MaskKind::Linear {
                    start: [0., 0.],
                    end: [1., 1.],
                },
            )],
            ..Default::default()
        });
    assert!(camera_raw_gpu::supports(&json!({"settings": settings})).unwrap());
}

#[test]
fn documented_rgb_exclusions_remain_explicit() {
    // These are engineering gaps on RGB, not missing CFA data.
    let base = serde_json::to_value(resident_settings()).unwrap();
    for (pointer, value) in [
        ("/lens/profile", json!({"kind":"auto"})),
        ("/lens/profile", json!({"kind":"auto_calibrated"})),
        ("/lens/remove_chromatic_aberration", json!(true)),
        ("/lens/defringe_purple/amount", json!(20)),
        ("/lens/defringe_green/amount", json!(20)),
        ("/geometry/orientation", json!(6)),
        ("/geometry/constrain_crop", json!(true)),
        ("/geometry/upright/mode", json!("auto")),
    ] {
        let mut settings = base.clone();
        *settings.pointer_mut(pointer).unwrap() = value;
        assert!(
            !camera_raw_gpu::supports(&json!({"settings": settings})).unwrap(),
            "{pointer}"
        );
    }
    // No supplied depth/profile/model binding: engine rejects these controls.
    for (pointer, value) in [
        ("/effects/lens_blur", json!({"amount": 20})),
        ("/lens/softness_correction", json!(10)),
    ] {
        let mut settings = base.clone();
        *settings.pointer_mut(pointer).unwrap() = value;
        assert!(
            camera_raw_gpu::supports(&json!({"settings": settings})).is_err(),
            "{pointer}"
        );
    }
}

fn upload(gpu: &pipeline_gpu::GpuContext, data: &[[f32; 4]]) -> wgpu::Buffer {
    gpu.device
        .create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: None,
            contents: bytemuck::cast_slice(data),
            usage: wgpu::BufferUsages::STORAGE,
        })
}

fn profile_context(builtin: Option<color_mgmt::Builtin>, extent: Extent) -> FilterContext {
    use compositor::document::ColorProfile;
    FilterContext {
        profile: builtin.map(|b| {
            let icc = color_mgmt::Registry::new().builtin(b).unwrap();
            ColorProfile::from_icc("working", icc.icc_bytes().to_vec())
        }),
        level: 0,
        canvas: extent,
    }
}

/// B5-28: the resident path decodes encoded samples like the CPU path. An
/// untagged document is sRGB-encoded: +1 EV on 0.5 is 0.6858, not 1.0.
#[test]
fn resident_plus_one_ev_doubles_decoded_light_of_srgb_encoded_mid_grey() {
    let gpu = pipeline_gpu::GpuContext::new().unwrap();
    let extent = Extent::new(16, 16);
    let data = vec![[0.5, 0.5, 0.5, 1.]; extent.area() as usize];
    let mut s = resident_settings();
    s.detail.sharpening.amount = 0.;
    s.detail.noise_reduction.color = 0.;
    s.tone.exposure = 1.;
    let value = json!({"settings": s});
    for builtin in [None, Some(color_mgmt::Builtin::Srgb)] {
        let context = profile_context(builtin, extent);
        let output = camera_raw_gpu::evaluate(
            &gpu.device,
            &gpu.queue,
            &upload(&gpu, &data),
            extent,
            &value,
            &context,
        )
        .unwrap();
        let p = readback(&gpu, &output, extent.area() * 16)[40];
        assert!((p[1] - 0.6858).abs() < 0.002, "{builtin:?}: {p:?}");
    }
}

/// CPU evaluator and resident GPU agree on ENCODED input for gamma (Adobe RGB),
/// sRGB-curve (Display P3, untagged) and linear profiles, including signed
/// and >1 float samples on both sides of the transfer-curve extension.
#[test]
fn resident_matches_cpu_evaluator_on_encoded_samples_across_profiles() {
    use color_mgmt::Builtin;
    let gpu = pipeline_gpu::GpuContext::new().unwrap();
    let extent = Extent::new(37, 29);
    let (mut raster, mut data) = pixels(extent);
    for (i, p) in data.iter_mut().enumerate().step_by(9) {
        p[0] = -0.1 - (i % 3) as f32 * 0.05;
        p[2] = 1.2 + (i % 5) as f32 * 0.1;
    }
    raster
        .edit_region(Rect::of_extent(extent), 2, |x, y, p| {
            *p = data[(y * extent.width + x) as usize];
        })
        .unwrap();
    let input = upload(&gpu, &data);
    let mut s = rich_settings();
    s.detail.sharpening.amount = 0.;
    s.detail.noise_reduction.luminance = 0.;
    for builtin in [
        None,
        Some(Builtin::DisplayP3),
        Some(Builtin::AdobeRgb),
        Some(Builtin::LinearRec2020),
    ] {
        let context = profile_context(builtin, extent);
        let value = json!({"settings": s, "amount": 0.8});
        let cpu = filters::camera_raw::evaluate(&raster, &value, &context).unwrap();
        let output =
            camera_raw_gpu::evaluate(&gpu.device, &gpu.queue, &input, extent, &value, &context)
                .unwrap();
        let actual = readback(&gpu, &output, extent.area() * 16);
        let mut max_error = 0f32;
        for (i, p) in actual.iter().enumerate() {
            let expected = cpu.pixel(i as u32 % extent.width, i as u32 / extent.width);
            assert_eq!(p[3].to_bits(), data[i][3].to_bits());
            for c in 0..3 {
                assert!(p[c].is_finite());
                max_error = max_error.max((p[c] - expected[c]).abs());
            }
        }
        eprintln!("{builtin:?}: max |GPU - CPU| {max_error}");
        assert!(max_error < 0.002, "{builtin:?}: {max_error}");
    }
}
