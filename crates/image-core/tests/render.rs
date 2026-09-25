mod common;
#[path = "common/preview.rs"]
mod preview;

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, mpsc};
use std::time::{Duration, Instant};

use common::*;
use engine_api::EngineError;
use engine_api::jobs::{CancellationToken, JobStatus, Priority, Scheduler};
use engine_api::recipe::settings::{
    DemosaicMethod, GamutMapping, HighlightReconstruction, WhiteBalanceMode,
};
use engine_api::recipe::{DevelopSettings, ProcessVersion};
use engine_api::stage::StageId;
use engine_api::tile::{TileCoord, TileFormat};
use image_core::{
    CountingStageOp, CpuStageOp, PipelineGraph, PixelRect, ProgressiveRenderJob, RawImage,
    RenderOutput, Renderer, RendererConfig, TileCache, Viewport,
};
use pipeline_cpu::{RenderSource, render_linear_scaled, render_scaled};

fn renderer(threads: usize) -> Renderer {
    Renderer::new(RendererConfig {
        threads,
        ..RendererConfig::default()
    })
}

fn counting(threads: usize, budget: usize) -> (Renderer, Arc<CountingStageOp<CpuStageOp>>) {
    let ops = Arc::new(CountingStageOp::new(CpuStageOp));
    let r = Renderer::with_ops(
        ops.clone(),
        Arc::new(TileCache::new(budget)),
        RendererConfig {
            threads,
            ..RendererConfig::default()
        },
    );
    (r, ops)
}

fn full(image: &RawImage, level: u8) -> PixelRect {
    PixelRect::full(image.level_extent(level))
}

fn reference_u8(image: &RawImage, s: &DevelopSettings, level: u8) -> Vec<u8> {
    let source = RenderSource::Cfa {
        image: image.cfa(),
        metadata: image.metadata(),
    };
    if level == 0 {
        render_scaled(s, &source, 1).unwrap().into_raw()
    } else {
        preview::display(&preview::linear(&source, s, 1 << level), s)
    }
}

fn reference_linear(image: &RawImage, s: &DevelopSettings, level: u8) -> Vec<Vec<f32>> {
    let source = RenderSource::Cfa {
        image: image.cfa(),
        metadata: image.metadata(),
    };
    if level == 0 {
        render_linear_scaled(s, &source, 1)
            .unwrap()
            .planes()
            .to_vec()
    } else {
        preview::linear(&source, s, 1 << level).planes().to_vec()
    }
}

fn bayer_image() -> RawImage {
    // Unaligned crop, partial edge tiles and partial edge blocks at every level.
    synthetic(1, 700, 533, RGGB, [3, 5, 690, 521])
}

#[test]
fn cold_render_matches_reference_at_every_level() {
    let image = bayer_image();
    let s = DevelopSettings::default();
    for threads in [1, 4] {
        for level in 0..=3 {
            let r = renderer(threads);
            let e = image.level_extent(level);
            let tiles = r
                .render_region(&image, &s, level, full(&image, level))
                .unwrap();
            assert!(tiles.iter().all(|t| t.format() == TileFormat::U8));
            let got = assemble_u8(e, &tiles);
            let reference = reference_u8(&image, &s, level);
            // The scene is not degenerate: many distinct codes, some clipped.
            let distinct: std::collections::BTreeSet<u8> = reference.iter().copied().collect();
            assert!(distinct.len() > 100, "{}", distinct.len());
            assert_eq!(max_u8_diff(&got, &reference), 0, "L{level}");

            let r = renderer(threads);
            let tiles = r
                .render_region_as(
                    &image,
                    &s,
                    level,
                    full(&image, level),
                    RenderOutput::SceneLinear,
                )
                .unwrap();
            let diff = max_f32_diff(
                &assemble_f32(e, &tiles),
                &reference_linear(&image, &s, level),
            );
            assert!(diff <= 1e-5, "L{level}: {diff}");
        }
    }
}

