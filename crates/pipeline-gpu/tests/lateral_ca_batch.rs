#[path = "../../image-core/tests/common/mod.rs"]
mod common;

use engine_api::{
    jobs::CancellationToken,
    recipe::DevelopSettings,
    tile::{Extent, Tile, TileCoord},
};
use image_core::{Op, StageOp};
use pipeline_cpu::{CaPlan, DemosaicAlgorithm, Image, LensContext};
use pipeline_gpu::{GpuContext, GpuStageOp};
use std::sync::Arc;

fn plan(metadata: &raw_decode::RawMetadata) -> CaPlan {
    let profile = lens::Profile {
        camera: None,
        maker: "test".into(),
        model: "test".into(),
        samples: vec![lens::CalibrationSample {
            ca_red: [1.003, 0.002, -0.0001],
            ca_blue: [0.997, -0.001, 0.0002],
            coordinate_scale: [0.8, 1.1],
            ..Default::default()
        }],
    };
    let settings = DevelopSettings::default();
    let image = Image::new(1, 1, vec![vec![0.2]; 3]).unwrap();
    let resolved = pipeline_cpu::resolve_lens(
        &image,
        &settings.lens,
        Some(metadata),
        &LensContext {
            profile: Some(&profile),
            ..Default::default()
        },
    )
    .unwrap();
    resolved
        .plan(&settings, metadata)
        .unwrap()
        .unwrap()
        .ca
        .unwrap()
}

fn reference(
    raw: &Image,
    cfa: raw_decode::CfaLayout,
    algorithm: DemosaicAlgorithm,
    plan: &CaPlan,
) -> Image {
    let mut rgb = Image::new(
        raw.width(),
        raw.height(),
        vec![vec![0.; (raw.width() * raw.height()) as usize]; 3],
    )
    .unwrap();
    let period = if matches!(cfa, raw_decode::CfaLayout::XTrans(_)) {
        6
    } else {
        2
    };
    for coord in raw.coords() {
        rgb.put(
            &pipeline_cpu::demosaic(&raw.tile(coord, 3, period).unwrap(), cfa, algorithm).unwrap(),
        )
        .unwrap();
    }
    let mut planes = rgb.planes().to_vec();
    for c in [0, 2] {
        for y in 0..raw.height() {
            for x in 0..raw.width() {
                let [u, v] = plan.source(x, y, c).unwrap();
                let u = u.clamp(0., (raw.width() - 1) as f64);
                let v = v.clamp(0., (raw.height() - 1) as f64);
                let (a, b) = (u.floor() as u32, v.floor() as u32);
                let at = |x: u32, y: u32| {
                    rgb.planes()[c]
                        [(y.min(raw.height() - 1) * raw.width() + x.min(raw.width() - 1)) as usize]
                        as f64
                };
                planes[c][(y * raw.width() + x) as usize] =
                    ((at(a, b) * (1. - u.fract()) + at(a + 1, b) * u.fract()) * (1. - v.fract())
                        + (at(a, b + 1) * (1. - u.fract()) + at(a + 1, b + 1) * u.fract())
                            * v.fract()) as f32;
            }
        }
    }
    Image::new(raw.width(), raw.height(), planes).unwrap()
}

fn assert_tiles(actual: &[Tile], reference: &Image) {
    for tile in actual {
        let expected = reference.tile(tile.coord(), 0, 1).unwrap();
        let error = tile
            .samples::<f32>()
            .unwrap()
            .iter()
            .zip(expected.samples::<f32>().unwrap())
            .map(|(a, b)| {
                assert!(a.is_finite());
                (a - b).abs()
            })
            .fold(0f32, f32::max);
        assert!(error < 1e-4, "CA CPU gate at {:?}: {error}", tile.coord());
    }
}

