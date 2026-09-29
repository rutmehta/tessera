//! Measured develop-backend selection. Explicit overrides skip calibration.

pub(crate) mod proxy_decision_cache;

use engine_api::{
    EngineResult,
    recipe::{DevelopSettings, settings::WhiteBalanceMode},
};
use image_core::{CpuStageOp, PixelRect, RawImage, Renderer, RendererConfig, StageOp, TileCache};
use std::{sync::Arc, time::Instant};

// Settings-only engine history cannot carry a recipe field directly. Typed
// transition metadata on real history entries preserves the existing schema.
const PROCESS_MARKER: &str = "tessera:process-version:v1:";

pub(crate) fn record_process(
    recipe: &mut engine_api::recipe::Recipe,
    version: engine_api::recipe::ProcessVersion,
    timestamp: i64,
) -> EngineResult<bool> {
    use engine_api::{
        id::HistoryEntryId,
        recipe::{EditMeta, history::HistoryEntry},
    };
    if recipe.process_version == version {
        return Ok(false);
    }
    let id = HistoryEntryId(recipe.history.entries.len() as u64 + 1);
    let rationale = format!(
        "{PROCESS_MARKER}{}",
        serde_json::to_string(&(recipe.process_version, version))?
    );
    recipe.history.entries.push(HistoryEntry {
        id,
        parent: recipe.history.head,
        meta: EditMeta {
            rationale: Some(rationale),
            ..EditMeta::user("Process Version", timestamp)
        },
        changes: Vec::new(),
    });
    recipe.history.head = Some(id);
    recipe.process_version = version;
    Ok(true)
}

pub(crate) fn sync_process(recipe: &mut engine_api::recipe::Recipe) -> EngineResult<()> {
    use engine_api::recipe::{ProcessVersion, history::HistoryEntry};
    fn transition(entry: &HistoryEntry) -> EngineResult<Option<(ProcessVersion, ProcessVersion)>> {
        entry
            .meta
            .rationale
            .as_deref()
            .and_then(|v| v.strip_prefix(PROCESS_MARKER))
            .map(|v| serde_json::from_str(v).map_err(Into::into))
            .transpose()
    }
    let mut version = None;
    for entry in &recipe.history.entries {
        if let Some((before, _)) = transition(entry)? {
            version = Some(before);
            break;
        }
    }
    for entry in recipe.history.lineage(recipe.history.head)? {
        if let Some((_, after)) = transition(entry)? {
            version = Some(after);
        }
    }
    if let Some(version) = version {
        recipe.process_version = version;
    }
    Ok(())
}

