//! ENG-7b/7c: exports report a named lens profile that is not available (S3)
//! in the existing per-file export warnings (`<file>.tessera-warnings.txt`),
//! which the app lists in the completion toast. A default (Auto) raw with no
//! profile available is the normal case and writes no warnings file (REV2 N-B2).
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
    // ENG-7c (REV2 N-B2): Auto without a profile is the normal case (Tessera
    // has no lens database), so it is not a per-file warning.
    assert!(warnings(raw(), LensProfileSource::Auto).is_empty());
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

/// ENG-7c (REV2 N-B2): a default raw export writes no warnings file.
#[test]
fn default_raw_export_writes_no_warnings_file() {
    let (cfa, m) = fixture(32, 24);
    let dir = tempfile::tempdir().unwrap();
    let settings = ExportSettings {
        output_dir: dir.path().into(),
        format: Format::Png,
        metadata: Metadata::None,
        ..Default::default()
    };
    let image = ExportImage {
        source: RenderSource::Cfa {
            image: &cfa,
            metadata: &m,
        },
        name: "default",
        sequence: 1,
        date: "",
        metadata: None,
    };
    let path = export::export_one(&image, &Recipe::default(), &settings).unwrap();
    assert!(path.exists());
    let files: Vec<_> = std::fs::read_dir(dir.path())
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert!(
        !files.iter().any(|f| f.ends_with(".tessera-warnings.txt")),
        "{files:?}"
    );
}
