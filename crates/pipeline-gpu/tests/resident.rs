#[path = "../../image-core/tests/common/mod.rs"]
mod common;
use engine_api::{recipe::DevelopSettings, stage::StageId};
use image_core::{PixelRect, Renderer, RendererConfig, TileCache};
use pipeline_gpu::{GpuContext, GpuStageOp};
use std::sync::Arc;

#[test]
fn fused_tone_display_matches_separate_dispatches() {
    use engine_api::{
        jobs::CancellationToken,
        recipe::settings::{GamutMapping, ToneSettings},
        tile::{Extent, Tile, TileCoord, TileLayout},
    };
    use image_core::{Op, StageOp};
    let gpu = GpuStageOp::new(Arc::new(GpuContext::new().unwrap()));
    let input = Tile::from_samples(
        TileCoord::new(2, 1, 3),
        TileLayout {
            extent: Extent::new(17, 19),
            halo: 0,
            channels: 3,
        },
        (0..17 * 19 * 3)
            .map(|i| (i as f32 / 193.0).sin() * 1.2)
            .collect(),
    )
    .unwrap();
    let tone = ToneSettings {
        exposure: 0.7,
        contrast: 15.0,
        highlights: -30.0,
        shadows: 25.0,
        ..Default::default()
    };
    for gamut in [GamutMapping::Clip, GamutMapping::default()] {
        let chain = [
            Op::Tone(&tone),
            Op::Display {
                gamut,
                headroom: None,
            },
        ];
        let mut batch = gpu.begin_resident().unwrap();
        let raw = batch.upload(&input).unwrap();
        let toned = batch.run(&chain[0], &raw).unwrap();
        let separate = batch.run(&chain[1], &toned).unwrap();
        let fused = batch.run_chain(&chain, &raw).unwrap();
        let result = batch
            .finish(vec![separate, fused], true, None, &CancellationToken::new())
            .unwrap();
        assert!(
            result.tiles[0]
                .samples::<u8>()
                .unwrap()
                .iter()
                .zip(result.tiles[1].samples::<u8>().unwrap())
                .all(|(a, b)| a.abs_diff(*b) <= 1)
        );
    }
}

#[test]
fn raw_source_cache_preserves_sensor_precision_and_accounts_f32_bytes() {
    use engine_api::{
        id::ImageId,
        jobs::CancellationToken,
        stage::{MemoKey, ParamHash},
        tile::{Extent, Tile, TileCoord, TileLayout},
    };
    use image_core::StageOp;
    let gpu = GpuStageOp::with_cache_budget(Arc::new(GpuContext::new().unwrap()), 64);
    let input = Tile::from_samples(
        TileCoord::new(0, 0, 0),
        TileLayout {
            extent: Extent::new(3, 1),
            halo: 0,
            channels: 1,
        },
        vec![0.10001_f32, 0.99991, 1.0001],
    )
    .unwrap();
    let key = MemoKey {
        image_id: ImageId(909),
        stage: StageId::Decode,
        params_hash: ParamHash::default(),
        tile: input.coord(),
    };
    let cancel = CancellationToken::new();
    let mut batch = gpu.begin_resident().unwrap();
    let raw = batch.upload_cached(key, &input).unwrap();
    let first = batch.finish(vec![raw], false, None, &cancel).unwrap();
    assert_eq!(
        first.tiles[0].samples::<f32>().unwrap(),
        input.samples::<f32>().unwrap()
    );
    assert_eq!(gpu.cache_bytes(), 12);
    let uploads = gpu.stats().uploads;
    let mut batch = gpu.begin_resident().unwrap();
    let raw = batch.cached(&key).unwrap().unwrap();
    let second = batch.finish(vec![raw], false, None, &cancel).unwrap();
    assert_eq!(
        second.tiles[0].samples::<f32>().unwrap(),
        input.samples::<f32>().unwrap()
    );
    assert_eq!(gpu.stats().uploads, uploads);
}

