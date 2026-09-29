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

// ---- Stage B call-site tests (B1-B11, B13). Each leads with
// `assert!(v.begin.is_some(), "WB-RED: no Begin recorded")` (rev7 D5). ----

use crate::wb_diagnostic::{
    ARENA, BeginContext, Bucket, Counts, Identity, Lease, Outcome, PhaseKind, Reach, Record,
    RequestContext, Route, RouteState, SlotMeta, Snapshot, reach,
};
use engine_api::{EngineError, EngineResult, stage::MemoKey};

const RED: &str = "WB-RED: no Begin recorded";

/// Owned copy of one drained transaction (test storage, outside the ledger).
struct Capture {
    meta: SlotMeta,
    records: Vec<Record>,
    overflow: u64,
    context: crate::wb_diagnostic::Context,
    reach: [Reach; 3],
}
impl Capture {
    fn begin(&self) -> Option<BeginContext> {
        self.meta.begin
    }
    fn body(&self) -> &[Record] {
        let n = self.records.len();
        if n >= 2 { &self.records[1..n - 1] } else { &[] }
    }
    fn outcomes(&self) -> Vec<(Bucket, Outcome)> {
        self.records.iter().map(|r| (r.bucket, r.outcome)).collect()
    }
}
fn reach_all(s: &Snapshot) -> [Reach; 3] {
    let identity = s.records[0].map_or(
        Identity {
            operator: 0,
            phase: 0,
            transaction: 0,
            generation: 0,
        },
        |r| r.identity,
    );
    [Bucket::Detail, Bucket::PaddedWb, Bucket::TileWb].map(|b| reach(s, identity, b).unwrap())
}
/// Drains exactly one completed transaction from the guard's epoch.
fn drain_one(g: &EpochGuard) -> Capture {
    let d = g.epoch().drain().expect("one completed transaction");
    d.inspect(|v| Capture {
        meta: *v.meta,
        records: v.snapshot.records[..v.snapshot.len]
            .iter()
            .flatten()
            .copied()
            .collect(),
        overflow: v.snapshot.overflow,
        context: v.snapshot.context,
        reach: reach_all(v.snapshot),
    })
}
fn request_ctx(operator: u64, level: u8) -> RequestContext {
    RequestContext::new([0; 32], [0; 32], 0, 1, 0, level, PhaseKind::Test, operator)
}
/// An armed epoch, an operator-tagged renderer and one bound lease.
fn bound(g: &EpochGuard, m: &Arc<Model>, level: u8) -> (Renderer, Lease, u64) {
    g.epoch().arm(1).unwrap();
    let operator = ARENA.next_operator();
    assert_ne!(operator, 0);
    let r = renderer(m).with_diagnostic_operator(operator);
    let lease = ARENA
        .reserve_armed(request_ctx(operator, level))
        .expect("armed reservation")
        .bind();
    (r, lease, operator)
}
/// Product-style completion: Some -> Delivered, None -> Declined, Err -> Abort.
fn complete<T>(lease: Lease, out: &EngineResult<Option<T>>) {
    match out {
        Ok(Some(_)) => lease.finish(Route::Delivered),
        Ok(None) => lease.finish(Route::Declined),
        Err(_) => drop(lease),
    }
}
/// Harness conclusiveness (Stage A review NB-1, NB-6).
fn assert_attributable(c: &Capture, counts: &Counts) {
    assert!(!harness::epoch_inconclusive(counts), "{counts:?}");
    assert!(harness::operator_matches(&c.meta));
    assert_eq!(c.meta.route, RouteState::Delivered);
    assert_eq!(c.overflow, 0);
    for r in &c.records {
        assert_eq!(r.identity, c.records[0].identity);
    }
    assert_eq!(
        c.records[0].identity.operator,
        c.meta.request.expected_operator
    );
}
/// The exact cold level-route sequence (B1/B7/B13 body).
fn assert_cold_level_sequence(c: &Capture, terminal: Outcome) {
    use Bucket::*;
    let mut expected = vec![
        (Detail, Outcome::Begin),
        (Detail, Outcome::None),
        (PaddedWb, Outcome::None),
    ];
    for _ in 0..35 {
        expected.extend([(TileWb, Outcome::None), (TileWb, Outcome::Request)]);
    }
    expected.extend([
        (PaddedWb, Outcome::Request),
        (Detail, Outcome::Request),
        (Detail, terminal),
    ]);
    assert_eq!(c.records.len(), 76);
    assert_eq!(c.outcomes(), expected);
    // Each lookup miss is followed by the Request of the same exact key.
    let body = c.body();
    for (i, r) in body.iter().enumerate() {
        if r.outcome == Outcome::Request {
            assert!(
                body[..i]
                    .iter()
                    .any(|p| p.key == r.key && p.outcome == Outcome::None)
            );
        }
    }
}
fn instrumented(key: &MemoKey) -> bool {
    matches!(key.stage, StageId::WhiteBalance | StageId::Detail)
}
fn wb_bits_of(image: &RawImage, settings: &DevelopSettings) -> [u64; 9] {
    let m = image.metadata();
    let camera_xyz =
        pipeline_cpu::camera_to_xyz(engine_api::color::ColorMatrix3(std::array::from_fn(|r| {
            m.cam_xyz[r].map(f64::from)
        })))
        .unwrap();
    let wb = pipeline_cpu::white_balance_matrix(&settings.white_balance, camera_xyz, m.as_shot_wb)
        .unwrap()
        .0;
    [
        wb[0][0], wb[0][1], wb[0][2], wb[1][0], wb[1][1], wb[1][2], wb[2][0], wb[2][1], wb[2][2],
    ]
    .map(f64::to_bits)
}
fn digest_of(bits: [u64; 9]) -> [u8; 32] {
    let bytes: Vec<u8> = bits.iter().flat_map(|b| b.to_le_bytes()).collect();
    engine_api::id::Digest::derive("tessera wb-diagnostic resolved-wb v1", &bytes).0
}

