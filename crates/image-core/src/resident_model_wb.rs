//! Stage B WB diagnostic call-site tests on the CPU resident model (rev7 6.2).
//! No GPU. The shared static `ARENA` requires `--test-threads=1`, which
//! `EpochGuard::open` enforces.
use super::{Model, common};
use crate::{RawImage, RenderOutput, Renderer, RendererConfig, TileCache, wb_diagnostic::harness};
use engine_api::{
    id::ImageId,
    jobs::CancellationToken,
    recipe::{DevelopSettings, ProcessVersion},
    stage::StageId,
    tile::{TILE_SIZE, TileCoord},
};
use harness::EpochGuard;
use pipeline_cpu::{CameraLinearProxy, LensContext};
use std::{
    sync::{Arc, atomic::Ordering, mpsc},
    time::{Duration, Instant},
};

const GUARD: &str = "EpochGuard: requires --test-threads=1 and an Idle ARENA \
                     (an earlier test may have leaked a live lease)";

/// rev7 NOTE-3: raw default crop [1, 1, w-2, h-2] = 1640x1092; at scale 1 the
/// proxy is 1640x1092, a 7x5 grid of 35 L0 tiles (before orientation 6).
fn fixture() -> (RawImage, DevelopSettings) {
    let (w, h) = (1642, 1094);
    let raw = common::synthetic(901, w, h, common::RGGB, [1, 1, w - 2, h - 2]);
    let mut metadata = raw.metadata().clone();
    metadata.orientation = 6;
    let raw = raw.with_metadata(ImageId(902), Arc::new(metadata)).unwrap();
    let settings = DevelopSettings::default();
    let proxy = CameraLinearProxy::generate(
        raw.cfa(),
        raw.metadata(),
        &settings,
        ProcessVersion::NATIVE_CURRENT,
        [7; 32],
        &LensContext::default(),
    )
    .unwrap();
    let preview =
        RawImage::from_camera_linear_proxy(raw.id(), ImageId(903), Arc::new(proxy)).unwrap();
    (preview, settings)
}
fn model(level_mode: bool) -> Arc<Model> {
    Arc::new(Model {
        level_mode,
        ..Default::default()
    })
}
fn renderer(model: &Arc<Model>) -> Renderer {
    Renderer::with_ops(
        model.clone(),
        Arc::new(TileCache::new(0)),
        RendererConfig::default(),
    )
}
fn coords(image: &RawImage, settings: &DevelopSettings, level: u8) -> Vec<TileCoord> {
    let (gw, gh) = Renderer::output_extent(image, settings, level)
        .unwrap()
        .tile_grid(TILE_SIZE);
    (0..gh)
        .flat_map(|y| (0..gw).map(move |x| TileCoord::new(level, x, y)))
        .collect()
}
fn calls(model: &Model, stage: StageId) -> u32 {
    model.cached_calls.lock().unwrap()[stage.index()]
}
fn gathers(model: &Model) -> u32 {
    model.gather_level_calls.load(Ordering::Relaxed)
}
fn render(
    r: &Renderer,
    image: &RawImage,
    settings: &DevelopSettings,
    coords: &[TileCoord],
    diag: Option<crate::wb_diagnostic::Token>,
) -> engine_api::EngineResult<Option<crate::resident::ResidentOutput>> {
    r.camera_linear_resident_for_test(
        image,
        settings,
        coords,
        RenderOutput::Display,
        &CancellationToken::new(),
        diag,
    )
}

// Pre-RED sanity (rev7 6.2): must pass from B0 onward.

#[test]
fn sanity_level_route_cold_counts() {
    let _g = EpochGuard::open().expect(GUARD);
    let (image, settings) = fixture();
    let all = coords(&image, &settings, 0);
    assert_eq!(all.len(), 35);
    let m = model(true);
    let r = renderer(&m);
    assert!(render(&r, &image, &settings, &all, None).unwrap().is_some());
    assert_eq!(gathers(&m), 1);
    assert_eq!(calls(&m, StageId::Detail), 1);
    assert_eq!(calls(&m, StageId::WhiteBalance), 36);
    assert_eq!(calls(&m, StageId::Demosaic), 35);
}

#[test]
fn sanity_level_route_warm_hits_detail() {
    let _g = EpochGuard::open().expect(GUARD);
    let (image, settings) = fixture();
    let all = coords(&image, &settings, 0);
    let m = model(true);
    let r = renderer(&m);
    render(&r, &image, &settings, &all, None).unwrap().unwrap();
    render(&r, &image, &settings, &all, None).unwrap().unwrap();
    assert_eq!(calls(&m, StageId::Detail), 2);
    assert_eq!(calls(&m, StageId::WhiteBalance), 36);
    assert_eq!(calls(&m, StageId::Demosaic), 35);
    assert_eq!(gathers(&m), 1);
}

#[test]
fn sanity_tile_route_without_level_mode() {
    let _g = EpochGuard::open().expect(GUARD);
    let (image, settings) = fixture();
    let all = coords(&image, &settings, 0);
    let m = model(false);
    let r = renderer(&m);
    assert!(render(&r, &image, &settings, &all, None).unwrap().is_some());
    assert_eq!(gathers(&m), 0);
    assert_eq!(calls(&m, StageId::WhiteBalance), 35);
}

/// B12 (non-regression guard, not RED): a render with `diag = None` never
/// touches the arena, so it completes while another thread holds its lock.
#[test]
fn b12_unbound_render_never_touches_arena_lock() {
    let _g = EpochGuard::open().expect(GUARD);
    let (image, settings) = fixture();
    let all = coords(&image, &settings, 0);
    let m = model(true);
    let r = renderer(&m);
    render(&r, &image, &settings, &all, None).unwrap().unwrap();
    let (held_tx, held_rx) = mpsc::channel();
    let (done_tx, done_rx) = mpsc::channel::<()>();
    let elapsed = std::thread::scope(|s| {
        s.spawn(move || {
            crate::wb_diagnostic::ARENA.hold_lock_for_test(|| {
                held_tx.send(()).unwrap();
                let _ = done_rx.recv_timeout(Duration::from_secs(10));
            });
        });
        held_rx.recv_timeout(Duration::from_secs(2)).unwrap();
        let start = Instant::now();
        let out = render(&r, &image, &settings, &all, None);
        let elapsed = start.elapsed();
        done_tx.send(()).unwrap();
        out.unwrap().unwrap();
        elapsed
    });
    assert!(
        elapsed < Duration::from_secs(2),
        "unbound render took {elapsed:?} while the arena lock was held"
    );
}