fn gpu_is_faster(cpu: [f64; 3], gpu: [f64; 3]) -> bool {
    cpu.iter().chain(&gpu).all(|v| v.is_finite() && *v > 0.) && gpu[1] < cpu[1] && gpu[2] < cpu[2]
}
#[cfg(test)]
fn measure(renderer: &Renderer, image: &RawImage) -> EngineResult<[f64; 3]> {
    measure_at(
        renderer,
        image,
        &DevelopSettings::default(),
        2,
        #[cfg(feature = "wb-diagnostic")]
        false,
    )
}
/// Calibration presentation: `render_surface`, spelled out as its own
/// `render_surface_as(…, Display, …)` delegation.
#[cfg(not(all(test, feature = "wb-diagnostic")))]
fn surface_render(
    renderer: &Renderer,
    image: &RawImage,
    settings: &DevelopSettings,
    level: u8,
    surface: u32,
    cancel: &engine_api::jobs::CancellationToken,
) -> EngineResult<Option<image_core::resident::DisplayHistogram>> {
    renderer.render_surface(image, settings, level, surface, cancel)
}
/// WB diagnostic: the same presentation carrying an attribution token.
#[cfg(all(test, feature = "wb-diagnostic"))]
fn surface_render(
    renderer: &Renderer,
    image: &RawImage,
    settings: &DevelopSettings,
    level: u8,
    surface: u32,
    cancel: &engine_api::jobs::CancellationToken,
    diag: Option<image_core::wb_diagnostic::Token>,
) -> EngineResult<Option<image_core::resident::DisplayHistogram>> {
    renderer.render_surface_as_observed(
        image,
        settings,
        level,
        surface,
        image_core::RenderOutput::Display,
        cancel,
        diag,
    )
}
fn measure_at(
    renderer: &Renderer,
    image: &RawImage,
    settings: &DevelopSettings,
    level: u8,
    #[cfg(all(test, feature = "wb-diagnostic"))] observe: bool,
) -> EngineResult<[f64; 3]> {
    let mut s = settings.clone();
    let extent = Renderer::output_extent(image, &s, level)?;
    let rect = PixelRect::full(extent);
    // Match the actual develop sink on both backends. Timing GPU pixel
    // readback here can reject a faster zero-copy presentation path.
    // Allocation is not a per-frame cost and stays outside the timer.
    let surface = crate::surface::Surface::create_rgba8(extent.width, extent.height)
        .map_err(engine_api::EngineError::internal)?;
    let cancel = engine_api::jobs::CancellationToken::new();
    let mut times = [0.; 3];
    for (i, time) in times.iter_mut().enumerate() {
        if i == 1 {
            s.tone.exposure += 0.25;
        }
        if i == 2 {
            s.white_balance.mode = if matches!(s.white_balance.mode, WhiteBalanceMode::Daylight) {
                WhiteBalanceMode::AsShot
            } else {
                WhiteBalanceMode::Daylight
            };
        }
        // Outside the timed region: the observed (Metal) arm reserves one
        // transaction per iteration; the CPU arm only counts, while armed.
        #[cfg(all(test, feature = "wb-diagnostic"))]
        let lease = observe_iteration(renderer, image, &s, level, i, observe);
        #[cfg(all(test, feature = "wb-diagnostic"))]
        let mut surfaced = true;
        let start = Instant::now();
        let histogram = match surface_render(
            renderer,
            image,
            &s,
            level,
            surface.id(),
            &cancel,
            #[cfg(all(test, feature = "wb-diagnostic"))]
            lease.as_ref().map(image_core::wb_diagnostic::Lease::token),
        )? {
            Some(histogram) => histogram,
            None => {
                // The `render_region` fallback is not observed: Declined.
                #[cfg(all(test, feature = "wb-diagnostic"))]
                {
                    surfaced = false;
                }
                let tiles = renderer.render_region(image, &s, level, rect)?;
                crate::develop::write_level(Some(&surface), &tiles)
                    .map_err(engine_api::EngineError::internal)?
            }
        };
        std::hint::black_box(histogram);
        *time = start.elapsed().as_secs_f64() * 1000.;
        #[cfg(all(test, feature = "wb-diagnostic"))]
        if let Some(lease) = lease {
            use image_core::wb_diagnostic::Route;
            lease.finish(if surfaced {
                Route::Delivered
            } else {
                Route::Declined
            });
        }
    }
    Ok(times)
}

/// WB diagnostic calibration reservation for iteration `i` (rev7 4.2).
#[cfg(all(test, feature = "wb-diagnostic"))]
fn observe_iteration(
    renderer: &Renderer,
    image: &RawImage,
    settings: &DevelopSettings,
    level: u8,
    iteration: usize,
    observe: bool,
) -> Option<image_core::wb_diagnostic::Lease> {
    use image_core::wb_diagnostic::{
        ARENA, PhaseKind, RequestContext, Reservation, output_tag, process_identity,
        recipe_fingerprint, settings_fingerprint,
    };
    if !observe {
        ARENA.count_cpu_iteration_unobserved();
        return None;
    }
    let (tag, headroom) = output_tag(image_core::RenderOutput::Display);
    ARENA
        .reserve_armed(RequestContext::new(
            recipe_fingerprint(image.id(), settings),
            settings_fingerprint(settings),
            process_identity(renderer.config().process_version),
            tag,
            headroom,
            level,
            PhaseKind::Calibration {
                iteration: iteration as u8,
            },
            renderer.diagnostic_operator(),
        ))
        .map(Reservation::bind)
}
/// The selected develop backend: operators and the memo cache every session
/// shares. Each session builds its own [`Renderer`] over them so it can own
/// its mask raster cache and mask hooks (AI rasters, loupe overlay).
#[derive(Clone)]
pub(crate) struct Backend {
    ops: Arc<dyn StageOp>,
    cache: Arc<TileCache>,
    config: RendererConfig,
    pub(crate) name: String,
    /// WB diagnostic operator tag, copied into every renderer (rev7 2).
    #[cfg(all(test, feature = "wb-diagnostic"))]
    diag_operator: u64,
}