/// B1: cold level-mode render gives exactly the 76-record transaction, and
/// its Request keys are the model's instrumented `cache_exact` keys.
#[test]
fn b01_cold_level_render_records_exact_sequence() {
    let g = EpochGuard::open().expect(GUARD);
    let (image, settings) = fixture();
    let all = coords(&image, &settings, 0);
    let m = model(true);
    let (r, lease, _) = bound(&g, &m, 0);
    let out = render(&r, &image, &settings, &all, Some(lease.token()));
    complete(lease, &out);
    let c = drain_one(&g);
    assert!(c.begin().is_some(), "{RED}");
    out.unwrap().unwrap();
    assert_attributable(&c, &ARENA.counts().unwrap());
    assert_cold_level_sequence(&c, Outcome::End);
    let requests: Vec<MemoKey> = c
        .body()
        .iter()
        .filter(|r| r.outcome == Outcome::Request)
        .map(|r| r.key)
        .collect();
    let log: Vec<MemoKey> = m
        .cache_exact_log
        .lock()
        .unwrap()
        .iter()
        .copied()
        .filter(instrumented)
        .collect();
    assert_eq!(requests, log);
    assert_eq!(c.reach, [Reach::Miss, Reach::Miss, Reach::Miss]);
}

/// B2: the warm render is a Detail hit: 3 records; Padded/Tile NotReached.
#[test]
fn b02_warm_level_render_is_detail_hit() {
    let g = EpochGuard::open().expect(GUARD);
    let (image, settings) = fixture();
    let all = coords(&image, &settings, 0);
    let m = model(true);
    render(&renderer(&m), &image, &settings, &all, None)
        .unwrap()
        .unwrap();
    let (r, lease, _) = bound(&g, &m, 0);
    let out = render(&r, &image, &settings, &all, Some(lease.token()));
    complete(lease, &out);
    let c = drain_one(&g);
    assert!(c.begin().is_some(), "{RED}");
    assert_attributable(&c, &ARENA.counts().unwrap());
    assert_eq!(
        c.outcomes(),
        [
            (Bucket::Detail, Outcome::Begin),
            (Bucket::Detail, Outcome::Some),
            (Bucket::Detail, Outcome::End),
        ]
    );
    assert_eq!(c.reach, [Reach::Hit, Reach::NotReached, Reach::NotReached]);
}

