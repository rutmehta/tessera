//! M3-16b: real device handoff tests; inference is deterministic in these tests.
#[path = "../../image-core/tests/common/mod.rs"]
mod common;
use engine_api::{
    jobs::CancellationToken,
    tile::{Extent, Tile, TileCoord, TileLayout},
};
use image_core::{
    StageOp,
    cfa::{CfaDenoise, PackedCfa},
};
use pipeline_gpu::{GpuContext, GpuStageOp};
use std::sync::Arc;

// Independent whole-padded-image rotation oracle, including same-phase padding.
fn packed(w: u32, h: u32, turns: u8, sensor: &[f32]) -> Vec<f32> {
    let (pw, ph) = (w.div_ceil(2) * 2, h.div_ceil(2) * 2);
    let (rw, rh) = if turns.is_multiple_of(2) {
        (pw, ph)
    } else {
        (ph, pw)
    };
    let mut out = Vec::new();
    for c in 0..4 {
        for y in 0..rh / 2 {
            for x in 0..rw / 2 {
                let (x, y) = (x * 2 + c % 2, y * 2 + c / 2);
                let (sx, sy) = match turns {
                    0 => (x, y),
                    1 => (pw - 1 - y, x),
                    2 => (pw - 1 - x, ph - 1 - y),
                    _ => (y, ph - 1 - x),
                };
                let sx = if sx >= w { sx - 2 } else { sx };
                let sy = if sy >= h { sy - 2 } else { sy };
                out.push(sensor[(sy * w + sx) as usize]);
            }
        }
    }
    out
}
fn fold(v: i64, n: u32) -> u32 {
    if v < 0 {
        v.rem_euclid(2) as u32
    } else if v >= i64::from(n) {
        let p = v as u32 % 2;
        p + (n - 1 - p) / 2 * 2
    } else {
        v as u32
    }
}

#[test]
fn gpu_unpack_rotations_halos_masks_and_amount_endpoints() {
    let gpu = GpuStageOp::new(Arc::new(GpuContext::new().unwrap()));
    let (w, h) = (263, 259);
    let sensor: Vec<f32> = (0..w * h).map(|i| (i as f32 * 0.0031).sin()).collect();
    let mask: Vec<f32> = (0..w * h)
        .map(|i| [0.0, 0.25, 1.0][i as usize % 3])
        .collect();
    for turns in 0..4 {
        let full = PackedCfa::new(
            Extent::new(w, h),
            turns,
            packed(w, h, turns, &sensor),
            Some(packed(w, h, turns, &mask)),
        )
        .unwrap();
        for (origin, extent, halo) in [
            ((0, 0), Extent::new(17, 13), 4),
            ((256, 256), Extent::new(7, 3), 4),
            ((7, 253), Extent::new(256, 6), 2),
        ] {
            let layout = TileLayout {
                extent,
                halo,
                channels: 1,
            };
            let input = Tile::from_samples(
                TileCoord::new(0, 0, 0),
                layout,
                vec![-0.125_f32; layout.len()],
            )
            .unwrap();
            let before = gpu.stats().uploads;
            let mut batch = gpu.begin_resident().unwrap();
            let raw = batch.upload(&input).unwrap();
            let restored = batch.upload_cfa(&full, raw.coord, layout, origin).unwrap();
            let mut outputs = Vec::new();
            for amount in [0.0, 0.5, 1.0] {
                outputs.push(batch.blend_cfa(&raw, &restored, amount).unwrap());
            }
            let result = batch
                .finish(outputs, false, None, &CancellationToken::new())
                .unwrap();
            assert_eq!(
                gpu.stats().uploads - before,
                2,
                "one packed payload upload plus the original input"
            );
            for (tile, amount) in result.tiles.iter().zip([0.0, 0.5, 1.0]) {
                for (i, &actual) in tile.samples::<f32>().unwrap().iter().enumerate() {
                    let x = fold(
                        i as i64 % layout.stride() as i64 + origin.0 as i64 - halo as i64,
                        w,
                    );
                    let y = fold(
                        i as i64 / layout.stride() as i64 + origin.1 as i64 - halo as i64,
                        h,
                    );
                    let j = (y * w + x) as usize;
                    let a = amount * mask[j];
                    let expected = if a == 0.0 {
                        -0.125
                    } else if a == 1.0 {
                        sensor[j]
                    } else {
                        -0.125 * (1.0 - a) + sensor[j] * a
                    };
                    if a == 0.0 || a == 1.0 {
                        assert_eq!(actual.to_bits(), expected.to_bits());
                    } else {
                        assert!(
                            (actual - expected).abs() < 2e-7,
                            "{turns} {i}: {actual} != {expected}"
                        );
                    }
                }
            }
        }
    }
}