#[test]
fn resident_graph_one_submission_and_warm_edits_without_uploads() {
    let gpu = Arc::new(GpuStageOp::new(Arc::new(GpuContext::new().unwrap())));
    let cfg = RendererConfig::default();
    let r = Renderer::with_ops(
        gpu.clone(),
        Arc::new(TileCache::new(cfg.cache_budget_bytes)),
        cfg,
    );
    let cpu = Renderer::new(RendererConfig::default());
    let image = common::synthetic(906, 700, 533, common::RGGB, [3, 5, 690, 521]);
    for level in [0, 1, 2, 12] {
        let mut s = DevelopSettings::default();
        let rect = PixelRect::full(image.level_extent(level));
        for change in 0..5 {
            if change == 1 {
                s.tone.exposure += 0.3;
            }
            if change == 2 {
                s.white_balance.mode = engine_api::recipe::settings::WhiteBalanceMode::Daylight;
            }
            if change == 3 {
                s.detail.sharpening.amount = 80.0;
            }
            if change == 4 {
                s.detail.sharpening.amount = 0.0;
                s.detail.noise_reduction.color = 0.0;
            }
            let before = gpu.stats();
            let a = r.render_region(&image, &s, level, rect).unwrap();
            let after = gpu.stats();
            assert_eq!(after.submissions - before.submissions, 1);
            assert_eq!(after.readbacks - before.readbacks, 1);
            if change > 0 {
                assert_eq!(after.uploads - before.uploads, 0);
            }
            let b = cpu.render_region(&image, &s, level, rect).unwrap();
            assert_eq!(a.len(), b.len());
            for (a, b) in a.iter().zip(&b) {
                let max_difference = a
                    .samples::<u8>()
                    .unwrap()
                    .iter()
                    .zip(b.samples::<u8>().unwrap())
                    .map(|(a, b)| a.abs_diff(*b))
                    .max()
                    .unwrap();
                assert!(
                    a.samples::<u8>()
                        .unwrap()
                        .iter()
                        .zip(b.samples::<u8>().unwrap())
                        .all(|(a, b)| a.abs_diff(*b) <= 2),
                    "level={level} change={change} tile={:?} max_difference={max_difference}",
                    a.coord()
                );
            }
            let again = r.render_region(&image, &s, level, rect).unwrap();
            for (a, b) in a.iter().zip(&again) {
                assert_eq!(a.samples::<u8>().unwrap(), b.samples::<u8>().unwrap());
            }
        }
    }
}

#[test]
fn resident_cache_budget_is_bounded() {
    let gpu = Arc::new(GpuStageOp::with_cache_budget(
        Arc::new(GpuContext::new().unwrap()),
        4096,
    ));
    let cfg = RendererConfig::default();
    let r = Renderer::with_ops(gpu.clone(), Arc::new(TileCache::new(4096)), cfg);
    let image = common::synthetic(907, 41, 39, common::RGGB, [1, 1, 39, 37]);
    let rect = PixelRect::full(image.level_extent(2));
    let a = r
        .render_region(&image, &DevelopSettings::default(), 2, rect)
        .unwrap();
    let b = r
        .render_region(&image, &DevelopSettings::default(), 2, rect)
        .unwrap();
    assert!(gpu.cache_bytes() <= 4096);
    assert_eq!(a[0].samples::<u8>().unwrap(), b[0].samples::<u8>().unwrap());
}

#[test]
fn partial_preview_gathers_unrequested_detail_neighbours_without_cache() {
    use engine_api::{jobs::CancellationToken, tile::TileCoord};
    use image_core::RenderOutput;
    let gpu = Arc::new(GpuStageOp::with_cache_budget(
        Arc::new(GpuContext::new().unwrap()),
        0,
    ));
    let r = Renderer::with_ops(
        gpu.clone(),
        Arc::new(TileCache::new(0)),
        RendererConfig::default(),
    );
    let cpu = Renderer::new(RendererConfig::default());
    let image = common::synthetic(908, 700, 533, common::RGGB, [3, 5, 690, 521]);
    let settings = DevelopSettings::default();
    let coords = [TileCoord::new(1, 1, 1)];
    let render = |renderer: &Renderer| {
        let mut tiles = Vec::new();
        renderer
            .render_tiles(
                &image,
                &settings,
                &coords,
                RenderOutput::Display,
                &CancellationToken::new(),
                &mut |t| tiles.push(t),
            )
            .unwrap();
        tiles
    };
    let a = render(&r);
    let b = render(&cpu);
    assert_eq!(a.len(), 1);
    assert_eq!(b.len(), 1);
    assert_eq!(a[0].coord(), coords[0]);
    assert_eq!(a[0].layout(), b[0].layout());
    assert!(
        a[0].samples::<u8>()
            .unwrap()
            .iter()
            .zip(b[0].samples::<u8>().unwrap())
            .all(|(a, b)| a.abs_diff(*b) <= 2)
    );
    assert_eq!(gpu.stats().submissions, 1);
    assert_eq!(gpu.stats().readbacks, 1);
    assert_eq!(gpu.cache_bytes(), 0);
}