/// B3: Detail frame removed, padded frame present: 5 records, padded hit.
#[test]
fn b03_detail_evicted_padded_present_is_padded_hit() {
    let g = EpochGuard::open().expect(GUARD);
    let (image, settings) = fixture();
    let all = coords(&image, &settings, 0);
    let m = model(true);
    render(&renderer(&m), &image, &settings, &all, None)
        .unwrap()
        .unwrap();
    m.frames
        .lock()
        .unwrap()
        .retain(|k, _| k.stage != StageId::Detail);
    let (r, lease, _) = bound(&g, &m, 0);
    let out = render(&r, &image, &settings, &all, Some(lease.token()));
    complete(lease, &out);
    let c = drain_one(&g);
    assert!(c.begin().is_some(), "{RED}");
    assert_attributable(&c, &ARENA.counts().unwrap());
    assert_eq!(
        c.outcomes(),
        [
            (Bucket::Detail, Outcome::Begin),
            (Bucket::Detail, Outcome::None),
            (Bucket::PaddedWb, Outcome::Some),
            (Bucket::Detail, Outcome::Request),
            (Bucket::Detail, Outcome::End),
        ]
    );
    assert_eq!(c.reach, [Reach::Miss, Reach::Hit, Reach::NotReached]);
}

/// B4: an injected WB lookup error is recorded once, propagated unchanged,
/// and the dropped lease records Abort. Lookups are identical without a token.
#[test]
fn b04_lookup_error_is_recorded_propagated_and_aborts() {
    let g = EpochGuard::open().expect(GUARD);
    let (image, settings) = fixture();
    let all = coords(&image, &settings, 0);
    let failing = || {
        Arc::new(Model {
            level_mode: true,
            fail_cached: Some(StageId::WhiteBalance),
            ..Default::default()
        })
    };
    let (unbound_model, bound_model) = (failing(), failing());
    let plain = render(&renderer(&unbound_model), &image, &settings, &all, None);
    let (r, lease, _) = bound(&g, &bound_model, 0);
    let out = render(&r, &image, &settings, &all, Some(lease.token()));
    complete(lease, &out);
    let c = drain_one(&g);
    assert!(c.begin().is_some(), "{RED}");
    let (Err(plain), Err(observed)) = (plain, out) else {
        panic!("injected lookup error must propagate");
    };
    assert!(matches!(&observed, EngineError::Internal { message } if message == "injected"));
    assert_eq!(plain.to_string(), observed.to_string());
    assert_eq!(
        c.outcomes(),
        [
            (Bucket::Detail, Outcome::Begin),
            (Bucket::Detail, Outcome::None),
            (Bucket::PaddedWb, Outcome::Error),
            (Bucket::Detail, Outcome::Abort),
        ]
    );
    assert_eq!(c.meta.route, RouteState::Aborted);
    assert_eq!(
        *unbound_model.cached_calls.lock().unwrap(),
        *bound_model.cached_calls.lock().unwrap()
    );
}

/// B5: at each instrumented `cache_exact`, the last arena record is the
/// Request of that exact key.
#[test]
fn b05_request_is_recorded_immediately_before_each_cache_exact() {
    let g = EpochGuard::open().expect(GUARD);
    let (image, settings) = fixture();
    let all = coords(&image, &settings, 0);
    let m = model(true);
    let (r, lease, _) = bound(&g, &m, 0);
    *m.wb_probe.lock().unwrap() = Some(lease.token());
    let out = render(&r, &image, &settings, &all, Some(lease.token()));
    *m.wb_probe.lock().unwrap() = None;
    complete(lease, &out);
    let c = drain_one(&g);
    assert!(c.begin().is_some(), "{RED}");
    let probes: Vec<_> = m
        .wb_probe_log
        .lock()
        .unwrap()
        .iter()
        .copied()
        .filter(|(k, _)| instrumented(k))
        .collect();
    assert_eq!(probes.len(), 37);
    for (key, last) in probes {
        let last = last.expect("a record precedes every instrumented cache_exact");
        assert_eq!((last.outcome, last.key), (Outcome::Request, key));
    }
}