#[test]
fn level_zero_matches_reference_for_non_default_settings() {
    let image = bayer_image();
    let mut s = DevelopSettings::default();
    s.tone.exposure = 1.3;
    s.tone.contrast = 25.0;
    s.tone.shadows = 40.0;
    s.tone.highlights = -60.0;
    s.white_balance.mode = WhiteBalanceMode::Daylight;
    s.output.gamut_mapping = GamutMapping::Clip;
    for (method, highlights) in [
        (DemosaicMethod::Bilinear, HighlightReconstruction::Clip),
        (
            DemosaicMethod::Auto,
            HighlightReconstruction::ReconstructColor,
        ),
    ] {
        s.demosaic.method = method;
        s.linearize.highlight_reconstruction = highlights;
        let tiles = renderer(3)
            .render_region(&image, &s, 0, full(&image, 0))
            .unwrap();
        let got = assemble_u8(image.level_extent(0), &tiles);
        assert_eq!(max_u8_diff(&got, &reference_u8(&image, &s, 0)), 0);
    }
}

#[test]
fn xtrans_matches_reference() {
    // 515 wide: the last tile column is 3 px, narrower than the 6 px period.
    let image = synthetic(2, 515, 300, xtrans(), [1, 0, 514, 299]);
    let s = DevelopSettings::default();
    for level in [0, 1, 3] {
        let tiles = renderer(2)
            .render_region(&image, &s, level, full(&image, level))
            .unwrap();
        let got = assemble_u8(image.level_extent(level), &tiles);
        assert_eq!(
            max_u8_diff(&got, &reference_u8(&image, &s, level)),
            0,
            "L{level}"
        );
    }
}

#[test]
fn sub_region_equals_the_same_tiles_of_a_full_render() {
    let image = bayer_image();
    let s = DevelopSettings::default();
    let all = renderer(2)
        .render_region(&image, &s, 0, full(&image, 0))
        .unwrap();
    let part = renderer(2)
        .render_region(&image, &s, 0, PixelRect::new(300, 260, 10, 10))
        .unwrap();
    assert_eq!(part.len(), 1);
    let same = all.iter().find(|t| t.coord() == part[0].coord()).unwrap();
    assert_eq!(
        same.samples::<u8>().unwrap(),
        part[0].samples::<u8>().unwrap()
    );
    assert!(
        renderer(1)
            .render_region(&image, &s, 0, PixelRect::new(5000, 0, 10, 10))
            .unwrap()
            .is_empty()
    );
}

#[test]
fn tone_change_reuses_white_balance_and_reruns_uncached_detail() {
    let image = bayer_image();
    let (r, ops) = counting(4, 256 << 20);
    let mut s = DevelopSettings::default();
    let level = 1;
    let tiles = r
        .render_region(&image, &s, level, full(&image, level))
        .unwrap();
    let n = tiles.len() as u64;
    let cold = ops.counts();
    for stage in [
        StageId::Linearize,
        StageId::Demosaic,
        StageId::CameraProfile,
        StageId::WhiteBalance,
    ] {
        assert!(cold[stage.index()] > 0, "{stage} ran on the cold render");
    }
    // Each sensor tile is linearized and demosaiced exactly once per request.
    assert_eq!(cold[StageId::Linearize.index()], 3 * 3);
    assert_eq!(cold[StageId::Demosaic.index()], 3 * 3);
    assert_eq!(cold[StageId::Tone.index()], 2 * n + 1);
    assert_eq!(cold[StageId::Output.index()], n);

    let before = s.clone();
    s.tone.exposure = 0.7;
    s.tone.contrast = -30.0;
    assert_eq!(
        PipelineGraph::earliest_dirty_stage(&before, &s),
        Some(StageId::Tone)
    );
    ops.reset();
    let warm = r
        .render_region(&image, &s, level, full(&image, level))
        .unwrap();
    let counts = ops.counts();
    for stage in StageId::ALL {
        let expected = match stage {
            StageId::Tone => 2 * n + 1, // Base pass, basic tone and whole-image ToneExtra.
            StageId::Detail | StageId::Color | StageId::Effects | StageId::Output => n,
            StageId::Geometry => 1,
            _ => 0,
        };
        assert_eq!(counts[stage.index()], expected, "{stage}");
    }
    // Served from the f16 cache: within f16 rounding of a cold render.
    let cold_tiles = renderer(4)
        .render_region(&image, &s, level, full(&image, level))
        .unwrap();
    let e = image.level_extent(level);
    assert!(max_u8_diff(&assemble_u8(e, &warm), &assemble_u8(e, &cold_tiles)) <= 1);

    // White balance: demosaic comes from cache, only the matrices re-run.
    s.white_balance.mode = WhiteBalanceMode::Tungsten;
    ops.reset();
    r.render_region(&image, &s, level, full(&image, level))
        .unwrap();
    assert_eq!(ops.count(StageId::Linearize), 0);
    assert_eq!(ops.count(StageId::Demosaic), 0);
    assert_eq!(ops.count(StageId::CameraProfile), 9);
    assert_eq!(ops.count(StageId::WhiteBalance), 9);

    // A different level reuses level-0 demosaic tiles.
    ops.reset();
    r.render_region(&image, &s, 2, full(&image, 2)).unwrap();
    assert_eq!(ops.count(StageId::Demosaic), 0);
    assert_eq!(ops.count(StageId::Tone), 3); // Base tone, basic tone, ToneExtra.
}

