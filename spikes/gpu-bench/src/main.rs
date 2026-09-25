//! M0-07 spike: wgpu 30 compute vs native Metal (MSL via objc2-metal) on a
//! 4096x4096 image for three tile kernels, plus an HDR surface probe.
//! `cargo run --release` writes REPORT.md and report.json next to Cargo.toml.

mod data;
mod hdr;
#[cfg(target_os = "macos")]
mod metal_bench;
#[cfg(test)]
mod tests;
mod wgpu_bench;

use serde::Serialize;
use std::time::Instant;

const WARMUP: usize = 3;
const RUNS: usize = 20;
const RATIO_LIMIT: f64 = 1.5;
const ERR_LIMIT: f64 = 1e-4;

#[derive(Clone, Debug)]
pub struct Timing {
    pub gpu_ms: f64,
    pub wall_ms: f64,
    pub gpu_min: f64,
    pub gpu_max: f64,
}

fn median(mut v: Vec<f64>) -> f64 {
    v.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let n = v.len();
    if n % 2 == 1 {
        v[n / 2]
    } else {
        0.5 * (v[n / 2 - 1] + v[n / 2])
    }
}

impl Timing {
    pub fn from_samples(gpu: Vec<f64>, wall: Vec<f64>) -> Timing {
        // A median can hide failed individual measurements (including infinity).
        // Reject them before aggregation for both GPU backends.
        assert!(!gpu.is_empty(), "no timing samples");
        assert_eq!(gpu.len(), wall.len(), "unpaired timing samples");
        assert!(
            gpu.iter().chain(&wall).all(|v| v.is_finite() && *v > 0.0),
            "non-finite or non-positive timing sample"
        );
        let gpu_min = gpu.iter().cloned().fold(f64::INFINITY, f64::min);
        let gpu_max = gpu.iter().cloned().fold(0.0, f64::max);
        Timing {
            gpu_ms: median(gpu),
            wall_ms: median(wall),
            gpu_min,
            gpu_max,
        }
    }
}

#[derive(Serialize, Clone, Debug)]
struct KernelResult {
    name: String,
    backend: String,
    gpu_ms: f64,
    wall_ms: f64,
    max_abs_err: f64,
    mean_abs_err: f64,
    gpu_ms_min: f64,
    gpu_ms_max: f64,
}

#[derive(Serialize)]
struct Report {
    kernels: Vec<KernelResult>,
    /// Supplementary: wgpu with naga runtime bounds checks disabled
    /// (`create_shader_module_trusted` + `ShaderRuntimeChecks::unchecked()`).
    wgpu_unchecked: Vec<KernelResult>,
    hdr: hdr::HdrResult,
    recommendation: String,
    rationale: String,
    environment: Env,
}

#[derive(Serialize)]
struct Env {
    image: String,
    runs: usize,
    warmup: usize,
    wgpu_adapter: String,
    metal_device: String,
    cpu_reference_s: f64,
}

fn kr(name: &str, backend: &str, t: &Timing, err: (f64, f64)) -> KernelResult {
    KernelResult {
        name: name.into(),
        backend: backend.into(),
        gpu_ms: t.gpu_ms,
        wall_ms: t.wall_ms,
        max_abs_err: err.0,
        mean_abs_err: err.1,
        gpu_ms_min: t.gpu_min,
        gpu_ms_max: t.gpu_max,
    }
}

const NAMES: [&str; 3] = [
    "demosaic_bilinear_rggb",
    "guided_filter_r8",
    "lut3d_oklab_33",
];

fn run_wgpu(
    ctx: &wgpu_bench::Ctx,
    backend: &str,
    inputs: &Inputs,
    refs: &[Vec<data::Px>; 3],
) -> Result<Vec<KernelResult>, String> {
    let mut out = Vec::new();
    for (k, name) in NAMES.iter().enumerate() {
        let job = match k {
            0 => ctx.demosaic_job(&inputs.cfa),
            1 => ctx.guided_job(&inputs.noisy),
            _ => ctx.lut_job(&inputs.gt, &inputs.lut),
        };
        let (t, img) = ctx.run(&job, WARMUP, RUNS)?;
        let e = data::errors(&img, &refs[k]);
        eprintln!(
            "  {backend:<15} {name:<24} gpu {:8.3} ms  wall {:8.3} ms  max {:.3e} mean {:.3e}",
            t.gpu_ms, t.wall_ms, e.0, e.1
        );
        out.push(kr(name, backend, &t, e));
    }
    Ok(out)
}