/// B6: Begin carries the resolved WB bits and the S1 digest reaches the
/// drained context; Daylight and AsShot differ.
#[test]
fn b06_begin_carries_resolved_wb_bits_and_digest() {
    let g = EpochGuard::open().expect(GUARD);
    let (image, as_shot) = fixture();
    let mut daylight = as_shot.clone();
    daylight.white_balance.mode = engine_api::recipe::settings::WhiteBalanceMode::Daylight;
    let all = coords(&image, &as_shot, 0);
    let mut seen = Vec::new();
    for settings in [&as_shot, &daylight] {
        let m = model(true);
        let (r, lease, _) = bound(&g, &m, 0);
        let out = render(&r, &image, settings, &all, Some(lease.token()));
        complete(lease, &out);
        let c = drain_one(&g);
        assert!(c.begin().is_some(), "{RED}");
        let b = c.begin().unwrap();
        assert_eq!(b.wb_bits, wb_bits_of(&image, settings));
        assert_eq!(b.wb_digest, digest_of(b.wb_bits));
        assert_eq!(c.context.resolved_wb, b.wb_digest);
        assert_eq!(b.output_tag, 1);
        seen.push(b.wb_bits);
        g.epoch().disarm().unwrap();
    }
    assert_ne!(seen[0], seen[1]);
}

/// B7: a coarse L1 request develops the shared L0 level: Begin records
/// render level 1 while every lookup key is L0.
#[test]
fn b07_coarse_level_one_records_level_zero_keys() {
    let g = EpochGuard::open().expect(GUARD);
    let (image, settings) = fixture();
    let l1 = coords(&image, &settings, 1);
    let m = model(true);
    let (r, lease, _) = bound(&g, &m, 1);
    let out = render(&r, &image, &settings, &l1, Some(lease.token()));
    complete(lease, &out);
    let c = drain_one(&g);
    assert!(c.begin().is_some(), "{RED}");
    let tiles = out.unwrap().expect("coarse route").tiles;
    assert_eq!(tiles.len(), l1.len());
    assert!(tiles.iter().all(|t| t.coord().level == 1));
    assert_eq!(c.begin().unwrap().render_level, 1);
    assert_eq!(gathers(&m), 1);
    assert_attributable(&c, &ARENA.counts().unwrap());
    assert_cold_level_sequence(&c, Outcome::End);
    assert!(c.body().iter().all(|r| r.key.tile.level == 0));
}

/// B8: uncacheable WB/Detail give no L1/L2 records (Inconclusive); a mapped
/// tail and the capability query give no Begin (Unobserved).
#[test]
fn b08_uncacheable_mapped_and_capability_paths() {
    let g = EpochGuard::open().expect(GUARD);
    let (image, settings) = fixture();
    let all = coords(&image, &settings, 0);
    // Sub-case 1: cacheable = false (Begin at GREEN).
    let m = model(true);
    let (_, lease, operator) = bound(&g, &m, 0);
    let config = RendererConfig {
        graph: crate::PipelineGraph::m2()
            .with_cacheable(StageId::WhiteBalance, false)
            .with_cacheable(StageId::Detail, false),
        ..RendererConfig::default()
    };
    let r = Renderer::with_ops(m.clone(), Arc::new(TileCache::new(0)), config)
        .with_diagnostic_operator(operator);
    let out = render(&r, &image, &settings, &all, Some(lease.token()));
    complete(lease, &out);
    let c = drain_one(&g);
    assert!(c.begin().is_some(), "{RED}");
    assert!(c.body().iter().all(|r| r.bucket == Bucket::TileWb));
    assert_eq!(c.body().len(), 70);
    assert_eq!(
        c.reach,
        [
            Reach::Inconclusive,
            Reach::Inconclusive,
            Reach::Inconclusive
        ]
    );
    // Sub-case 2: a mapped tail declines before binding.
    let mut mapped = settings.clone();
    mapped.geometry.crop.rect.right = 0.91;
    mapped.geometry.crop.angle = 4.;
    let lease = ARENA
        .reserve_armed(request_ctx(operator, 0))
        .unwrap()
        .bind();
    let mapped_coords = coords(&image, &mapped, 0);
    let out = render(&r, &image, &mapped, &mapped_coords, Some(lease.token()));
    assert!(matches!(out, Ok(None)));
    complete(lease, &out);
    let c = drain_one(&g);
    assert_eq!(c.begin(), None);
    assert_eq!(c.meta.route, RouteState::Unobserved);
    assert_eq!(c.records.len(), 1);
    // Sub-case 3: the capability query never binds.
    let lease = ARENA
        .reserve_armed(request_ctx(operator, 0))
        .unwrap()
        .bind();
    assert!(
        renderer(&model(true))
            .can_render_resident(&image, &settings)
            .unwrap()
    );
    lease.finish(Route::Delivered);
    let c = drain_one(&g);
    assert_eq!(c.begin(), None);
    assert_eq!(c.meta.route, RouteState::Unobserved);
    assert_eq!(c.records.len(), 1);
}

