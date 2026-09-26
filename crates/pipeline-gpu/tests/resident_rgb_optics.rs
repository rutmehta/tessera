use engine_api::{
    jobs::CancellationToken,
    tile::{Tile, TileCoord, TileLayout},
};
use engine_api::{
    recipe::{DevelopSettings, settings::LensProfileSource},
    tile::Extent,
};
use image_core::StageOp;
use pipeline_gpu::resident_rgb_optics::RgbOpticsPlan;
use pipeline_gpu::{GpuContext, GpuStageOp};
use std::sync::Arc;

#[test]
fn manual_gain_and_geometry_stay_resident_and_match_reference() {
    let frame = Extent::new(31, 23);
    let mut s = DevelopSettings::default();
    s.lens.profile = LensProfileSource::None;
    s.lens.remove_chromatic_aberration = false;
    s.lens.manual_vignetting = 27.;
    s.geometry.crop.rect = engine_api::recipe::settings::NormalizedRect {
        left: 0.1,
        top: 0.15,
        right: 0.9,
        bottom: 0.85,
    };
    s.geometry.crop.angle = 4.;
    let p = RgbOpticsPlan::new(&s, frame, None).unwrap().unwrap();
    let samples: Vec<f32> = (0..frame.area() as usize * 3)
        .map(|i| 0.4 + 0.2 * (i as f32 * 0.1).sin())
        .collect();
    let mut planes: Vec<Vec<f32>> = samples
        .chunks(frame.area() as usize)
        .map(|p| p.to_vec())
        .collect();
    for y in 0..frame.height {
        for x in 0..frame.width {
            let px = 2. * (x as f64 + 0.5) / frame.width as f64 - 1.;
            let py = 2. * (y as f64 + 0.5) / frame.height as f64 - 1.;
            let gain = (27. / 50. * ((px * px + py * py) / 2.).powf(0.25 + 3.75 * 0.5)).exp2();
            for c in &mut planes {
                c[(y * frame.width + x) as usize] =
                    (c[(y * frame.width + x) as usize] as f64 * gain) as f32;
            }
        }
    }
    let cpu = pipeline_cpu::geometry(
        &pipeline_cpu::Image::new(frame.width, frame.height, planes).unwrap(),
        &s.geometry,
    )
    .unwrap();
    let ctx = Arc::new(GpuContext::new().unwrap());
    let gpu = GpuStageOp::new(ctx.clone());
    let mut b = gpu.begin_resident().unwrap();
    let input = Tile::from_samples(
        TileCoord::new(0, 0, 0),
        TileLayout {
            extent: frame,
            halo: 0,
            channels: 3,
        },
        samples,
    )
    .unwrap();
    use wgpu::util::DeviceExt;
    let buffer = ctx
        .device
        .create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("shared RGB"),
            contents: bytemuck::cast_slice(input.samples::<f32>().unwrap()),
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
        });
    let t = ctx
        .import_resident_buffer(input.coord(), input.layout(), &buffer)
        .unwrap();
    let t = p.before_white_balance(b.as_mut(), &t).unwrap();
    let t = p.after_white_balance(b.as_mut(), &t).unwrap();
    let t = p.after_effects(b.as_mut(), &t).unwrap();
    assert_eq!(t.layout.extent, p.output_extent());
    let guard = ctx.resident_buffer(&t).unwrap();
    let done = b
        .finish(vec![], false, None, &CancellationToken::new())
        .unwrap();
    assert!(done.tiles.is_empty());
    assert!(!guard.packed());
    let b = gpu.begin_resident().unwrap();
    let out = b
        .finish(vec![t], false, None, &CancellationToken::new())
        .unwrap();
    let expected: Vec<_> = cpu.planes().iter().flatten().copied().collect();
    let error = out.tiles[0]
        .samples::<f32>()
        .unwrap()
        .iter()
        .zip(expected)
        .map(|(a, b)| (a - b).abs())
        .fold(0., f32::max);
    assert!(error < 2e-5, "max error {error}");
}