struct Inference(std::sync::atomic::AtomicUsize);
impl pipeline_cpu::PostDemosaicDenoise for Inference {
    fn adapter_revision(&self) -> &str {
        "test-cfa-v1/noise-1"
    }
    fn denoise(
        &self,
        input: &pipeline_cpu::Image,
        _: f32,
    ) -> engine_api::EngineResult<pipeline_cpu::Image> {
        Ok(input.clone())
    }
}
impl CfaDenoise for Inference {
    fn supports(
        &self,
        cfa: raw_decode::CfaLayout,
        s: &engine_api::recipe::settings::DenoiseSettings,
    ) -> bool {
        matches!(cfa, raw_decode::CfaLayout::Bayer(_)) && pipeline_cpu::cfa_denoise_selected(s)
    }
    fn infer(
        &self,
        input: &pipeline_cpu::Image,
        cfa: raw_decode::CfaLayout,
        _: &engine_api::recipe::settings::DenoiseSettings,
    ) -> engine_api::EngineResult<PackedCfa> {
        self.0.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let turns = image_core::cfa::bayer_turns(cfa).unwrap();
        let samples: Vec<f32> = input.planes()[0].iter().map(|v| v * 0.9 + 0.013).collect();
        PackedCfa::new(
            Extent::new(input.width(), input.height()),
            turns,
            packed(input.width(), input.height(), turns, &samples),
            None,
        )
    }
}
fn settings() -> engine_api::recipe::DevelopSettings {
    let mut s = engine_api::recipe::DevelopSettings::default();
    s.lens.profile = engine_api::recipe::settings::LensProfileSource::None;
    s.lens.remove_chromatic_aberration = false;
    s.denoise.method = engine_api::recipe::settings::DenoiseMethod::Neural {
        model: engine_api::id::ModelRef {
            id: "enhance/cfa-unet-fp32".into(),
            version: "a".repeat(64),
        },
        joint_demosaic: false,
    };
    s
}
#[test]
fn resident_chain_reuses_inference_and_matches_cpu_and_bands() {
    use image_core::{PixelRect, Renderer, RendererConfig, TileCache};
    use std::sync::atomic::Ordering;
    let gpu = Arc::new(GpuStageOp::new(Arc::new(GpuContext::new().unwrap())));
    let inference = Arc::new(Inference(Default::default()));
    let renderer = Renderer::with_ops(
        gpu.clone(),
        Arc::new(TileCache::new(64 << 20)),
        RendererConfig::default(),
    )
    .with_cfa_denoise(inference.clone());
    let cpu = Renderer::new(RendererConfig::default())
        .with_cfa_denoise(Arc::new(Inference(Default::default())));
    let image = common::synthetic(933, 263, 259, common::RGGB, [0, 0, 263, 259]);
    let mut s = settings();
    let cancel = CancellationToken::new();
    let rect = PixelRect::full(image.level_extent(0));
    assert!(renderer.can_render_resident(&image, &s).unwrap());
    for amount in [100.0, 25.0, 0.0, 75.0] {
        s.denoise.amount = amount;
        let actual = renderer
            .render_resident_region(&image, &s, 0, rect, &cancel)
            .unwrap()
            .expect("CFA resident");
        let expected = cpu.render_region(&image, &s, 0, rect).unwrap();
        let a = common::assemble_u8(image.level_extent(0), &actual);
        let b = common::assemble_u8(image.level_extent(0), &expected);
        assert!(a.iter().zip(&b).all(|(a, b)| a.abs_diff(*b) <= 2));
        assert_eq!(inference.0.load(Ordering::SeqCst), 1);
        // Viewport backends intentionally reject row-band export. The managed
        // export test below exercises CFA on full-width bands instead.
        let mut band = vec![0.0; 263 * 259 * 3];
        assert!(
            !renderer
                .render_export_rows(&image, &s, 0, 0..259, None, &mut band, &cancel)
                .unwrap()
        );
    }
    s.tone.exposure = 0.3;
    let uploads = gpu.stats().uploads;
    renderer.render_region(&image, &s, 0, rect).unwrap();
    assert_eq!(gpu.stats().uploads, uploads, "tone edits stay resident");
    assert_eq!(inference.0.load(Ordering::SeqCst), 1);
    s.linearize.highlight_reconstruction =
        engine_api::recipe::settings::HighlightReconstruction::Clip;
    renderer.render_region(&image, &s, 0, rect).unwrap();
    assert_eq!(inference.0.load(Ordering::SeqCst), 2);
    let other = common::synthetic(934, 263, 259, common::RGGB, [0, 0, 263, 259]);
    renderer.render_region(&other, &s, 0, rect).unwrap();
    assert_eq!(inference.0.load(Ordering::SeqCst), 3);
    let xtrans = common::synthetic(935, 30, 30, common::xtrans(), [0, 0, 30, 30]);
    assert!(!renderer.can_render_resident(&xtrans, &s).unwrap());
    cancel.cancel();
    assert!(
        renderer
            .render_resident_region(&image, &s, 0, rect, &cancel)
            .is_err()
    );
}