/// B9: without level support, the L1 surface path binds, then declines at
/// the coarse capability check; the region path makes no resident WB lookup.
#[test]
fn b09_level_one_without_level_support_declines() {
    let g = EpochGuard::open().expect(GUARD);
    let (image, settings) = fixture();
    let m = model(false);
    let (r, lease, _) = bound(&g, &m, 1);
    let out = r.render_surface_as_observed(
        &image,
        &settings,
        1,
        0,
        RenderOutput::Display,
        &CancellationToken::new(),
        Some(lease.token()),
    );
    complete(lease, &out);
    let c = drain_one(&g);
    assert!(c.begin().is_some(), "{RED}");
    assert!(matches!(out, Ok(None)));
    assert_eq!(c.meta.route, RouteState::Declined);
    assert_eq!(
        c.outcomes(),
        [
            (Bucket::Detail, Outcome::Begin),
            (Bucket::Detail, Outcome::End),
        ]
    );
    let extent = Renderer::output_extent(&image, &settings, 1).unwrap();
    let tiles = r
        .render_region(&image, &settings, 1, crate::PixelRect::full(extent))
        .unwrap();
    assert!(!tiles.is_empty());
    assert_eq!(calls(&m, StageId::WhiteBalance), 0);
    assert_eq!(gathers(&m), 0);
}

/// B10: partial coordinates take the tile route: TileWb records only, which
/// the interpreter reports as Inconclusive.
#[test]
fn b10_partial_coords_on_tile_route_are_inconclusive() {
    let g = EpochGuard::open().expect(GUARD);
    let (image, settings) = fixture();
    let all = coords(&image, &settings, 0);
    let m = model(true);
    let (r, lease, _) = bound(&g, &m, 0);
    let out = render(&r, &image, &settings, &all[..3], Some(lease.token()));
    complete(lease, &out);
    let c = drain_one(&g);
    assert!(c.begin().is_some(), "{RED}");
    assert_eq!(out.unwrap().unwrap().tiles.len(), 3);
    assert_eq!(gathers(&m), 0);
    assert!(!c.body().is_empty());
    assert!(c.body().iter().all(|r| r.bucket == Bucket::TileWb));
    assert_eq!(
        c.reach,
        [
            Reach::Inconclusive,
            Reach::Inconclusive,
            Reach::Inconclusive
        ]
    );
}

/// B11: an unbound render while an outer lease is live adds nothing to it.
#[test]
fn b11_unbound_render_during_bound_lease_adds_nothing() {
    let g = EpochGuard::open().expect(GUARD);
    let (image, settings) = fixture();
    let all = coords(&image, &settings, 0);
    let m = model(true);
    let (r, lease, _) = bound(&g, &m, 0);
    let out = render(&r, &image, &settings, &all, Some(lease.token()));
    let other = model(true);
    std::thread::scope(|s| {
        s.spawn(|| {
            render(&renderer(&other), &image, &settings, &all, None)
                .unwrap()
                .unwrap();
        });
    });
    complete(lease, &out);
    let c = drain_one(&g);
    assert!(c.begin().is_some(), "{RED}");
    assert_eq!(calls(&other, StageId::WhiteBalance), 36);
    assert_attributable(&c, &ARENA.counts().unwrap());
    assert_cold_level_sequence(&c, Outcome::End);
    assert!(g.epoch().drain().is_err());
}

/// B13: `render_surface_as_observed` forwards the token; the model rejects
/// surfaces in `finish`, so the transaction is Aborted after full lookups.
#[test]
fn b13_render_surface_as_observed_forwards_token() {
    let g = EpochGuard::open().expect(GUARD);
    let (image, settings) = fixture();
    let m = model(true);
    let (r, lease, _) = bound(&g, &m, 0);
    let out = r.render_surface_as_observed(
        &image,
        &settings,
        0,
        0,
        RenderOutput::Display,
        &CancellationToken::new(),
        Some(lease.token()),
    );
    complete(lease, &out);
    let c = drain_one(&g);
    assert!(c.begin().is_some(), "{RED}");
    assert!(out.is_err());
    assert_eq!(c.meta.route, RouteState::Aborted);
    assert_cold_level_sequence(&c, Outcome::Abort);
}