#[test]
fn cancelled_batch_publishes_nothing_and_does_not_submit_compute() {
    use engine_api::{
        id::ImageId,
        jobs::CancellationToken,
        stage::{MemoKey, ParamHash},
        tile::{Extent, Tile, TileCoord, TileLayout},
    };
    use image_core::StageOp;
    let gpu = GpuStageOp::new(Arc::new(GpuContext::new().unwrap()));
    let t = Tile::from_samples(
        TileCoord::new(0, 0, 0),
        TileLayout {
            extent: Extent::new(3, 1),
            halo: 0,
            channels: 3,
        },
        vec![0.2_f32; 9],
    )
    .unwrap();
    let k = MemoKey {
        image_id: ImageId(99),
        stage: StageId::WhiteBalance,
        params_hash: ParamHash::default(),
        tile: t.coord(),
    };
    let mut b = gpu.begin_resident().unwrap();
    let t = b.upload(&t).unwrap();
    let t = b.cache(k, &t).unwrap();
    let cancel = CancellationToken::new();
    cancel.cancel();
    assert!(b.finish(vec![t], false, None, &cancel).is_err());
    assert_eq!(gpu.stats().submissions, 0);
    assert_eq!(gpu.cache_bytes(), 0);
}

#[test]
#[ignore = "requires Metal and the Nikon NEF fixture; prints measurements, not a target guarantee"]
fn bench_nef_level2_resident() {
    use engine_api::id::ImageId;
    use std::time::Instant;
    let path = std::env::var_os("PIPELINE_NEF_FIXTURE")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| {
            std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("../../fixtures/raw/nikon-nef.NEF")
        });
    let image = image_core::RawImage::open(ImageId(600), &path).unwrap();
    let gpu = Arc::new(GpuStageOp::new(Arc::new(GpuContext::new().unwrap())));
    eprintln!(
        "adapter: {:?}; source: {}; extent: {:?}",
        gpu.context().adapter_info,
        path.display(),
        image.level_extent(2)
    );
    let cfg = RendererConfig::default();
    let r = Renderer::with_ops(
        gpu.clone(),
        Arc::new(TileCache::new(cfg.cache_budget_bytes)),
        cfg.clone(),
    );
    let cpu = Renderer::new(cfg);
    let rect = PixelRect::full(image.level_extent(2));
    let mut times = [
        [Vec::new(), Vec::new(), Vec::new()],
        [Vec::new(), Vec::new(), Vec::new()],
    ];
    for iteration in 0..5 {
        for (j, renderer) in [&r, &cpu].into_iter().enumerate() {
            renderer.cache().clear();
            if j == 0 {
                gpu.clear_cache();
            }
            let mut settings = DevelopSettings::default();
            for (k, label) in ["first", "tone", "WB"].into_iter().enumerate() {
                if k == 1 {
                    settings.tone.exposure = 0.25;
                }
                if k == 2 {
                    settings.white_balance.mode =
                        engine_api::recipe::settings::WhiteBalanceMode::Daylight;
                }
                let before = gpu.stats();
                let start = Instant::now();
                let tiles = renderer.render_region(&image, &settings, 2, rect).unwrap();
                let ms = start.elapsed().as_secs_f64() * 1000.;
                assert!(!tiles.is_empty());
                times[j][k].push(ms);
                if j == 0 {
                    let after = gpu.stats();
                    assert_eq!(after.submissions - before.submissions, 1);
                    assert!(
                        after.last_resident_allocated_bytes < 768 * 1024 * 1024,
                        "cold render retained too much GPU payload: {:?}",
                        after
                    );
                    eprintln!("resident allocation diagnostics: {:?}", after);
                    eprintln!(
                        "sample {iteration} {label}: {ms:.3} ms; uploads {}, readbacks {}, submissions {}; cache {} bytes",
                        after.uploads - before.uploads,
                        after.readbacks - before.readbacks,
                        after.submissions - before.submissions,
                        gpu.cache_bytes()
                    );
                }
            }
        }
    }
    for (k, label) in ["first", "tone (target <=5 ms)", "WB (target <=40 ms)"]
        .into_iter()
        .enumerate()
    {
        for row in &mut times {
            row[k].sort_by(f64::total_cmp);
        }
        eprintln!(
            "NEF L2 {label}: GPU {:.3} ms, CPU {:.3} ms; median of 5, wall time including final readback",
            times[0][k][2], times[1][k][2]
        );
    }
}
