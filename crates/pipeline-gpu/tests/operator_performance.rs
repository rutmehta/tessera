//! Real-NEF operator wall-time benchmark; never runs in the normal test gate.
//!
//! PIPELINE_BENCH_NEF=/path/image.NEF PIPELINE_BENCH_LABEL=baseline \
//! cargo test -p pipeline-gpu --release --test operator_performance -- \
//!   --ignored --nocapture --test-threads=1
//!
//! Defaults: full L2, full L0, and a centered 1024x1024 L0 region, three samples
//! after one warmup. PIPELINE_BENCH_SAMPLES overrides the sample count. Decode,
//! upstream reconstruction, input cloning and output validation are not timed.
//! GPU times include allocations, encoding, upload, synchronization and readback;
//! they are NOT resident slider/IOSurface latency or GPU timestamp queries.
//! CPU is the serial StageOp reference, not the parallel Renderer. Each operator
//! receives the same immutable, neutral-developed NEF pixels independently.
use engine_api::{
    id::ImageId,
    jobs::CancellationToken,
    recipe::{DevelopSettings, LocalParams, settings::*},
    stage::StageId,
    tile::Extent,
};
use image_core::{CpuStageOp, Op, RawImage, StageOp};
use pipeline_cpu::{Image, RenderSource};
use pipeline_gpu::{GpuContext, GpuStageOp};
use std::{hint::black_box, path::PathBuf, sync::Arc, time::Instant};

fn nef_fixture() -> PathBuf {
    if let Some(path) = std::env::var_os("PIPELINE_BENCH_NEF") {
        let path = PathBuf::from(path);
        assert!(path.is_file(), "NEF fixture missing: {}", path.display());
        assert!(
            path.extension()
                .is_some_and(|e| e.eq_ignore_ascii_case("nef"))
        );
        return path;
    }
    let root = std::env::var_os("PIPELINE_RAW_FIXTURES")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/raw"));
    let mut paths: Vec<_> = std::fs::read_dir(&root)
        .unwrap_or_else(|e| {
            panic!(
                "real NEF required at {} or PIPELINE_BENCH_NEF: {e}",
                root.display()
            )
        })
        .map(|e| e.unwrap().path())
        .filter(|p| p.is_file() && p.extension().is_some_and(|e| e.eq_ignore_ascii_case("nef")))
        .collect();
    paths.sort();
    paths
        .into_iter()
        .next()
        .expect("no real NEF fixture found; no synthetic fallback")
}

fn median(samples: &mut [f64]) -> f64 {
    samples.sort_by(f64::total_cmp);
    let n = samples.len();
    if n.is_multiple_of(2) {
        (samples[n / 2 - 1] + samples[n / 2]) / 2.0
    } else {
        samples[n / 2]
    }
}

fn measure<T>(samples: usize, mut run: impl FnMut() -> (T, f64)) -> (Vec<f64>, T) {
    let (warmup, _) = run();
    black_box(&warmup);
    drop(warmup);
    let mut times = Vec::with_capacity(samples);
    let mut last = None;
    for _ in 0..samples {
        // Drop the previous output before the next input is prepared/timed.
        drop(last.take());
        let (out, ms) = run();
        black_box(&out);
        times.push(ms);
        last = Some(out);
    }
    (times, last.unwrap())
}

fn report(scope: &str, name: &str, cpu: &mut [f64], gpu: &mut [f64], submissions: u64, error: f32) {
    let cpu_ms = median(cpu);
    let gpu_ms = median(gpu);
    eprintln!(
        "OP_BENCH scope={scope} op={name} cpu_ms={cpu_ms:.3} gpu_ms={gpu_ms:.3} gpu_submissions={submissions} max_abs_error={error:.8e} cpu_samples={cpu:?} gpu_samples={gpu:?}"
    );
}