impl Backend {
    fn new(ops: Arc<dyn StageOp>, config: RendererConfig, name: String) -> Self {
        Self {
            ops,
            cache: Arc::new(TileCache::new(config.cache_budget_bytes)),
            config,
            name,
            #[cfg(all(test, feature = "wb-diagnostic"))]
            diag_operator: image_core::wb_diagnostic::ARENA.next_operator(),
        }
    }

    /// A renderer over the shared operators and memo cache.
    #[cfg(not(all(test, feature = "wb-diagnostic")))]
    pub(crate) fn renderer(&self) -> Renderer {
        Renderer::with_ops(self.ops.clone(), self.cache.clone(), self.config.clone())
    }

    /// A renderer over the shared operators and memo cache, carrying this
    /// backend's WB diagnostic operator tag.
    #[cfg(all(test, feature = "wb-diagnostic"))]
    pub(crate) fn renderer(&self) -> Renderer {
        Renderer::with_ops(self.ops.clone(), self.cache.clone(), self.config.clone())
            .with_diagnostic_operator(self.diag_operator)
    }

    /// WB diagnostic tests: a CPU backend (no calibration, no GPU).
    #[cfg(all(test, feature = "wb-diagnostic"))]
    pub(crate) fn cpu_for_test() -> Self {
        let config = RendererConfig::default();
        let name = format!("CPU ×{}", config.threads);
        Self::new(Arc::new(CpuStageOp), config, name)
    }
}

/// `gpu` yields the engine's shared Metal device (created on first use and
/// shared with layered-document sessions), or `None` without Metal.
pub(crate) fn select(
    image: &RawImage,
    gpu: impl FnOnce() -> Option<gpu_core::GpuDevice>,
) -> Backend {
    select_at(
        image,
        &DevelopSettings::default(),
        2,
        gpu,
        None,
        #[cfg(all(test, target_os = "macos"))]
        None,
    )
}

#[derive(Clone, Copy)]
pub(crate) struct ProxyReuse<'a> {
    pub(crate) asset: proxy_decision_cache::AssetIdentity,
    pub(crate) store: &'a proxy_decision_cache::Store,
    pub(crate) settings: &'a DevelopSettings,
    pub(crate) process: engine_api::recipe::ProcessVersion,
}