/// Real fixtures and real pinned weights only. Calibration file lines:
/// <fixture filename> <shot R G1 G2 B> <read R G1 G2 B> (whitespace separated).
#[test]
#[ignore = "requires Metal, RAW fixtures, pinned CFA weights and measured per-fixture calibration"]
fn benchmark_real_cfa_first_frame_then_tone_only() {
    use image_core::{PixelRect, RawImage, Renderer, RendererConfig, TileCache};
    use std::{path::PathBuf, time::Instant};
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let fixtures = std::env::var_os("PIPELINE_RAW_FIXTURES")
        .map(PathBuf::from)
        .unwrap_or_else(|| root.join("fixtures/raw"));
    let Ok(entries) = std::fs::read_dir(&fixtures) else {
        eprintln!("NO TIMINGS: RAW fixtures absent: {}", fixtures.display());
        return;
    };
    let mut paths: Vec<_> = entries
        .map(|e| e.unwrap().path())
        .filter(|p| {
            p.extension().is_some_and(|e| {
                matches!(
                    e.to_string_lossy().to_ascii_lowercase().as_str(),
                    "cr3" | "arw" | "nef" | "raf" | "dng"
                )
            })
        })
        .collect();
    paths.sort();
    if paths.is_empty() {
        eprintln!("NO TIMINGS: no RAW fixtures in {}", fixtures.display());
        return;
    }
    let Some(calibration) = std::env::var_os("TESSERA_CFA_BENCH_CALIBRATION") else {
        eprintln!(
            "NO TIMINGS: set TESSERA_CFA_BENCH_CALIBRATION to measured per-fixture noise coefficients"
        );
        return;
    };
    let calibration = std::fs::read_to_string(calibration).unwrap();
    let manifest = std::env::var_os("TESSERA_CFA_MANIFEST")
        .map(PathBuf::from)
        .unwrap_or_else(|| root.join("crates/ml-runtime/models.toml"));
    let cache = std::env::temp_dir().join("tessera-m3-16b-bench-models");
    let registry = Arc::new(ml_runtime::ModelRegistry::open(manifest, cache).unwrap());
    let model_id =
        std::env::var("TESSERA_CFA_MODEL_ID").unwrap_or_else(|_| "enhance/cfa-unet-fp32".into());
    let spec = registry
        .models()
        .iter()
        .find(|m| m.id == model_id)
        .expect("CFA model in manifest");
    let model = engine_api::id::ModelRef {
        id: spec.id.as_str().into(),
        version: spec.sha256.clone(),
    };
    let context = Arc::new(GpuContext::new().expect("real Metal device"));
    for (i, path) in paths.iter().enumerate() {
        let name = path.file_name().unwrap().to_string_lossy();
        let Some(line) = calibration
            .lines()
            .find(|l| l.split_whitespace().next() == Some(name.as_ref()))
        else {
            eprintln!("NO TIMINGS {name}: measured calibration absent");
            continue;
        };
        let coefficients: Vec<f32> = line
            .split_whitespace()
            .skip(1)
            .map(|v| v.parse().unwrap())
            .collect();
        assert_eq!(
            coefficients.len(),
            8,
            "four shot plus four read coefficients"
        );
        let image = RawImage::open(engine_api::id::ImageId(8000 + i as u128), path).unwrap();
        if !matches!(image.metadata().cfa_layout, raw_decode::CfaLayout::Bayer(_)) {
            eprintln!("NO CFA TIMINGS {name}: X-Trans uses RGB fallback");
            continue;
        }
        let adapter = Arc::new(image_core::MlCfaDenoise::new(
            registry.clone(),
            Default::default(),
            model.clone(),
            ml_enhance::CfaNoise {
                shot: coefficients[..4].try_into().unwrap(),
                read: coefficients[4..].try_into().unwrap(),
            },
        ));
        let gpu = Arc::new(GpuStageOp::new(context.clone()));
        let renderer = Renderer::with_ops(
            gpu.clone(),
            Arc::new(TileCache::new(512 << 20)),
            RendererConfig::default(),
        )
        .with_cfa_denoise(adapter);
        let mut s = settings();
        s.denoise.method = engine_api::recipe::settings::DenoiseMethod::Neural {
            model: model.clone(),
            joint_demosaic: false,
        };
        let level = 2;
        let rect = PixelRect::full(image.level_extent(level));
        let cancel = CancellationToken::new();
        let start = Instant::now();
        let first = renderer
            .render_resident_region(&image, &s, level, rect, &cancel)
            .unwrap()
            .expect("resident CFA");
        println!(
            "{name}: first render L{level} including host inference/upload and final pixel readback: {:.3} ms ({} tiles)",
            start.elapsed().as_secs_f64() * 1000.0,
            first.len()
        );
        for exposure in [0.1, 0.2, -0.1, 0.3, 0.0] {
            s.tone.exposure = exposure;
            let uploads = gpu.stats().uploads;
            let start = Instant::now();
            renderer
                .render_resident_region(&image, &s, level, rect, &cancel)
                .unwrap()
                .expect("resident tone");
            println!(
                "{name}: tone-only exposure={exposure}: {:.3} ms, uploads={}",
                start.elapsed().as_secs_f64() * 1000.0,
                gpu.stats().uploads - uploads
            );
        }
    }
}