fn compare(a: &Image, b: &Image) -> f32 {
    assert_eq!((a.width(), a.height()), (b.width(), b.height()));
    assert_eq!(a.planes().len(), b.planes().len());
    a.planes()
        .iter()
        .flatten()
        .zip(b.planes().iter().flatten())
        .map(|(a, b)| {
            assert!(a.is_finite() && b.is_finite());
            (a - b).abs()
        })
        .fold(0.0, f32::max)
}

fn bench_op(
    gpu: &GpuStageOp,
    input: &Image,
    scope: &str,
    name: &str,
    stage: StageId,
    op: Op<'_>,
    samples: usize,
) {
    let cancel = CancellationToken::new();
    let run = |backend: &dyn StageOp| {
        let owned = input.clone();
        let start = Instant::now();
        let out = backend.run_image(stage, &op, owned, &cancel).unwrap();
        (out, start.elapsed().as_secs_f64() * 1000.0)
    };
    let (mut cpu_times, expected) = measure(samples, || run(&CpuStageOp));
    let before = gpu.stats();
    let (mut gpu_times, actual) = measure(samples, || run(gpu));
    let error = compare(&actual, &expected);
    // Report rather than loosen/repurpose the correctness suites' tolerances.
    // Signed real-RAW presence filters can amplify upstream floating-point error.
    report(
        scope,
        name,
        &mut cpu_times,
        &mut gpu_times,
        gpu.stats().submissions - before.submissions,
        error,
    );
}

fn bench_image(gpu: &GpuStageOp, input: &Image, scope: &str, samples: usize) {
    eprintln!(
        "OP_BENCH_INPUT scope={scope} width={} height={} samples={samples} warmups=1",
        input.width(),
        input.height()
    );
    let mut s = DevelopSettings::default();
    s.tone.exposure = 0.35;
    s.tone.contrast = 12.0;
    s.tone.shadows = 18.0;
    bench_op(
        gpu,
        input,
        scope,
        "basic_tone",
        StageId::Tone,
        Op::Tone(&s.tone),
        samples,
    );
    for (name, texture, clarity, dehaze, darks) in [
        ("curves", 0.0, 0.0, 0.0, 25.0),
        ("texture", 20.0, 0.0, 0.0, 0.0),
        ("clarity", 0.0, 15.0, 0.0, 0.0),
        ("dehaze", 0.0, 0.0, 10.0, 0.0),
    ] {
        let mut tone = ToneSettings {
            texture,
            clarity,
            dehaze,
            ..Default::default()
        };
        tone.curves.parametric.darks = darks;
        bench_op(
            gpu,
            input,
            scope,
            name,
            StageId::Tone,
            Op::ToneExtra(&tone),
            samples,
        );
    }
    for (name, sharpening, luminance, color) in [
        ("sharpening", 60.0, 0.0, 0.0),
        ("luminance_nr", 0.0, 15.0, 0.0),
        ("chroma_nr", 0.0, 0.0, 30.0),
    ] {
        let mut detail = DetailSettings::default();
        detail.sharpening.amount = sharpening;
        detail.noise_reduction.luminance = luminance;
        detail.noise_reduction.color = color;
        bench_op(
            gpu,
            input,
            scope,
            name,
            StageId::Detail,
            Op::Detail(&detail),
            samples,
        );
    }
    for name in ["vibrance", "hsl", "color_grading"] {
        let mut color = ColorSettings::default();
        match name {
            "vibrance" => color.vibrance = 35.0,
            "hsl" => color.hsl.hue.blue = -20.0,
            _ => color.grading.shadows.saturation = 15.0,
        }
        bench_op(
            gpu,
            input,
            scope,
            name,
            StageId::Color,
            Op::Color(&color),
            samples,
        );
    }
    for name in ["vignette", "grain"] {
        let mut effects = EffectsSettings::default();
        if name == "vignette" {
            effects.vignette.amount = -20.0;
        } else {
            effects.grain.amount = 15.0;
        }
        bench_op(
            gpu,
            input,
            scope,
            name,
            StageId::Effects,
            Op::Effects(&effects, Extent::new(input.width(), input.height())),
            samples,
        );
    }
    let mut geometry = GeometrySettings::default();
    geometry.crop.angle = 1.0;
    bench_op(
        gpu,
        input,
        scope,
        "crop_straighten",
        StageId::Geometry,
        Op::Geometry(&geometry),
        samples,
    );

    // Locals only exposes blending on GPU. Adjustment/mask preparation is not
    // timed; calling this a full local-adjustment GPU benchmark would be false.
    let adjusted = pipeline_cpu::adjust_local(
        input,
        &LocalParams {
            exposure: 0.5,
            ..Default::default()
        },
        1.0,
    )
    .unwrap();
    let mask: Vec<_> = (0..input.width() as usize * input.height() as usize)
        .map(|i| (i % input.width() as usize) as f32 / input.width() as f32)
        .collect();
    let run = |backend: &dyn StageOp| {
        let start = Instant::now();
        let out = backend.blend_local(input, &adjusted, &mask).unwrap();
        (out, start.elapsed().as_secs_f64() * 1000.0)
    };
    let (mut cpu_times, expected) = measure(samples, || run(&CpuStageOp));
    let before = gpu.stats();
    let (mut gpu_times, actual) = measure(samples, || run(gpu));
    report(
        scope,
        "local_blend_only",
        &mut cpu_times,
        &mut gpu_times,
        gpu.stats().submissions - before.submissions,
        compare(&actual, &expected),
    );
}

