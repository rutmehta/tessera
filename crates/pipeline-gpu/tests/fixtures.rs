use engine_api::{
    EngineResult, id::ImageId, jobs::CancellationToken, recipe::DevelopSettings, stage::StageId,
    tile::Tile,
};
use image_core::{
    CpuStageOp, Op, PixelRect, RawImage, RenderOutput, Renderer, RendererConfig, StageOp, TileCache,
};
use pipeline_gpu::{GpuContext, GpuStageOp};
#[path = "support/color_difference.rs"]
mod color_difference;
use std::{
    path::PathBuf,
    sync::{Arc, Mutex},
    time::Instant,
};

fn fixtures(all: bool) -> Vec<PathBuf> {
    let root = std::env::var_os("PIPELINE_RAW_FIXTURES")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/raw"));
    let mut selected = Vec::new();
    for ext in if all {
        &["arw", "cr3", "nef", "raf", "dng"][..]
    } else {
        &["arw"][..]
    } {
        let mut files: Vec<_> = std::fs::read_dir(&root)
            .unwrap_or_else(|e| {
                panic!(
                    "fixtures required at {} (or set PIPELINE_RAW_FIXTURES): {e}",
                    root.display()
                )
            })
            .map(|e| e.unwrap().path())
            .filter(|p| p.is_file() && p.extension().is_some_and(|e| e.eq_ignore_ascii_case(ext)))
            .collect();
        files.sort();
        assert!(
            !files.is_empty(),
            "missing {ext} fixture at {}",
            root.display()
        );
        selected.push(files.remove(0));
    }
    selected
}

fn difference(a: &Tile, b: &Tile) -> f32 {
    assert_eq!(a.coord(), b.coord());
    assert_eq!(a.layout(), b.layout());
    if let Ok(a) = a.samples::<f32>() {
        a.iter()
            .zip(b.samples::<f32>().unwrap())
            .map(|(a, b)| {
                assert!(a.is_finite() && b.is_finite());
                (a - b).abs()
            })
            .fold(0.0, f32::max)
    } else {
        a.samples::<u8>()
            .unwrap()
            .iter()
            .zip(b.samples::<u8>().unwrap())
            .map(|(a, b)| a.abs_diff(*b) as f32)
            .fold(0.0, f32::max)
    }
}