#[cfg(target_os = "macos")]
fn run_metal(
    inputs: &Inputs,
    refs: &[Vec<data::Px>; 3],
) -> Result<(Vec<KernelResult>, String), String> {
    let ctx = metal_bench::Ctx::new()?;
    let mut out = Vec::new();
    for (k, name) in NAMES.iter().enumerate() {
        let job = match k {
            0 => ctx.demosaic_job(&inputs.cfa)?,
            1 => ctx.guided_job(&inputs.noisy)?,
            _ => ctx.lut_job(&inputs.gt, &inputs.lut)?,
        };
        let (t, img) = ctx.run(&job, WARMUP, RUNS)?;
        let e = data::errors(&img, &refs[k]);
        eprintln!(
            "  {:<15} {name:<24} gpu {:8.3} ms  wall {:8.3} ms  max {:.3e} mean {:.3e}",
            "metal", t.gpu_ms, t.wall_ms, e.0, e.1
        );
        out.push(kr(name, "metal", &t, e));
    }
    Ok((out, ctx.device_name.clone()))
}

#[cfg(not(target_os = "macos"))]
fn run_metal(_: &Inputs, _: &[Vec<data::Px>; 3]) -> Result<(Vec<KernelResult>, String), String> {
    Err("native Metal comparison requires macOS".into())
}

struct Inputs {
    gt: Vec<data::Px>,
    cfa: Vec<u16>,
    noisy: Vec<data::Px>,
    lut: Vec<data::Px>,
}

fn main() {
    if matches!(
        std::env::args().nth(1).as_deref(),
        Some("--hdr-srgb" | "--hdr-p3")
    ) {
        println!("{}", serde_json::to_string(&hdr::run_single()).unwrap());
        return;
    }
    if let Err(e) = real_main() {
        eprintln!("gpu-bench failed: {e}");
        std::process::exit(1);
    }
}

