//! ENG-15 (REV-ENG-14 SHOULD-FIX 2): an export the GPU declines must leave
//! nothing on the device. Texture, Clarity and Dehaze need whole-frame
//! statistics, so a frame larger than one GPU tile band falls back to the CPU.
//! Before ENG-15 the export first built the output's ICC, gamut and transfer
//! tables with uploads that wgpu only flushes at the next submission: every
//! declined export left about 1.8 MiB on the device, without bound in a
//! headless batch of such images.
//!
//! A single-test binary: the Metal allocation counter is process-wide.
use engine_api::{
    color::ColorMatrix3,
    jobs::CancellationToken,
    recipe::{EditMeta, Recipe, settings::LensProfileSource},
};
use export::{ExportImage, ExportSettings, Format, Metadata};
use pipeline_cpu::RenderSource;
use raw_decode::{CfaImage, CfaLayout, RawMetadata};

/// Wider and taller than one GPU tile band at the export budget (2048 wide:
/// 768 rows), so presence recipes are declined.
const WIDTH: u32 = 2048;
const HEIGHT: u32 = 1024;

fn fixture() -> (CfaImage, RawMetadata) {
    let m = RawMetadata {
        make: "synthetic".into(),
        model: "camera".into(),
        lens: None,
        iso: 100.,
        shutter_s: 0.01,
        aperture: 4.,
        focal_mm: 50.,
        capture_time: 0,
        orientation: 1,
        catalog_orientation: None,
        baseline_exposure: 0.,
        width: WIDTH,
        height: HEIGHT,
        cfa_layout: CfaLayout::Bayer([[0, 1], [1, 2]]),
        black_levels: [0.; 4],
        white_level: 65535,
        as_shot_wb: [2., 1., 1.5, 1.],
        camera_to_xyz: ColorMatrix3::IDENTITY,
        cam_xyz: [[0.7, 0.2, 0.1], [0.1, 0.8, 0.1], [0.1, 0.2, 0.7], [0.; 3]],
        rgb_cam: [[0.; 4]; 3],
        default_crop: [0, 0, WIDTH, HEIGHT],
        has_gain_map: false,
        has_opcode_list: false,
        opcode_lists: [None, None, None],
    };
    let samples = (0..WIDTH * HEIGHT)
        .map(|i| 0.05 + 0.4 * ((i % WIDTH) as f32 / WIDTH as f32))
        .collect();
    (CfaImage::from_linear(WIDTH, HEIGHT, samples).unwrap(), m)
}

fn presence(edit: fn(&mut engine_api::recipe::DevelopSettings)) -> Recipe {
    let mut recipe = Recipe::default();
    recipe
        .edit(EditMeta::user("presence", 0), |s| {
            s.lens.profile = LensProfileSource::None;
            edit(s);
        })
        .unwrap();
    recipe
}

#[test]
fn gpu_declined_exports_leave_no_device_memory() {
    if std::env::var("TESSERA_EXPORT_BACKEND").as_deref() == Ok("cpu") {
        return;
    }
    let Some(_) = export::idle_device_allocated_bytes() else {
        eprintln!("SKIPPED: no Metal allocation counter");
        return;
    };
    let (cfa, metadata) = fixture();
    let dir = tempfile::tempdir().unwrap();
    let recipes = [
        presence(|s| s.tone.clarity = 30.),
        presence(|s| s.tone.dehaze = 25.),
        presence(|s| s.tone.texture = 20.),
    ];
    let export = |recipe: &Recipe, resize: export::Resize| {
        let settings = ExportSettings {
            output_dir: dir.path().into(),
            format: Format::Png,
            metadata: Metadata::None,
            resize,
            ..Default::default()
        };
        let image = ExportImage {
            source: RenderSource::Cfa {
                image: &cfa,
                metadata: &metadata,
            },
            name: "declined",
            sequence: 1,
            date: "",
            metadata: None,
        };
        export::render_one_cancellable(
            &image,
            recipe,
            &settings,
            &CancellationToken::new(),
            None,
            None,
        )
        .unwrap();
    };
    // Warm-up: the device, its pipelines and any one-time state.
    export(&recipes[0], export::Resize::None);
    let baseline = export::idle_device_allocated_bytes().unwrap();
    let mut after = Vec::new();
    for round in 0..2 {
        for recipe in &recipes {
            for resize in [export::Resize::None, export::Resize::LongEdge(1500)] {
                export(recipe, resize);
                after.push(export::idle_device_allocated_bytes().unwrap());
            }
        }
        eprintln!("round {round}: idle device bytes {after:?} (baseline {baseline})");
    }
    let mib = |b: u64| b as f64 / (1 << 20) as f64;
    let growth: Vec<f64> = after
        .iter()
        .map(|&b| mib(b.saturating_sub(baseline)))
        .collect();
    eprintln!(
        "DECLINED baseline={:.2} MiB growth after each of {} declined exports (MiB): {growth:.2?}",
        mib(baseline),
        after.len()
    );
    // Unchanged: within 64 KiB of the idle baseline after every export (the
    // leak was about 1.8 MiB per export, accumulating).
    assert!(
        after.iter().all(|&b| b <= baseline + (64 << 10)),
        "declined exports grew the idle device baseline: {growth:.2?} MiB"
    );
}
