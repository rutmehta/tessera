//! ENG-7b: exports report when no lens profile was applied (export ruling)
//! and when a named lens profile is not available (S3). The note travels in
//! the existing per-file export warnings (`<file>.tessera-warnings.txt`),
//! which the app lists in the completion toast.
use engine_api::{
    color::ColorMatrix3,
    jobs::CancellationToken,
    recipe::{
        EditMeta, Recipe,
        settings::{LensProfileRef, LensProfileSource},
    },
};
use export::{ExportImage, ExportSettings, Format, Metadata};
use pipeline_cpu::RenderSource;
use raw_decode::{CfaImage, CfaLayout, RawMetadata};

fn fixture(w: u32, h: u32) -> (CfaImage, RawMetadata) {
    let m = RawMetadata {
        make: "synthetic".into(),
        model: "camera".into(),
        lens: Some("Synthetic Zoom".into()),
        iso: 100.,
        shutter_s: 0.01,
        aperture: 4.,
        focal_mm: 50.,
        capture_time: 0,
        orientation: 1,
        catalog_orientation: None,
        baseline_exposure: 0.,
        width: w,
        height: h,
        cfa_layout: CfaLayout::Bayer([[0, 1], [1, 2]]),
        black_levels: [0.; 4],
        white_level: 65535,
        as_shot_wb: [2., 1., 1.5, 1.],
        camera_to_xyz: ColorMatrix3::IDENTITY,
        cam_xyz: [[0.7, 0.2, 0.1], [0.1, 0.8, 0.1], [0.1, 0.2, 0.7], [0.; 3]],
        rgb_cam: [[0.; 4]; 3],
        default_crop: [0, 0, w, h],
        has_gain_map: false,
        has_opcode_list: false,
        opcode_lists: [None, None, None],
    };
    let c = CfaImage::from_linear(w, h, vec![0.2; (w * h) as usize]).unwrap();
    (c, m)
}

fn warnings(source: RenderSource<'_>, profile: LensProfileSource) -> Vec<String> {
    let mut recipe = Recipe::default();
    recipe
        .edit(EditMeta::user("lens", 0), |s| s.lens.profile = profile)
        .unwrap();
    let dir = tempfile::tempdir().unwrap();
    let settings = ExportSettings {
        output_dir: dir.path().into(),
        format: Format::Png,
        metadata: Metadata::None,
        ..Default::default()
    };
    let image = ExportImage {
        source,
        name: "lens",
        sequence: 1,
        date: "",
        metadata: None,
    };
    export::render_one_cancellable(
        &image,
        &recipe,
        &settings,
        &CancellationToken::new(),
        None,
        None,
    )
    .unwrap()
    .warnings()
    .to_vec()
}

#[test]
fn raw_export_notes_missing_and_unavailable_lens_profiles() {
    let (cfa, m) = fixture(32, 24);
    let raw = || RenderSource::Cfa {
        image: &cfa,
        metadata: &m,
    };
    assert_eq!(
        warnings(raw(), LensProfileSource::Auto),
        ["No lens profile available — no profile correction applied"]
    );
    assert_eq!(
        warnings(
            raw(),
            LensProfileSource::Database {
                profile: LensProfileRef::named("Adobe (Synthetic Missing Lens)"),
            }
        ),
        [
            "Lens profile 'Adobe (Synthetic Missing Lens)' not available — no profile correction applied"
        ]
    );
    assert!(warnings(raw(), LensProfileSource::None).is_empty());
    let pixels = pipeline_cpu::Image::new(8, 6, vec![vec![0.18; 48]; 3]).unwrap();
    assert!(warnings(RenderSource::Rgb(&pixels), LensProfileSource::Auto).is_empty());
}