#[test]
fn resolved_ca_and_profile_gain_match_scalar_coordinates() {
    let frame = Extent::new(19, 13);
    let s = DevelopSettings::default();
    let sample = lens::CalibrationSample {
        ca_red: [1.012, 0.004, 0.],
        ca_blue: [0.988, -0.003, 0.],
        vignette: [-0.12, 0.02, 0.],
        ..Default::default()
    };
    let resolved = pipeline_cpu::ResolvedLens::from_calibration(sample.clone());
    let p = RgbOpticsPlan::new(&s, frame, Some(&resolved))
        .unwrap()
        .unwrap();
    let n = frame.area() as usize;
    let samples: Vec<f32> = (0..3 * n)
        .map(|i| 0.1 + ((i * 13 % 97) as f32) / 120.)
        .collect();
    let gpu = GpuStageOp::new(Arc::new(GpuContext::new().unwrap()));
    let mut b = gpu.begin_resident().unwrap();
    let t = b
        .upload(
            &Tile::from_samples(
                TileCoord::new(0, 0, 0),
                TileLayout {
                    extent: frame,
                    halo: 0,
                    channels: 3,
                },
                samples.clone(),
            )
            .unwrap(),
        )
        .unwrap();
    let t = p.before_white_balance(b.as_mut(), &t).unwrap();
    let t = p.after_white_balance(b.as_mut(), &t).unwrap();
    let out = b
        .finish(vec![t], false, None, &CancellationToken::new())
        .unwrap();
    let got = out.tiles[0].samples::<f32>().unwrap();
    for c in 0..3 {
        for y in 0..frame.height {
            for x in 0..frame.width {
                let px = 2. * (x as f64 + 0.5) / frame.width as f64 - 1.;
                let py = 2. * (y as f64 + 0.5) / frame.height as f64 - 1.;
                let r = px * px + py * py;
                let coeff = if c == 0 {
                    sample.ca_red
                } else if c == 2 {
                    sample.ca_blue
                } else {
                    [1., 0., 0.]
                };
                let scale = coeff[0] + r * (coeff[1] + r * coeff[2]);
                let u = ((px * scale + 1.) * frame.width as f64 / 2. - 0.5)
                    .clamp(0., (frame.width - 1) as f64);
                let v = ((py * scale + 1.) * frame.height as f64 / 2. - 0.5)
                    .clamp(0., (frame.height - 1) as f64);
                let at = |xx: u32, yy: u32| {
                    samples[c * n
                        + (yy.min(frame.height - 1) * frame.width + xx.min(frame.width - 1))
                            as usize] as f64
                };
                let a = u.floor() as u32;
                let z = v.floor() as u32;
                let value = (at(a, z) * (1. - u.fract()) + at(a + 1, z) * u.fract())
                    * (1. - v.fract())
                    + (at(a, z + 1) * (1. - u.fract()) + at(a + 1, z + 1) * u.fract()) * v.fract();
                let expected = value / (1. + r * (-0.12 + r * 0.02));
                assert!(
                    (got[c * n + (y * frame.width + x) as usize] as f64 - expected).abs() < 3e-5
                );
            }
        }
    }
}

#[test]
fn unsupported_controls_and_invalid_frames_are_rejected() {
    let frame = Extent::new(19, 13);
    let mut s = DevelopSettings::default();
    s.lens.profile = LensProfileSource::None;
    s.lens.remove_chromatic_aberration = false;
    s.lens.defringe_purple.amount = 5.;
    assert!(RgbOpticsPlan::new(&s, frame, None).unwrap().is_none());
    s.lens.defringe_purple.amount = 0.;
    s.geometry.orientation = 6;
    assert!(RgbOpticsPlan::new(&s, frame, None).unwrap().is_none());
    assert!(RgbOpticsPlan::new(&s, Extent::new(0, 13), None).is_err());
}

#[test]
fn unresolved_auto_and_ca_are_not_silently_identity() {
    let mut s = DevelopSettings::default();
    let frame = Extent::new(31, 23);
    assert!(RgbOpticsPlan::new(&s, frame, None).unwrap().is_none());
    s.lens.profile = LensProfileSource::None;
    assert!(RgbOpticsPlan::new(&s, frame, None).unwrap().is_none());
    s.lens.remove_chromatic_aberration = false;
    assert!(RgbOpticsPlan::new(&s, frame, None).unwrap().is_some());
}