fn real_main() -> Result<(), String> {
    // winit must own the main thread's event loop, so probe HDR first.
    eprintln!("HDR surface probe...");
    let hdr = hdr::run();
    eprintln!("{}", hdr.notes);

    eprintln!("Synthesising {}x{} inputs...", data::W, data::H);
    let gt = data::ground_truth();
    let inputs = Inputs {
        cfa: data::mosaic(&gt),
        noisy: data::noisy(&gt),
        lut: data::make_lut(),
        gt,
    };

    eprintln!("CPU f32 references...");
    let t0 = Instant::now();
    let refs = [
        data::cpu_demosaic(&inputs.cfa),
        data::cpu_guided(&inputs.noisy),
        data::cpu_lut(&inputs.gt, &inputs.lut),
    ];
    let cpu_s = t0.elapsed().as_secs_f64();
    // Sanity: demosaic reference vs ground truth, so the synthetic setup is meaningful.
    let flat: Vec<f32> = refs[0].iter().flatten().cloned().collect();
    let (dm_max, dm_mean) = data::errors(&flat, &inputs.gt);
    eprintln!("  demosaic ref vs ground truth: max {dm_max:.3e} mean {dm_mean:.3e} ({cpu_s:.1} s for all refs)");

    eprintln!("wgpu (checked, default)...");
    let wctx = wgpu_bench::Ctx::new(true)?;
    let wgpu_res = run_wgpu(&wctx, "wgpu", &inputs, &refs)?;
    let adapter = wctx.adapter_info.clone();
    drop(wctx);
    eprintln!("wgpu (unchecked)...");
    let uctx = wgpu_bench::Ctx::new(false)?;
    let unchecked = run_wgpu(&uctx, "wgpu-unchecked", &inputs, &refs)?;
    drop(uctx);
    eprintln!("native Metal...");
    let (metal_res, metal_name) = run_metal(&inputs, &refs)?;

    let mut kernels = Vec::new();
    for k in 0..3 {
        kernels.push(wgpu_res[k].clone());
        kernels.push(metal_res[k].clone());
    }
    for k in &kernels {
        let vals = [k.gpu_ms, k.wall_ms, k.max_abs_err, k.mean_abs_err];
        if vals.iter().any(|v| !v.is_finite()) || k.gpu_ms <= 0.0 {
            return Err(format!(
                "non-finite or non-positive result for {} / {}",
                k.name, k.backend
            ));
        }
    }

    let (recommendation, rationale) = decide(&wgpu_res, &metal_res, &unchecked, &hdr);
    let report = Report {
        kernels,
        wgpu_unchecked: unchecked,
        hdr,
        recommendation,
        rationale,
        environment: Env {
            image: format!("{}x{} RGBA f32 (demosaic input u16 RGGB)", data::W, data::H),
            runs: RUNS,
            warmup: WARMUP,
            wgpu_adapter: adapter,
            metal_device: metal_name,
            cpu_reference_s: cpu_s,
        },
    };
    let dir = env!("CARGO_MANIFEST_DIR");
    std::fs::write(
        format!("{dir}/report.json"),
        serde_json::to_string_pretty(&report).unwrap(),
    )
    .map_err(|e| e.to_string())?;
    std::fs::write(
        format!("{dir}/REPORT.md"),
        markdown(&report, dm_max, dm_mean),
    )
    .map_err(|e| e.to_string())?;
    eprintln!(
        "wrote {dir}/REPORT.md and {dir}/report.json: recommendation = {}",
        report.recommendation
    );
    Ok(())
}

fn decide(
    w: &[KernelResult],
    m: &[KernelResult],
    u: &[KernelResult],
    hdr: &hdr::HdrResult,
) -> (String, String) {
    let mut fails = Vec::new();
    let mut notes = Vec::new();
    for k in 0..3 {
        let ratio = w[k].gpu_ms / m[k].gpu_ms;
        let uratio = u[k].gpu_ms / m[k].gpu_ms;
        let slow = ratio > RATIO_LIMIT;
        let inexact = w[k].max_abs_err > ERR_LIMIT;
        let metal_inexact = m[k].max_abs_err > ERR_LIMIT;
        notes.push(format!(
            "{}: wgpu/metal GPU time {:.2}x (unchecked {:.2}x), wgpu max err {:.2e}, metal max err {:.2e}",
            w[k].name, ratio, uratio, w[k].max_abs_err, m[k].max_abs_err
        ));
        if slow || inexact {
            let mut why = Vec::new();
            if slow {
                why.push(format!("{:.2}x slower than MSL", ratio));
            }
            if inexact {
                why.push(format!(
                    "max err {:.2e} > 1e-4{}",
                    w[k].max_abs_err,
                    if metal_inexact {
                        " (native MSL also exceeds it: precision of the f32 algorithm, not wgpu)"
                    } else {
                        ""
                    }
                ));
            }
            fails.push(format!("{} ({})", w[k].name, why.join(", ")));
        }
    }
    let hdr_ok = hdr.extended_srgb_linear || hdr.extended_display_p3;
    let rec = match fails.len() {
        0 => "go",
        3 => "no-go",
        _ => "go-with-msl-passthrough",
    };
    let mut r = match rec {
        "go" => format!(
            "All three kernels are within {RATIO_LIMIT}x of native MSL GPU time with max abs error <= 1e-4 against the f32 CPU reference. "
        ),
        "no-go" => format!("wgpu fails the bar on every kernel: {}. ", fails.join("; ")),
        _ => format!("wgpu fails the bar on: {}; the rest pass. ", fails.join("; ")),
    };
    r.push_str(&notes.join(". "));
    r.push_str(&format!(
        ". HDR: Rgba16Float + ExtendedSrgbLinear configure {}, + ExtendedDisplayP3 configure {}{}.",
        if hdr.extended_srgb_linear {
            "succeeded"
        } else {
            "failed"
        },
        if hdr.extended_display_p3 {
            "succeeded"
        } else {
            "failed"
        },
        if hdr_ok {
            ""
        } else {
            " (presentation is owned by Swift/AppKit per plan §1.2, so this does not block compute)"
        }
    ));
    (rec.into(), r)
}