/// Explicit proxy route: actual immutable-prefix-compatible settings and default
/// screen IOSurface sink, independent of the original's backend decision.
pub(crate) fn select_proxy(
    image: &RawImage,
    settings: &DevelopSettings,
    gpu: impl FnOnce() -> Option<gpu_core::GpuDevice>,
    reuse: Option<ProxyReuse<'_>>,
    #[cfg(all(test, target_os = "macos"))] observer: Option<
        &std::sync::Mutex<crate::develop::proxy_cache_contracts::SelectionState>,
    >,
) -> Backend {
    // Match Develop's pre-surface default screen level. Explicit surface-size
    // and adaptive-level benchmarks remain required before enabling by default.
    let level = (0..=4)
        .find(|&level| {
            let extent = image.level_extent(level);
            extent.width.max(extent.height) <= 2048
        })
        .unwrap_or(4);
    // Calibration precedes the host's display-headroom probe and uses an SDR
    // ring. Match Develop's separation of presentation policy from pixel settings
    // without dropping any other unsupported recipe controls.
    let mut pixels = settings.clone();
    pixels.output.hdr = false;
    pixels.output.hdr_headroom_stops = 0.;
    select_at(
        image,
        &pixels,
        level,
        gpu,
        reuse,
        #[cfg(all(test, target_os = "macos"))]
        observer,
    )
}
fn select_at(
    image: &RawImage,
    settings: &DevelopSettings,
    level: u8,
    gpu: impl FnOnce() -> Option<gpu_core::GpuDevice>,
    reuse: Option<ProxyReuse<'_>>,
    #[cfg(all(test, target_os = "macos"))] observer: Option<
        &std::sync::Mutex<crate::develop::proxy_cache_contracts::SelectionState>,
    >,
) -> Backend {
    let config = RendererConfig::default();
    let cpu = Backend::new(
        Arc::new(CpuStageOp),
        config.clone(),
        format!("CPU ×{}", config.threads),
    );
    let preference = std::env::var("TESSERA_RENDER_BACKEND").unwrap_or_default();
    #[cfg(all(test, target_os = "macos"))]
    use crate::develop::proxy_cache_contracts::SelectionControl;
    #[cfg(all(test, target_os = "macos"))]
    let control = observer.and_then(|s| s.lock().unwrap().control);
    #[cfg(all(test, target_os = "macos"))]
    let preference = match control {
        Some(SelectionControl::ExplicitCpu) => "cpu".to_owned(),
        Some(SelectionControl::ExplicitMetal) => "gpu".to_owned(),
        None | Some(SelectionControl::ObserveAuto) => preference,
        Some(_) => String::new(),
    };
    let clear_decisions = || {
        if let Some(reuse) = reuse {
            reuse.store.clear();
        }
        #[cfg(all(test, target_os = "macos"))]
        if let Some(observer) = observer {
            observer.lock().unwrap().probe.entries = 0;
        }
    };
    if preference.eq_ignore_ascii_case("cpu") {
        return cpu;
    }
    #[cfg(all(test, target_os = "macos"))]
    if matches!(
        control,
        Some(SelectionControl::DeviceUnavailable | SelectionControl::DeviceLost)
    ) {
        // Test-only unavailable-device seam. Clear advisory decisions too.
        clear_decisions();
        return cpu;
    }
    let Some(device) = gpu() else {
        eprintln!("develop: Metal unavailable, using CPU");
        clear_decisions();
        return cpu;
    };
    if reuse.is_some() && device.device_failure().is_some() {
        clear_decisions();
        return cpu;
    }
    let device_identity = reuse.map(|_| device_identity(&device));
    let health = reuse.map(|_| device.clone());
    let ctx = match pipeline_gpu::GpuContext::from_shared(device) {
        Ok(ctx) => Arc::new(ctx),
        Err(e) => {
            eprintln!("develop: Metal operators unavailable, using CPU: {e}");
            clear_decisions();
            return cpu;
        }
    };
    #[cfg(all(test, target_os = "macos"))]
    if std::env::var_os("TESSERA_QUALIFY_ROUTE").is_some() {
        assert_eq!(ctx.adapter_info.backend, wgpu::Backend::Metal);
        eprintln!("qualification adapter: {:?}", ctx.adapter_info);
    }
    let name = format!("Metal ({})", ctx.adapter_info.name);
    let ops = Arc::new(pipeline_gpu::GpuStageOp::with_cache_budget(
        ctx,
        config.cache_budget_bytes,
    ));
    #[cfg(all(test, target_os = "macos"))]
    crate::develop::preview_qualification::capture_gpu(ops.clone());
    let gpu = Backend::new(ops, config.clone(), name);
    if preference.eq_ignore_ascii_case("gpu") {
        return gpu;
    }
    // Recheck the actual GPU candidate even for a previously measured CPU
    // winner. The temporary renderer owns no cache state retained by Store.
    let supported = reuse.is_some()
        && gpu
            .renderer()
            .can_render_resident(image, settings)
            .unwrap_or(false);
    #[cfg(all(test, target_os = "macos"))]
    if let Some(state) = observer {
        let mut state = state.lock().unwrap();
        state.probe.capability_checks += 1;
        state.probe.capability_hdr = settings.output.hdr;
        state.probe.capability_headroom = settings.output.hdr_headroom_stops;
        state.probe.capability_supported = supported;
    }
    let eligible = reuse
        .is_some_and(|r| supported && proxy_decision_cache::self_contained(r.settings, r.process));
    #[cfg(all(test, target_os = "macos"))]
    let eligible = eligible
        && observer.is_none_or(|s| !s.lock().unwrap().cache_disabled)
        && !matches!(control, Some(SelectionControl::UnversionedExternal));
    let cache_key = if eligible {
        reuse.and_then(|r| {
            let extent = Renderer::output_extent(image, settings, level).ok()?;
            proxy_decision_cache::key(&proxy_decision_cache::KeyInputs {
                asset: r.asset,
                settings: r.settings,
                recipe_process: r.process,
                config: &config,
                device: device_identity?,
                calibration_level: level,
                calibration_extent: [extent.width, extent.height],
                sink_policy_version: 1,
                selector_policy_version: 1,
            })
            .ok()
        })
    } else {
        None
    };
    #[cfg(all(test, target_os = "macos"))]
    if let Some(observer) = observer {
        observer.lock().unwrap().probe.last_key = cache_key.map(|key| key.bytes_for_test());
    }
    if let (Some(r), Some(key)) = (reuse, cache_key) {
        let decision = r.store.lookup(key);
        #[cfg(all(test, target_os = "macos"))]
        if let Some(observer) = observer {
            let entries = r.store.len();
            let mut state = observer.lock().unwrap();
            state.probe.lookups += 1;
            state.probe.hits += u64::from(decision.is_some());
            state.probe.entries = entries;
        }
        if let Some(decision) = decision {
            // Device loss may arrive while operators/key are being prepared.
            if health
                .as_ref()
                .is_some_and(|d| d.device_failure().is_some())
            {
                clear_decisions();
                return cpu;
            }
            return match decision {
                proxy_decision_cache::Decision::Cpu => cpu,
                proxy_decision_cache::Decision::Metal => gpu,
            };
        }
    }
    #[cfg(all(test, target_os = "macos"))]
    if let Some(observer) = observer {
        observer.lock().unwrap().probe.measurements += 1;
    }
    let measured = || {
        (
            measure_at(
                &cpu.renderer(),
                image,
                settings,
                level,
                #[cfg(all(test, feature = "wb-diagnostic"))]
                false,
            ),
            measure_at(
                &gpu.renderer(),
                image,
                settings,
                level,
                #[cfg(all(test, feature = "wb-diagnostic"))]
                true,
            ),
        )
    };
    #[cfg(all(test, target_os = "macos"))]
    let samples = match control {
        Some(SelectionControl::MeasuredCpu) => (Ok([1.; 3]), Ok([2.; 3])),
        Some(SelectionControl::MeasuredMetal) => (Ok([2.; 3]), Ok([1.; 3])),
        Some(SelectionControl::CalibrationFailure) => (
            Err(engine_api::EngineError::internal(
                "controlled calibration failure",
            )),
            Ok([1.; 3]),
        ),
        _ => measured(),
    };
    #[cfg(not(all(test, target_os = "macos")))]
    let samples = measured();
    match samples {
        (Ok(c), Ok(g)) => {
            if reuse.is_some()
                && health
                    .as_ref()
                    .is_some_and(|d| d.device_failure().is_some())
            {
                clear_decisions();
                return cpu;
            }
            if let (Some(r), Some(key)) = (reuse, cache_key) {
                let published = r
                    .store
                    .publish(key, proxy_decision_cache::Samples { cpu: c, gpu: g });
                #[cfg(all(test, target_os = "macos"))]
                if let Some(observer) = observer {
                    let entries = r.store.len();
                    let mut state = observer.lock().unwrap();
                    state.probe.publications += u64::from(published);
                    state.probe.entries = entries;
                }
                #[cfg(not(all(test, target_os = "macos")))]
                let _ = published;
            }
            eprintln!(
                "develop {} L{level} calibration [first, tone, WB] ms: CPU {c:?}; GPU {g:?}",
                if image.camera_linear_proxy().is_some() {
                    "Smart Preview"
                } else {
                    "original"
                }
            );
            if gpu_is_faster(c, g) {
                return gpu;
            }
        }
        (_, Err(e)) => eprintln!("develop: GPU calibration failed, using CPU: {e}"),
        (Err(e), _) => eprintln!("develop: CPU calibration failed, retaining CPU: {e}"),
    }
    if health
        .as_ref()
        .is_some_and(|d| d.device_failure().is_some())
    {
        clear_decisions();
    }
    cpu
}

