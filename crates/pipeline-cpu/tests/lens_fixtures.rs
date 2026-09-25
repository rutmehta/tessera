//! Explicit opt-in real RAW suite; absence is a failure, never a silent pass.
use engine_api::recipe::{DevelopSettings, settings::UprightMode};
use pipeline_cpu::{RenderSource, render_linear_scaled};
#[test]
#[ignore = "full-resolution five-camera acceptance: cargo test --release -p pipeline-cpu --test lens_fixtures -- --ignored --nocapture"]
fn five_actual_raws_auto_lens_and_upright_are_finite() {
    let root = std::env::var_os("PIPELINE_RAW_FIXTURES")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| {
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/raw")
        });
    for name in [
        "canon-cr3.CR3",
        "sony-arw.ARW",
        "nikon-nef.NEF",
        "fuji-raf.RAF",
        "sample.dng",
    ] {
        let mut raw = raw_decode::RawSource::open(root.join(name)).unwrap();
        let cfa = raw.decode_cfa().unwrap();
        let metadata = raw.metadata();
        let mut settings = DevelopSettings::default();
        settings.geometry.upright.mode = UprightMode::Auto;
        let rendered = render_linear_scaled(
            &settings,
            &RenderSource::Cfa {
                image: &cfa,
                metadata: &metadata,
            },
            8,
        )
        .unwrap();
        assert!(rendered.planes().iter().flatten().all(|v| v.is_finite()));
        assert_eq!(rendered.width(), metadata.default_crop[2].div_ceil(8));
        assert_eq!(rendered.height(), metadata.default_crop[3].div_ceil(8));
        eprintln!(
            "{name}: {}x{} finite; opcode bytes {:?}",
            rendered.width(),
            rendered.height(),
            metadata
                .opcode_lists
                .each_ref()
                .map(|x| x.as_ref().map(Vec::len))
        );
    }
}