#[path = "../../image-core/tests/common/cfa.rs"]
mod masked_inference;

#[test]
fn gpu_cfa_masked_chain_matches_cpu_for_odd_rotations_crops_and_previews() {
    use image_core::{PixelRect, Renderer, RendererConfig, TileCache};
    let ctx = Arc::new(GpuContext::new().unwrap());
    for (turn, pattern) in [
        [[0, 1], [3, 2]],
        [[1, 0], [2, 3]],
        [[2, 3], [1, 0]],
        [[3, 2], [0, 1]],
    ]
    .into_iter()
    .enumerate()
    {
        let image = common::synthetic(
            10000 + turn as u128,
            269,
            263,
            raw_decode::CfaLayout::Bayer(pattern),
            [3, 5, 263, 257],
        );
        let gpu = Arc::new(GpuStageOp::new(ctx.clone()));
        let renderer = Renderer::with_ops(
            gpu.clone(),
            Arc::new(TileCache::new(64 << 20)),
            RendererConfig::default(),
        )
        .with_cfa_denoise(Arc::new(masked_inference::Inference::default()));
        let cpu = Renderer::new(RendererConfig::default())
            .with_cfa_denoise(Arc::new(masked_inference::Inference::default()));
        let mut s = masked_inference::settings();
        for level in [0, 2] {
            for amount in [100.0, 37.0] {
                s.denoise.amount = amount;
                let e = image.level_extent(level);
                let rect = PixelRect::full(e);
                let a = renderer
                    .render_resident_region(&image, &s, level, rect, &CancellationToken::new())
                    .unwrap()
                    .expect("supported CFA");
                let b = cpu.render_region(&image, &s, level, rect).unwrap();
                let a = common::assemble_u8(e, &a);
                let b = common::assemble_u8(e, &b);
                let max = a.iter().zip(&b).map(|(a, b)| a.abs_diff(*b)).max().unwrap();
                assert!(
                    max <= 2,
                    "rotation {turn}, level {level}, amount {amount}: max error {max}"
                );
            }
        }
    }
}