/// Audit every stage on the first real sensor/output tile of each renderer
/// batch, as well as comparing every final pixel of the level-3 image.
struct Audited {
    gpu: GpuStageOp,
    errors: Mutex<[Option<f32>; StageId::COUNT]>,
}
impl StageOp for Audited {
    fn run_image(
        &self,
        stage: StageId,
        op: &Op<'_>,
        input: pipeline_cpu::Image,
        cancel: &CancellationToken,
    ) -> EngineResult<pipeline_cpu::Image> {
        let expected = CpuStageOp.run_image(stage, op, input.clone(), cancel)?;
        let actual = self.gpu.run_image(stage, op, input, cancel)?;
        assert_eq!(
            (actual.width(), actual.height()),
            (expected.width(), expected.height())
        );
        let diff = actual
            .planes()
            .iter()
            .flatten()
            .zip(expected.planes().iter().flatten())
            .map(|(a, b)| {
                assert!(a.is_finite() && b.is_finite());
                (a - b).abs()
            })
            .fold(0.0f32, f32::max);
        assert!(diff <= 1e-4, "image {stage:?}: {diff:e}");
        let mut errors = self.errors.lock().unwrap();
        errors[stage.index()] = Some(errors[stage.index()].unwrap_or(0.0).max(diff));
        Ok(actual)
    }
    fn run(&self, stage: StageId, op: &Op<'_>, input: Tile) -> EngineResult<Tile> {
        let expected = CpuStageOp.run(stage, op, input.clone())?;
        let actual = self.gpu.run(stage, op, input)?;
        let diff = difference(&actual, &expected);
        assert!(
            diff <= if matches!(op, Op::Display { .. }) {
                1.0
            } else {
                1e-4
            },
            "{stage:?}: {diff:e}"
        );
        let mut errors = self.errors.lock().unwrap();
        errors[stage.index()] = Some(errors[stage.index()].unwrap_or(0.0).max(diff));
        Ok(actual)
    }
    fn batch_size(&self) -> usize {
        self.gpu.batch_size()
    }
    fn run_chain_batch(
        &self,
        chain: &[(StageId, Op<'_>)],
        inputs: Vec<Tile>,
        cancel: &CancellationToken,
    ) -> EngineResult<Vec<Tile>> {
        if let Some(input) = inputs.first() {
            let mut tile = input.clone();
            for (stage, op) in chain {
                tile = self.run(*stage, op, tile)?;
            }
        }
        self.gpu.run_chain_batch(chain, inputs, cancel)
    }
}

#[test]
fn fixture_level3_tolerance_per_operator_and_output() {
    let context = Arc::new(GpuContext::new().unwrap());
    eprintln!(
        "adapter: {:?}; capabilities: {:?}",
        context.adapter_info, context.capabilities
    );
    let all = std::env::var("PIPELINE_GPU_ALL_FIXTURES").as_deref() == Ok("1");
    let mut failures = Vec::new();
    for path in fixtures(all) {
        let image = RawImage::open(ImageId(42), &path).unwrap();
        let mut s = DevelopSettings::default();
        s.tone.exposure = 0.35;
        s.tone.contrast = 12.0;
        s.tone.shadows = 18.0;
        s.tone.curves.parametric.darks = 25.0;
        s.color.vibrance = 35.0;
        s.color.hsl.hue.blue = -20.0;
        s.color.grading.shadows.saturation = 15.0;
        s.tone.texture = 20.0;
        s.tone.clarity = 15.0;
        s.tone.dehaze = 10.0;
        s.detail.sharpening.amount = 60.0;
        s.detail.noise_reduction.luminance = 15.0;
        s.detail.noise_reduction.color = 30.0;
        s.effects.vignette.amount = -20.0;
        s.effects.grain.amount = 15.0;
        s.geometry.crop.angle = 1.0;
        let ops = Arc::new(Audited {
            gpu: GpuStageOp::new(context.clone()),
            errors: Mutex::new([None; StageId::COUNT]),
        });
        let config = RendererConfig::default();
        let r = Renderer::with_ops(
            ops.clone(),
            Arc::new(TileCache::new(config.cache_budget_bytes)),
            config.clone(),
        );
        let cpu = Renderer::new(config);
        let rect = PixelRect::full(image.level_extent(3));
        for output in [RenderOutput::SceneLinear, RenderOutput::Display] {
            r.cache().clear();
            cpu.cache().clear();
            let actual = r.render_region_as(&image, &s, 3, rect, output).unwrap();
            let expected = cpu.render_region_as(&image, &s, 3, rect, output).unwrap();
            assert_eq!(actual.len(), expected.len());
            let diff = actual
                .iter()
                .zip(&expected)
                .map(|(a, b)| difference(a, b))
                .fold(0.0, f32::max);
            let tolerance = if output == RenderOutput::Display {
                1.0
            } else {
                1e-4
            };
            if diff > tolerance {
                failures.push(format!("{} {output:?}: {diff:e}", path.display()));
            }
            eprintln!("{} L3 {output:?}: max error {diff:e}", path.display());
            if output == RenderOutput::SceneLinear {
                let mut max_delta = 0.0_f64;
                for (a, b) in actual.iter().zip(&expected) {
                    let n = a.layout().plane_len();
                    let av = a.samples::<f32>().unwrap();
                    let bv = b.samples::<f32>().unwrap();
                    for i in 0..n {
                        let delta = color_difference::linear_rec2020_delta_e(
                            [av[i], av[n + i], av[2 * n + i]],
                            [bv[i], bv[n + i], bv[2 * n + i]],
                        );
                        assert!(delta.is_finite());
                        max_delta = max_delta.max(delta);
                    }
                }
                if max_delta > 0.5 {
                    failures.push(format!("{} DeltaE2000 {max_delta}", path.display()));
                }
                eprintln!("{} L3 max DeltaE2000: {max_delta:e}", path.display());
            }
        }
        let errors = ops.errors.lock().unwrap();
        for stage in [
            StageId::Linearize,
            StageId::Demosaic,
            StageId::CameraProfile,
            StageId::WhiteBalance,
            StageId::Tone,
            StageId::Output,
        ] {
            eprintln!(
                "  {stage:?}: max error {:e}",
                errors[stage.index()].expect("stage must be audited")
            );
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// Full fixture suite, warm upstream caches, five distinct tone edits.
/// cargo test -p pipeline-gpu --release --test fixtures -- --ignored --nocapture
#[test]
#[ignore]
fn bench_full_level2_tone_only_gpu_vs_cpu() {
    let context = Arc::new(GpuContext::new().unwrap());
    for path in fixtures(true) {
        let image = RawImage::open(ImageId(7), &path).unwrap();
        let rect = PixelRect::full(image.level_extent(2));
        // Retain the entire L2 WB image, but don't spend this benchmark's
        // cache budget on full-resolution demosaic buffers.
        let mut config = RendererConfig::default();
        config.graph = config.graph.with_cacheable(StageId::Demosaic, false);
        let gpu = Arc::new(GpuStageOp::new(context.clone()));
        let r = Renderer::with_ops(
            gpu.clone(),
            Arc::new(TileCache::new(config.cache_budget_bytes)),
            config.clone(),
        );
        let cpu = Renderer::new(config);
        let mut s = DevelopSettings::default();
        r.render_region(&image, &s, 2, rect).unwrap();
        cpu.render_region(&image, &s, 2, rect).unwrap();
        let mut times = [Vec::new(), Vec::new()];
        for i in 1..=5 {
            s.tone.exposure = 0.1 * i as f32;
            s.tone.contrast = 3.0 * i as f32;
            let before = gpu.stats();
            for (j, renderer) in [&r, &cpu].into_iter().enumerate() {
                let t = Instant::now();
                let tiles = renderer.render_region(&image, &s, 2, rect).unwrap();
                times[j].push(t.elapsed().as_secs_f64() * 1e3);
                if j == 0 {
                    let after = gpu.stats();
                    assert_eq!(
                        after.uploads - before.uploads,
                        tiles.len() as u64,
                        "must hit upstream cache"
                    );
                    assert_eq!(after.readbacks - before.readbacks, tiles.len() as u64);
                }
            }
        }
        for t in &mut times {
            t.sort_by(f64::total_cmp);
        }
        eprintln!(
            "{} full L2 {:?}: GPU {:.2} ms, CPU {:.2} ms (median of 5, wall incl transfers)",
            path.display(),
            image.level_extent(2),
            times[0][2],
            times[1][2]
        );
    }
}

/// Cold full L2 chain, including currently CPU-delegated M2 operators.
#[test]
#[ignore]
fn bench_full_level2_m2_chain_gpu_vs_cpu() {
    let context = Arc::new(GpuContext::new().unwrap());
    for path in fixtures(true) {
        let image = RawImage::open(ImageId(7), &path).unwrap();
        let rect = PixelRect::full(image.level_extent(2));
        let config = RendererConfig::default();
        let gpu = Arc::new(GpuStageOp::new(context.clone()));
        let r = Renderer::with_ops(
            gpu,
            Arc::new(TileCache::new(config.cache_budget_bytes)),
            config.clone(),
        );
        let cpu = Renderer::new(config);
        let mut s = DevelopSettings::default();
        s.tone.exposure = 0.3;
        s.tone.curves.parametric.darks = 25.0;
        s.tone.texture = 20.0;
        s.tone.clarity = 15.0;
        s.tone.dehaze = 10.0;
        s.color.vibrance = 35.0;
        s.color.hsl.hue.blue = -20.0;
        s.detail.sharpening.amount = 60.0;
        s.detail.noise_reduction.luminance = 15.0;
        s.detail.noise_reduction.color = 30.0;
        s.effects.vignette.amount = -20.0;
        s.effects.grain.amount = 15.0;
        s.geometry.crop.angle = 1.0;
        let mut times = [Vec::new(), Vec::new()];
        for _ in 0..3 {
            for (j, renderer) in [&r, &cpu].into_iter().enumerate() {
                renderer.cache().clear();
                let t = Instant::now();
                let tiles = renderer.render_region(&image, &s, 2, rect).unwrap();
                assert!(!tiles.is_empty());
                times[j].push(t.elapsed().as_secs_f64() * 1e3);
            }
        }
        for t in &mut times {
            t.sort_by(f64::total_cmp);
        }
        eprintln!(
            "{} M2 full-chain L2 {:?}: GPU-backend (hybrid) {:.2} ms, CPU {:.2} ms (median of 3, cold caches)",
            path.display(),
            image.level_extent(2),
            times[0][1],
            times[1][1]
        );
    }
}