fn device_identity(device: &gpu_core::GpuDevice) -> proxy_decision_cache::DeviceIdentity {
    let mut h = blake3::Hasher::new();
    h.update(b"tessera proxy adapter v1\0");
    let a = &device.adapter_info;
    for value in [&a.name, &a.driver, &a.driver_info] {
        h.update(&(value.len() as u64).to_le_bytes());
        h.update(value.as_bytes());
    }
    h.update(&a.vendor.to_le_bytes());
    h.update(&a.device.to_le_bytes());
    // wgpu's numeric backend/device-type discriminants are not persistent API;
    // use explicit tags within this versioned in-process key domain.
    let backend = match a.backend {
        wgpu::Backend::Noop => 0,
        wgpu::Backend::Vulkan => 1,
        wgpu::Backend::Metal => 2,
        wgpu::Backend::Dx12 => 3,
        wgpu::Backend::Gl => 4,
        wgpu::Backend::BrowserWebGpu => 5,
    };
    let kind = match a.device_type {
        wgpu::DeviceType::Other => 0,
        wgpu::DeviceType::IntegratedGpu => 1,
        wgpu::DeviceType::DiscreteGpu => 2,
        wgpu::DeviceType::VirtualGpu => 3,
        wgpu::DeviceType::Cpu => 4,
    };
    h.update(&[backend, kind]);
    let c = device.capabilities;
    proxy_decision_cache::DeviceIdentity {
        // Engine's OnceLock device cannot be replaced. A future reset API must
        // allocate a new generation (and clear Store) before this may change.
        generation: 1,
        adapter_fingerprint: *h.finalize().as_bytes(),
        capability_flags: u8::from(c.timestamp_query)
            | u8::from(c.shader_f16) << 1
            | u8::from(c.rgba16float_storage) << 2
            | u8::from(c.passthrough_shaders) << 3,
    }
}