#[test]
fn host_demosaic_ca_batches_sensor_neighbours_with_one_readback() {
    let gpu = GpuStageOp::new(Arc::new(GpuContext::new().unwrap()));
    let cfa = common::xtrans();
    let metadata = common::metadata(515, 259, cfa, [7, 5, 501, 249]);
    let ca = plan(&metadata);
    let raw = Image::new(515, 259, vec![common::samples(515, 259, cfa)]).unwrap();
    let coords: Vec<_> = raw.coords().collect();
    let inputs = coords.iter().map(|&c| raw.tile(c, 3, 6).unwrap()).collect();
    let algorithm = DemosaicAlgorithm::MalvarHeCutler;
    let actual = gpu
        .demosaic_ca_batch(
            &metadata,
            algorithm,
            &ca,
            inputs,
            &coords,
            &CancellationToken::new(),
        )
        .unwrap()
        .unwrap();
    assert_tiles(&actual, &reference(&raw, cfa, algorithm, &ca));
    assert_eq!(actual.len(), coords.len());
    assert_eq!(gpu.stats().submissions, 1);
    assert_eq!(gpu.stats().readbacks, 1);
}

#[test]
fn host_ca_declines_all_staged_opcodes_before_gpu_work() {
    let gpu = GpuStageOp::new(Arc::new(GpuContext::new().unwrap()));
    let cfa = common::xtrans();
    let base = common::metadata(17, 13, cfa, [0, 0, 17, 13]);
    let ca = plan(&base);
    let raw = Image::new(17, 13, vec![common::samples(17, 13, cfa)]).unwrap();
    for stage in 0..4 {
        let mut metadata = base.clone();
        if stage == 3 {
            metadata.has_opcode_list = true;
        } else {
            metadata.opcode_lists[stage] = Some(vec![0, 0, 0, 0]);
        }
        let output = gpu
            .demosaic_ca_batch(
                &metadata,
                DemosaicAlgorithm::MalvarHeCutler,
                &ca,
                vec![raw.tile(TileCoord::new(0, 0, 0), 3, 6).unwrap()],
                &[TileCoord::new(0, 0, 0)],
                &CancellationToken::new(),
            )
            .unwrap();
        assert!(output.is_none(), "stage {stage} must use CPU");
    }
    assert_eq!(gpu.stats().uploads, 0);
    assert_eq!(gpu.stats().submissions, 0);
}

#[test]
fn managed_exports_decline_staged_opcodes_even_with_a_supplied_plan() {
    use color_mgmt::{Builtin, Registry, TransformOptions};
    use pipeline_cpu::{OutputContext, OutputTarget};
    use pipeline_gpu::{GpuManagedOutput, ManagedRenderer};
    let mut settings = DevelopSettings::default();
    settings.lens.profile = engine_api::recipe::settings::LensProfileSource::None;
    settings.lens.remove_chromatic_aberration = false;
    let mut registry = Registry::new();
    let target = registry.builtin(Builtin::Srgb).unwrap();
    let output = Arc::new(
        GpuManagedOutput::new(
            Arc::new(GpuContext::new().unwrap()),
            &settings,
            &mut OutputContext {
                registry: &mut registry,
                target: OutputTarget::Export(&target),
                proof: None,
                options: TransformOptions::default(),
            },
        )
        .unwrap(),
    );
    let renderer = ManagedRenderer::new_export(output, Default::default());
    let lens = pipeline_cpu::LensPlan {
        ca: None,
        vignette: None,
        map: None,
    };
    for stage in 0..4 {
        let mut metadata = common::metadata(17, 13, common::xtrans(), [0, 0, 17, 13]);
        if stage == 3 {
            metadata.has_opcode_list = true;
        } else {
            metadata.opcode_lists[stage] = Some(vec![0, 0, 0, 0]);
        }
        let image = image_core::RawImage::new(
            engine_api::id::ImageId(999),
            Arc::new(
                raw_decode::CfaImage::from_linear(
                    17,
                    13,
                    common::samples(17, 13, common::xtrans()),
                )
                .unwrap(),
            ),
            Arc::new(metadata),
        )
        .unwrap();
        let rect = image_core::PixelRect::full(image.active_extent());
        let cancel = CancellationToken::new();
        assert!(
            renderer
                .render_export(&image, &settings, 0, rect, &cancel)
                .unwrap()
                .is_none()
        );
        assert!(
            renderer
                .render_export_lens(&image, &settings, 0, rect, &lens, &cancel)
                .unwrap()
                .is_none()
        );
        let mut dst = vec![-1.; 17 * 13 * 3];
        assert!(
            !renderer
                .render_export_rows(&image, &settings, 0, 0..13, Some(&lens), &mut dst, &cancel)
                .unwrap()
        );
        assert!(dst.iter().all(|&v| v == -1.));
    }
    assert_eq!(renderer.stats().uploads, 0);
    assert_eq!(renderer.stats().submissions, 0);
}

