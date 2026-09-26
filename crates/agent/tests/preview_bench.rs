//! Run with cargo test -p agent --release --test preview_bench -- --ignored --nocapture.
use agent::{Agent, Config, metrics, providers::FakePlanner};
use engine_api::{
    recipe::DevelopSettings,
    tools::{ToneUpdate, ToolCall, ToolRequest},
};
use std::{
    path::PathBuf,
    time::{Duration, Instant},
};
use style_profile::{Profile, Questionnaire};
use tessera_mcp::Console;

#[test]
#[ignore = "five real RAW fixtures; prints full-resolution CPU baseline and cached GPU timings"]
fn five_fixture_preview_benchmark() {
    let root = std::env::var_os("PIPELINE_RAW_FIXTURES")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/raw"));
    let mut failures = Vec::new();
    for name in [
        "canon-cr3.CR3",
        "sony-arw.ARW",
        "nikon-nef.NEF",
        "fuji-raf.RAF",
        "sample.dng",
    ] {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(name);
        std::fs::copy(root.join(name), &path).unwrap();
        let mut console = Console::open(dir.path().join("console")).unwrap();
        let id = console.open_image(&path).unwrap();
        let start = Instant::now();
        let preview = console.render_preview(id, 1024).unwrap();
        let cold = start.elapsed();
        let preview_metrics = metrics::measure(&preview, &[], None).unwrap();
        let start = Instant::now();
        let mut raw = raw_decode::RawSource::open(&path).unwrap();
        let cfa = raw.decode_cfa().unwrap();
        let metadata = raw.metadata();
        let full = pipeline_cpu::render(
            &DevelopSettings::default(),
            &pipeline_cpu::RenderSource::Cfa {
                image: &cfa,
                metadata: &metadata,
            },
        )
        .unwrap();
        let baseline = start.elapsed();
        let full_metrics = metrics::measure(&full, &[], None).unwrap();
        for (metric, a, b) in [
            (
                "mean",
                preview_metrics.mean_luminance,
                full_metrics.mean_luminance,
            ),
            (
                "shadows",
                preview_metrics.shadow_clipping,
                full_metrics.shadow_clipping,
            ),
            (
                "highlights",
                preview_metrics.highlight_clipping,
                full_metrics.highlight_clipping,
            ),
        ] {
            let error = (a - b).abs();
            eprintln!(
                "METRIC {name} {metric} preview={a:.6} full={b:.6} absolute_error={error:.6}"
            );
            if error > 0.02 {
                failures.push(format!("{name} {metric}: {error}"));
            }
        }
        drop(full);
        drop(cfa);
        drop(console);
        let mut agent = Agent::open(
            dir.path().join("agent"),
            Profile::new("bench", Questionnaire::default()).unwrap(),
            Config {
                max_iterations: 1,
                time_budget: Duration::from_secs(300),
                ..Default::default()
            },
        )
        .unwrap();
        let start = Instant::now();
        let packet = agent.perceive(&path).unwrap();
        let perception = start.elapsed();
        let steps = [0.1, 0.2, 0.3].map(|exposure| ToolRequest {
            call: ToolCall::SetTone {
                image: packet.image,
                update: ToneUpdate {
                    exposure: Some(exposure),
                    ..Default::default()
                },
            },
            rationale: Some("benchmark exposure step".into()),
            group: None,
            expect_recipe: None,
        });
        let mut planner = FakePlanner::new([steps.to_vec()]);
        let start = Instant::now();
        let report = agent.edit(&path, Some(&mut planner), None, false).unwrap();
        let elapsed = start.elapsed();
        assert_eq!(report.plans[0].len(), 3);
        eprintln!(
            "BENCH {name}: baseline_cpu_decode_full={baseline:?} cold_gpu_preview={cold:?} perception={perception:?} warm_fake_planner_3_steps={elapsed:?} amortized_step={:?}",
            elapsed / 3
        );
    }
    assert!(
        failures.is_empty(),
        "preview/full > 2 percentage points: {failures:?}"
    );
}