#[cfg(test)]
#[path = "../../image-core/tests/common/mod.rs"]
pub(crate) mod common;

#[cfg(test)]
mod tests {
    #[test]
    #[cfg(target_os = "macos")]
    fn adobe_gpu_backend_uses_host_barriers_at_preview_level() {
        use super::*;
        use engine_api::{
            recipe::{ProcessVersion, Recipe},
            stage::StageId,
        };
        let image = common::synthetic(418, 48, 40, common::RGGB, [0, 0, 48, 40]);
        let ops = Arc::new(pipeline_gpu::GpuStageOp::new(Arc::new(
            pipeline_gpu::GpuContext::new().unwrap(),
        )));
        let base = Renderer::with_ops(
            ops.clone(),
            Arc::new(TileCache::new(16 << 20)),
            RendererConfig::default(),
        );
        let mut recipe = Recipe::new(image.id());
        recipe.process_version = ProcessVersion::adobe(6);
        let compat = base.for_recipe(&recipe);
        assert!(
            !compat
                .can_render_resident(&image, &recipe.settings)
                .unwrap()
        );
        let rect = PixelRect::full(image.level_extent(2));
        let got = compat
            .render_region(&image, &recipe.settings, 2, rect)
            .unwrap();
        assert!(got.iter().all(|t| t.coord().level == 2));
        assert!(compat.adobe_invocations(StageId::Tone) > 0);
        // Conventional tile batches count readbacks; byte counters belong to
        // the resident surface path, which is deliberately not selected here.
        assert!(ops.stats().readbacks > 0);
        let cpu = Renderer::new(RendererConfig::default()).for_recipe(&recipe);
        let expected = cpu
            .render_region(&image, &recipe.settings, 2, rect)
            .unwrap();
        let a = common::assemble_u8(image.level_extent(2), &got);
        let b = common::assemble_u8(image.level_extent(2), &expected);
        assert!(common::max_u8_diff(&a, &b) <= 2);
    }