#[test]
fn resident_bands_keep_ca_in_sensor_frame_before_crop_and_readback() {
    for cfa in [
        common::RGGB,
        raw_decode::CfaLayout::Bayer([[1, 0], [2, 3]]),
        raw_decode::CfaLayout::Bayer([[3, 2], [0, 1]]),
        raw_decode::CfaLayout::Bayer([[2, 3], [1, 0]]),
        common::xtrans(),
    ] {
        for algorithm in [
            DemosaicAlgorithm::Bilinear,
            DemosaicAlgorithm::MalvarHeCutler,
        ] {
            let gpu = GpuStageOp::new(Arc::new(GpuContext::new().unwrap()));
            let frame = Extent::new(273, 269);
            // A supplied camera-RGB CA plan is independent of the demosaicer.
            // Database Bayer plans remain CPU-only at resolution time.
            let metadata = common::metadata(
                frame.width,
                frame.height,
                common::xtrans(),
                [7, 9, 259, 251],
            );
            let ca = plan(&metadata);
            let raw = Image::new(
                frame.width,
                frame.height,
                vec![common::samples(frame.width, frame.height, cfa)],
            )
            .unwrap();
            let expected = reference(&raw, cfa, algorithm, &ca);
            let period = if matches!(cfa, raw_decode::CfaLayout::XTrans(_)) {
                6
            } else {
                2
            };
            let mut batch = gpu.begin_resident().unwrap();
            let uploaded = batch
                .upload_rows(&raw.planes()[0], frame.width, 0..frame.height)
                .unwrap();
            let gathered = batch
                .gather_rows(frame, &uploaded, 0, 0..frame.height, 3, period)
                .unwrap();
            let dem = batch
                .run_at(&Op::Demosaic { cfa, algorithm }, &gathered, (0, 0))
                .unwrap();
            let halo = ca
                .max_displacement(frame.width, frame.height)
                .unwrap()
                .ceil() as u16
                + 2;
            let rows = 247..269;
            let gathered = batch
                .gather_rows(frame, &dem, 0, rows.clone(), halo, 1)
                .unwrap();
            let corrected = batch
                .lateral_ca_at(&gathered, (0, rows.start), frame, &ca)
                .unwrap();
            assert_eq!(gpu.stats().submissions, 0);
            assert_eq!(gpu.stats().readbacks, 0);
            let mut dst = vec![0.; frame.width as usize * rows.len() * 3];
            batch
                .finish_rows(corrected, &mut dst, &CancellationToken::new())
                .unwrap();
            for (i, &v) in dst.iter().enumerate() {
                let c = i % 3;
                let pixel = i / 3 + rows.start as usize * frame.width as usize;
                assert!(
                    (v - expected.planes()[c][pixel]).abs() < 1e-4,
                    "{cfa:?}/{algorithm:?}: {i}"
                );
            }
            assert_eq!(gpu.stats().submissions, 1);
            assert_eq!(gpu.stats().readbacks, 1);
        }
    }
}

#[test]
fn host_batch_rejects_missing_dependencies_and_cancelled_work() {
    let gpu = GpuStageOp::new(Arc::new(GpuContext::new().unwrap()));
    let metadata = common::metadata(515, 259, common::xtrans(), [7, 5, 501, 249]);
    let ca = plan(&metadata);
    let raw = Image::new(515, 259, vec![common::samples(515, 259, common::xtrans())]).unwrap();
    let coord = TileCoord::new(0, 0, 0);
    assert!(
        gpu.demosaic_ca_batch(
            &metadata,
            DemosaicAlgorithm::MalvarHeCutler,
            &ca,
            vec![raw.tile(coord, 3, 6).unwrap()],
            &[coord],
            &CancellationToken::new()
        )
        .is_err()
    );
    let cancel = CancellationToken::new();
    cancel.cancel();
    assert!(
        gpu.demosaic_ca_batch(
            &metadata,
            DemosaicAlgorithm::MalvarHeCutler,
            &ca,
            Vec::new(),
            &[],
            &cancel
        )
        .is_err()
    );
    assert_eq!(gpu.stats().submissions, 0);
    assert_eq!(gpu.stats().readbacks, 0);
}