fn markdown(r: &Report, dm_max: f64, dm_mean: f64) -> String {
    let mut s = String::new();
    s.push_str("# M0-07 GPU spike: wgpu 30 vs native Metal\n\n");
    s.push_str(&format!(
        "Generated by `cargo run --release` in `spikes/gpu-bench`. Image {}; {} timed runs after {} warmups, medians reported. \
         wgpu adapter: {}. Metal device: {}.\n\n",
        r.environment.image, r.environment.runs, r.environment.warmup, r.environment.wgpu_adapter, r.environment.metal_device
    ));
    s.push_str("## Method\n\n");
    s.push_str(
        "- **Kernels** (identical math and summation order in WGSL, MSL and the CPU reference):\n\
         \x20 1. `demosaic_bilinear_rggb`: u16 RGGB CFA (quantised from a known synthetic linear RGB image) to RGBA f32, bilinear, reflect-101 borders. WGSL reads the u16 data packed two-per-u32; MSL reads `ushort` directly.\n\
         \x20 2. `guided_filter_r8`: self-guided filter per channel, r = 8, eps = 1e-3, on a noisy RGBA f32 image; four dispatches (horizontal box of I and I², vertical box + a/b coefficients, horizontal box of a/b, vertical box + output), clipped-window means.\n\
         \x20 3. `lut3d_oklab_33`: linear sRGB to Oklab, 33³ trilinear LUT (L^0.9, 10° hue rotation, 1.15x chroma) indexed by (L, a+0.5, b+0.5), back to linear sRGB.\n\
         - **GPU time**: wgpu uses `Features::TIMESTAMP_QUERY` with `ComputePassTimestampWrites` at the beginning and end of the single compute pass holding all dispatches, `resolve_query_set`, scaled by `Queue::get_timestamp_period`. Metal uses `MTLCommandBuffer.GPUEndTime - GPUStartTime` of the single command buffer / serial compute encoder holding the same dispatches. Both use 16x16 workgroups over the full image and storage buffers.\n\
         - **Wall time** (context only): encode, submit, wait, and copy the full 256 MiB output into a host `Vec<f32>` (wgpu: copy to a MAP_READ staging buffer + map; Metal: `StorageModeShared` buffer, memcpy from `contents()`).\n\
         - **Accuracy**: scalar f32 CPU reference (same formulas; `f32::cbrt` where the GPU uses `sign(x)*pow(|x|,1/3)`), max and mean absolute error over RGBA in linear units. MSL is compiled at runtime with default `MTLCompileOptions` (Metal's default fast-math); wgpu/naga generates its own MSL.\n",
    );
    s.push_str(&format!(
        "- Sanity: CPU demosaic vs the synthetic ground truth: max {dm_max:.3e}, mean {dm_mean:.3e} (bilinear interpolation error, not backend error).\n\n"
    ));

    s.push_str("## GPU timing (median ms)\n\n| kernel | wgpu | metal | wgpu / metal | wgpu unchecked | unchecked / metal | wgpu wall | metal wall |\n|---|---:|---:|---:|---:|---:|---:|---:|\n");
    for k in 0..3 {
        let w = &r.kernels[2 * k];
        let m = &r.kernels[2 * k + 1];
        let u = &r.wgpu_unchecked[k];
        s.push_str(&format!(
            "| {} | {:.3} | {:.3} | {:.2}x | {:.3} | {:.2}x | {:.1} | {:.1} |\n",
            w.name,
            w.gpu_ms,
            m.gpu_ms,
            w.gpu_ms / m.gpu_ms,
            u.gpu_ms,
            u.gpu_ms / m.gpu_ms,
            w.wall_ms,
            m.wall_ms
        ));
    }
    s.push_str("\nGPU time spread (min to max over the 20 runs):\n\n| kernel | backend | min | max |\n|---|---|---:|---:|\n");
    for k in r.kernels.iter().chain(&r.wgpu_unchecked) {
        s.push_str(&format!(
            "| {} | {} | {:.3} | {:.3} |\n",
            k.name, k.backend, k.gpu_ms_min, k.gpu_ms_max
        ));
    }
    s.push_str("\n## Error vs f32 CPU reference (linear units, RGBA)\n\n| kernel | backend | max abs err | mean abs err |\n|---|---|---:|---:|\n");
    for k in r.kernels.iter().chain(&r.wgpu_unchecked) {
        s.push_str(&format!(
            "| {} | {} | {:.3e} | {:.3e} |\n",
            k.name, k.backend, k.max_abs_err, k.mean_abs_err
        ));
    }
    s.push_str("\n## HDR surface (wgpu 30, Metal, hidden winit window)\n\n");
    s.push_str(&format!(
        "- `SurfaceConfiguration {{ format: Rgba16Float, color_space: SurfaceColorSpace::ExtendedSrgbLinear }}`: **{}**\n\
         - `SurfaceConfiguration {{ format: Rgba16Float, color_space: SurfaceColorSpace::ExtendedDisplayP3 }}`: **{}**\n\n```\n{}\n```\n\n",
        if r.hdr.extended_srgb_linear { "configured OK" } else { "failed" },
        if r.hdr.extended_display_p3 { "configured OK" } else { "failed" },
        r.hdr.notes
    ));
    s.push_str(&format!("## Recommendation: `{}`\n\n", r.recommendation));
    s.push_str(&recommendation_paragraph(r));
    s.push('\n');
    s
}