    #[test]
    #[cfg(target_os = "macos")]
    fn calibration_measures_surface_presentation_without_pixel_readback() {
        use super::*;
        let image = common::synthetic(401, 269, 261, common::RGGB, [3, 5, 263, 251]);
        let ops = Arc::new(pipeline_gpu::GpuStageOp::new(Arc::new(
            pipeline_gpu::GpuContext::new().unwrap(),
        )));
        let renderer = Renderer::with_ops(
            ops.clone(),
            Arc::new(TileCache::new(16 << 20)),
            RendererConfig::default(),
        );
        let times = measure(&renderer, &image).unwrap();
        assert!(times.iter().all(|t| t.is_finite() && *t > 0.));
        assert_eq!(ops.stats().pixel_readback_bytes, 0);
        assert_eq!(ops.stats().histogram_readbacks, 3);
        assert_eq!(ops.stats().submissions, 3);
        let cpu = Renderer::new(RendererConfig::default());
        let times = measure(&cpu, &image).unwrap();
        assert!(times.iter().all(|t| t.is_finite() && *t > 0.));
    }

    #[test]
    fn selection_requires_measured_gpu_advantage() {
        // Interactive edits recur for the entire session. Do not reject the
        // faster interactive backend because its one-time cold fill costs more.
        assert!(super::gpu_is_faster([230., 9., 74.], [335., 6., 11.]));
        assert!(super::gpu_is_faster([10., 12., 40.], [9., 4., 20.]));
        assert!(!super::gpu_is_faster([10., 12., 40.], [9., 4., 80.]));
        assert!(!super::gpu_is_faster([10., 12., 40.], [10., 12., 40.]));
        assert!(!super::gpu_is_faster([10., 12., 40.], [f64::NAN, 4., 20.]));
    }

    /// Stage C (rev7 6.3) FFI synthetic contracts C1, C2, C2'. No GPU.
    #[cfg(feature = "wb-diagnostic")]
    mod wb_diag {
        use super::super::*;
        use engine_api::recipe::Recipe;
        use image_core::wb_diagnostic::{
            ARENA, RouteState, SlotMeta, harness::EpochGuard, recipe_fingerprint,
            settings_fingerprint,
        };

        const GUARD: &str = "EpochGuard: requires --test-threads=1 and an Idle ARENA \
                             (an earlier test may have leaked a live lease)";

        fn drain_metas(g: &EpochGuard) -> Vec<SlotMeta> {
            let mut metas = Vec::new();
            while let Ok(d) = g.epoch().drain() {
                metas.push(d.inspect(|v| *v.meta));
            }
            metas
        }
        fn image() -> RawImage {
            common::synthetic(402, 64, 48, common::RGGB, [0, 0, 64, 48])
        }