#[test]
fn cache_budget_is_respected_and_does_not_change_cold_output() {
    let image = bayer_image();
    let s = DevelopSettings::default();
    let budget = 1 << 20; // ~2.7 f16 demosaic tiles
    let (small, _) = counting(3, budget);
    let a = small.render_region(&image, &s, 0, full(&image, 0)).unwrap();
    let stats = small.cache().stats();
    assert!(small.cache().bytes() <= budget);
    assert!(stats.peak_bytes <= budget, "{stats:?}");
    assert!(stats.evictions > 0, "{stats:?}");
    let b = renderer(3)
        .render_region(&image, &s, 0, full(&image, 0))
        .unwrap();
    let e = image.level_extent(0);
    assert_eq!(assemble_u8(e, &a), assemble_u8(e, &b));

    // Memoized outputs are stored as F16Planar under the chained memo key.
    let big = renderer(1);
    big.render_region(&image, &s, 3, full(&image, 3)).unwrap();
    let chain = s.stage_chain(ProcessVersion::NATIVE_CURRENT.chain_seed());
    for (stage, coord) in [
        (StageId::Demosaic, TileCoord::new(0, 1, 1)),
        (StageId::WhiteBalance, TileCoord::new(3, 0, 0)),
    ] {
        let key = PipelineGraph::memo_key(image.id(), &chain, stage, coord);
        let t = big.cache().get(&key).expect("memoized");
        assert_eq!(t.format(), TileFormat::F16Planar);
    }
    assert!(big.cache().bytes() <= big.cache().budget());

    // Zero budget: nothing is kept, rendering still works.
    let (none, ops) = counting(2, 0);
    none.render_region(&image, &s, 1, full(&image, 1)).unwrap();
    none.render_region(&image, &s, 1, full(&image, 1)).unwrap();
    assert_eq!(none.cache().bytes(), 0);
    assert_eq!(ops.count(StageId::Demosaic), 18);
}

#[test]
fn progressive_render_delivers_coarse_first() {
    let image = bayer_image();
    let s = DevelopSettings::default();
    let r = renderer(4);
    let viewport = Viewport::new(PixelRect::new(100, 50, 500, 400));
    let mut levels = Vec::new();
    r.render_progressive(
        &image,
        &s,
        &viewport,
        RenderOutput::Display,
        &CancellationToken::new(),
        &mut |t| levels.push(t.coord().level),
    )
    .unwrap();
    assert_eq!(levels.first(), Some(&3));
    assert_eq!(levels.last(), Some(&0));
    assert!(levels.windows(2).all(|w| w[0] >= w[1]), "{levels:?}");
    for level in 0..=3 {
        let expected = Renderer::tiles_for(&image, level, viewport.rect.at_level(level)).len();
        assert_eq!(levels.iter().filter(|&&l| l == level).count(), expected);
    }
}