fn recommendation_paragraph(r: &Report) -> String {
    let ratios: Vec<String> = (0..3)
        .map(|k| {
            format!(
                "{} {:.2}x",
                r.kernels[2 * k].name,
                r.kernels[2 * k].gpu_ms / r.kernels[2 * k + 1].gpu_ms
            )
        })
        .collect();
    let max_err = r
        .kernels
        .iter()
        .filter(|k| k.backend == "wgpu")
        .map(|k| k.max_abs_err)
        .fold(0.0, f64::max);
    let base = format!(
        "Against the rule of thumb (wgpu within 1.5x of native MSL GPU time on all three kernels, max abs error <= 1e-4), \
         wgpu measured {} of native time, with a worst-case wgpu error of {:.2e}. ",
        ratios.join(", "),
        max_err
    );
    let tail = match r.recommendation.as_str() {
        "go" => "wgpu is acceptable for pipeline compute on Apple silicon: keep WGSL as the single kernel source, keep the \
                 per-backend tolerance gate (<= 1e-4 vs the CPU reference) in CI, and re-run this spike when kernels change \
                 materially or wgpu is upgraded. Presentation stays in Swift/AppKit as planned; MSL passthrough remains the \
                 documented fallback if a future kernel misses the bar."
            .to_string(),
        "go-with-msl-passthrough" => "wgpu is acceptable as the default compute path, but the kernel(s) that miss the bar should \
             ship through the MSL passthrough (`wgpu::hal` / `create_shader_module_passthrough` or a small objc2-metal side path \
             on the same MTLDevice), keeping the WGSL version as the portable fallback and the CPU tolerance gate on both. If the \
             miss is precision on both backends, fix the algorithm (e.g. centre the data before the variance computation) rather \
             than the backend."
            .to_string(),
        _ => "wgpu is not acceptable as the primary compute path on Apple silicon: write the tile kernels in MSL against \
              objc2-metal (or MSL passthrough under wgpu), keep wgpu only for portability, and revisit after profiling the \
              naga-generated MSL for the gap."
            .to_string(),
    };
    base + &tail
}