        /// C1: fresh backends carry distinct nonzero tags; clones, renderers
        /// and recipe snapshots keep them; `for_backend` resets to 0.
        #[test]
        fn c1_backend_operator_tags_are_distinct_and_propagate() {
            let _g = EpochGuard::open().expect(GUARD);
            let a = Backend::new(Arc::new(CpuStageOp), RendererConfig::default(), "a".into());
            let b = Backend::new(Arc::new(CpuStageOp), RendererConfig::default(), "b".into());
            assert_ne!(a.diag_operator, 0, "WB-RED: operator tag is 0");
            assert_ne!(b.diag_operator, 0);
            assert_ne!(a.diag_operator, b.diag_operator);
            assert_eq!(a.clone().diag_operator, a.diag_operator);
            let r = a.renderer();
            assert_eq!(r.diagnostic_operator(), a.diag_operator);
            assert_eq!(r.clone().diagnostic_operator(), a.diag_operator);
            let recipe = Recipe::new(image().id());
            assert_eq!(r.for_recipe(&recipe).diagnostic_operator(), a.diag_operator);
            assert_eq!(r.for_backend(Arc::new(CpuStageOp)).diagnostic_operator(), 0);
        }

        /// C2: the unobserved (CPU) arm makes no reservation and counts its
        /// three iterations while armed.
        #[test]
        fn c2_unobserved_arm_counts_iterations_without_reservation() {
            let g = EpochGuard::open().expect(GUARD);
            g.epoch().arm(1).unwrap();
            let cpu = Backend::cpu_for_test();
            let before = ARENA.counts().unwrap();
            measure_at(
                &cpu.renderer(),
                &image(),
                &DevelopSettings::default(),
                0,
                false,
            )
            .unwrap();
            let after = ARENA.counts().unwrap();
            assert_eq!(
                after.cpu_iterations_unobserved - before.cpu_iterations_unobserved,
                3,
                "WB-RED: cpu counter unchanged"
            );
            assert_eq!((after.reserved, after.active, after.completed), (0, 0, 0));
            assert_eq!(after.loss, 0);
            assert!(drain_metas(&g).is_empty());
        }

        /// C2': the observed arm on a CPU renderer reserves one transaction
        /// per iteration; CPU has no resident route, so each is Unobserved.
        /// Fingerprints follow the exposure +0.25 and WB toggle mutations.
        #[test]
        fn c2p_observed_arm_on_cpu_reserves_three_unobserved() {
            let g = EpochGuard::open().expect(GUARD);
            g.epoch().arm(1).unwrap();
            let cpu = Backend::cpu_for_test();
            let image = image();
            let base = DevelopSettings::default();
            measure_at(&cpu.renderer(), &image, &base, 0, true).unwrap();
            let mut metas = drain_metas(&g);
            assert!(!metas.is_empty(), "WB-RED: no reservation");
            assert_eq!(metas.len(), 3);
            metas.sort_by_key(|m| m.request.generation);
            let mut expected = Vec::new();
            let mut s = base.clone();
            expected.push(s.clone());
            s.tone.exposure += 0.25;
            expected.push(s.clone());
            s.white_balance.mode = WhiteBalanceMode::Daylight;
            expected.push(s);
            for (i, m) in metas.iter().enumerate() {
                assert_eq!(m.route, RouteState::Unobserved);
                assert!(m.begin.is_none());
                assert_eq!((m.request.phase_kind, m.request.generation), (1, i as u64));
                assert_eq!(
                    m.request.settings_fingerprint,
                    settings_fingerprint(&expected[i])
                );
                assert_eq!(
                    m.request.recipe_fingerprint,
                    recipe_fingerprint(image.id(), &expected[i])
                );
                assert_eq!(m.request.expected_operator, cpu.diag_operator);
                assert_eq!((m.request.output_tag, m.request.requested_level), (1, 0));
            }
            assert_ne!(
                metas[0].request.settings_fingerprint,
                metas[1].request.settings_fingerprint
            );
            assert_ne!(
                metas[1].request.settings_fingerprint,
                metas[2].request.settings_fingerprint
            );
            let c = ARENA.counts().unwrap();
            assert_eq!((c.cpu_iterations_unobserved, c.loss), (0, 0));
        }
    }
}