#[test]
fn progressive_render_is_cancellable_between_tiles() {
    let image = bayer_image();
    let s = DevelopSettings::default();
    let (r, ops) = counting(2, 256 << 20);
    let cancel = CancellationToken::new();
    let mut delivered = 0;
    let result = r.render_progressive(
        &image,
        &s,
        &Viewport::new(full(&image, 0)),
        RenderOutput::Display,
        &cancel,
        &mut |_| {
            delivered += 1;
            cancel.cancel();
        },
    );
    assert_eq!(result, Err(EngineError::Cancelled));
    assert_eq!(delivered, 1);
    // Only the coarse level's work was done.
    assert_eq!(ops.count(StageId::Output), 1);

    let pre = CancellationToken::new();
    pre.cancel();
    let (r, ops) = counting(2, 256 << 20);
    let result = r.render_progressive(
        &image,
        &s,
        &Viewport::new(full(&image, 0)),
        RenderOutput::Display,
        &pre,
        &mut |_| panic!("no tile after cancellation"),
    );
    assert_eq!(result, Err(EngineError::Cancelled));
    assert_eq!(ops.counts().iter().sum::<u64>(), 0);
}

#[test]
fn progressive_job_runs_on_the_scheduler() {
    let image = bayer_image();
    let pool = jobs::ThreadPoolScheduler::new(2);
    let renderer = Arc::new(renderer(2));
    let (tx, rx) = mpsc::channel();
    let tx = std::sync::Mutex::new(tx);
    let handle = pool.submit(
        Box::new(ProgressiveRenderJob {
            renderer: renderer.clone(),
            image: image.clone(),
            settings: DevelopSettings::default(),
            viewport: Viewport {
                rect: full(&image, 0),
                finest_level: 1,
                coarsest_level: 2,
            },
            output: RenderOutput::Display,
            priority: Priority::Viewport,
            sink: Box::new(move |t| tx.lock().unwrap().send(t.coord()).unwrap()),
        }),
        None,
    );
    let deadline = Instant::now() + Duration::from_secs(60);
    while pool.status(handle.id) != JobStatus::Succeeded {
        assert!(Instant::now() < deadline, "{:?}", pool.status(handle.id));
        std::thread::sleep(Duration::from_millis(5));
    }
    let coords: Vec<TileCoord> = rx.try_iter().collect();
    assert_eq!(coords.len(), 1 + 4);
    assert_eq!(coords[0].level, 2);

    // A cancelled queued render never starts.
    let started = Arc::new(AtomicUsize::new(0));
    let seen = started.clone();
    let token = CancellationToken::new();
    token.cancel();
    let handle = pool.submit(
        Box::new(ProgressiveRenderJob {
            renderer,
            image,
            settings: DevelopSettings::default(),
            viewport: Viewport::new(PixelRect::new(0, 0, 10, 10)),
            output: RenderOutput::Display,
            priority: Priority::Viewport,
            sink: Box::new(move |_| {
                seen.fetch_add(1, Ordering::SeqCst);
            }),
        }),
        Some(&token),
    );
    while pool.status(handle.id) != JobStatus::Cancelled {
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(5));
    }
    assert_eq!(started.load(Ordering::SeqCst), 0);
}

#[test]
fn rejects_unsupported_settings_and_bad_requests() {
    let image = bayer_image();
    let r = renderer(1);
    let mut s = DevelopSettings::default();
    s.output.hdr = true;
    assert!(r.render_region(&image, &s, 0, full(&image, 0)).is_err());
    let s = DevelopSettings::default();
    let bad = [TileCoord::new(0, 0, 0), TileCoord::new(1, 0, 0)];
    let mut sink = |_| {};
    assert!(
        r.render_tiles(
            &image,
            &s,
            &bad,
            RenderOutput::Display,
            &CancellationToken::new(),
            &mut sink
        )
        .is_err()
    );
    let outside = [TileCoord::new(0, 9, 0)];
    assert!(
        r.render_tiles(
            &image,
            &s,
            &outside,
            RenderOutput::Display,
            &CancellationToken::new(),
            &mut sink
        )
        .is_err()
    );
    let v = Viewport {
        rect: full(&image, 0),
        finest_level: 2,
        coarsest_level: 1,
    };
    assert!(
        r.render_progressive(
            &image,
            &s,
            &v,
            RenderOutput::Display,
            &CancellationToken::new(),
            &mut sink
        )
        .is_err()
    );
}