#[test]
fn packed_queue_writes_do_not_overwrite_earlier_unsubmitted_compute() {
    let gpu = GpuStageOp::new(Arc::new(GpuContext::new().unwrap()));
    let mut batch = gpu.begin_resident().unwrap();
    let layout = TileLayout {
        extent: Extent::new(8, 6),
        halo: 2,
        channels: 1,
    };
    let mut outputs = Vec::new();
    for value in [0.125_f32, 0.75, 0.375] {
        let full = PackedCfa::new(Extent::new(8, 6), 1, vec![value; 48], None).unwrap();
        let original = batch
            .upload(
                &Tile::from_samples(TileCoord::new(0, 0, 0), layout, vec![0.0; layout.len()])
                    .unwrap(),
            )
            .unwrap();
        let restored = batch
            .upload_cfa(&full, original.coord, layout, (0, 0))
            .unwrap();
        outputs.push(batch.blend_cfa(&original, &restored, 1.0).unwrap());
        // Drop both handles before the next host upload. Earlier compute must
        // still observe its own immutable host payload at submission time.
    }
    let output = batch
        .finish(outputs, false, None, &CancellationToken::new())
        .unwrap();
    for (tile, value) in output.tiles.iter().zip([0.125_f32, 0.75, 0.375]) {
        assert!(
            tile.samples::<f32>()
                .unwrap()
                .iter()
                .all(|v| v.to_bits() == value.to_bits())
        );
    }
}

#[test]
fn managed_export_bands_retain_the_injected_cfa_backend() {
    use color_mgmt::{Builtin, Registry, TransformOptions};
    use pipeline_cpu::{OutputContext, OutputTarget};
    use pipeline_gpu::{GpuManagedOutput, ManagedRenderer};
    let mut registry = Registry::new();
    let target = registry.builtin(Builtin::Srgb).unwrap();
    let s = masked_inference::settings();
    let output = Arc::new(
        GpuManagedOutput::new(
            Arc::new(GpuContext::new().unwrap()),
            &s,
            &mut OutputContext {
                registry: &mut registry,
                target: OutputTarget::Export(&target),
                proof: None,
                options: TransformOptions::default(),
            },
        )
        .unwrap(),
    );
    let infer = Arc::new(masked_inference::Inference::default());
    let renderer = ManagedRenderer::new_export(output, image_core::RendererConfig::default())
        .with_cfa_denoise(infer.clone());
    let image = common::synthetic(11112, 263, 259, common::RGGB, [0, 0, 263, 259]);
    let cancel = CancellationToken::new();
    let whole = renderer
        .render_export(
            &image,
            &s,
            0,
            image_core::PixelRect::full(image.level_extent(0)),
            &cancel,
        )
        .unwrap()
        .expect("managed resident CFA");
    let expected = common::assemble_f32(image.level_extent(0), &whole);
    let band = renderer.export_band(None);
    let mut rows = vec![0.0; 263 * 259 * 3];
    assert!(
        band.render_export_rows(&image, &s, 0, 0..259, None, &mut rows, &cancel)
            .unwrap()
    );
    for (i, pixel) in rows.as_chunks::<3>().0.iter().enumerate() {
        for c in 0..3 {
            assert!((pixel[c] - expected[c][i]).abs() < 2e-4);
        }
    }
    assert_eq!(infer.calls.load(std::sync::atomic::Ordering::SeqCst), 1);
}