#[test]
#[ignore = "requires a real NEF, Metal, and potentially several minutes at full L0"]
fn bench_nef_per_operator_l2_l0() {
    let path = nef_fixture();
    let samples = std::env::var("PIPELINE_BENCH_SAMPLES")
        .map(|s| s.parse::<usize>().unwrap())
        .unwrap_or(3);
    assert!(samples > 0, "at least one measured sample required");
    let label =
        std::env::var("PIPELINE_BENCH_LABEL").unwrap_or_else(|_| "unlabelled-working-tree".into());
    let context = Arc::new(GpuContext::new().expect("real Metal adapter required"));
    eprintln!(
        "OP_BENCH_RUN label={label:?} fixture={} adapter={:?} capabilities={:?}",
        path.display(),
        context.adapter_info,
        context.capabilities
    );
    eprintln!(
        "OP_BENCH_METHOD independent same-input operators; CPU serial reference; GPU backend may fall back (zero submissions); timed wall includes transfers and completion; excludes clone/decode/preparation; not resident frame latency"
    );
    let gpu = GpuStageOp::new(context);
    let raw = RawImage::open(ImageId(1717), &path).expect("decode real NEF");
    let mut neutral = DevelopSettings::default();
    neutral.detail.sharpening.amount = 0.0;
    neutral.detail.noise_reduction.luminance = 0.0;
    neutral.detail.noise_reduction.color = 0.0;
    // Reconstruct CFA at sensor resolution before downsampling RGB. Never
    // decimate Bayer samples to manufacture a lower-resolution RAW fixture.
    let source = RenderSource::Cfa {
        image: raw.cfa(),
        metadata: raw.metadata(),
    };
    let full = pipeline_cpu::render_linear_scaled(&neutral, &source, 1).unwrap();
    let preview = full
        .downsample_crop([0, 0, full.width(), full.height()], 4)
        .unwrap();
    bench_image(&gpu, &preview, "L2-full", samples);
    drop(preview);
    let w = full.width().min(1024);
    let h = full.height().min(1024);
    let region = full
        .downsample_crop([(full.width() - w) / 2, (full.height() - h) / 2, w, h], 1)
        .unwrap();
    bench_image(&gpu, &region, "L0-region-1024", samples);
    drop(region);
    bench_image(&gpu, &full, "L0-full", samples);
}
