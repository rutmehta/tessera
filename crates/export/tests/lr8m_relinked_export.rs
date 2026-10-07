//! LR-8m (A-LR8 M4): a relinked original (catalog orientation from an
//! imported Smart Preview) exports through the normal RAW path, GPU included,
//! exactly like an ordinary import with the same EXIF orientation.
use engine_api::{
    jobs::CancellationToken,
    recipe::{EditMeta, Recipe},
};
use export::*;
use pipeline_cpu::RenderSource;

fn metadata(orientation: u16, catalog: Option<u16>) -> raw_decode::RawMetadata {
    raw_decode::RawMetadata {
        make: "test".into(),
        model: "test".into(),
        lens: None,
        iso: 100.,
        shutter_s: 0.01,
        aperture: 4.,
        focal_mm: 50.,
        capture_time: 0,
        catalog_orientation: catalog,
        baseline_exposure: 0.,
        orientation,
        width: 256,
        height: 192,
        cfa_layout: raw_decode::CfaLayout::Bayer([[0, 1], [3, 2]]),
        black_levels: [0.; 4],
        white_level: 65535,
        as_shot_wb: [1.8, 1., 1.4, 1.],
        camera_to_xyz: engine_api::color::ColorMatrix3::IDENTITY,
        cam_xyz: [[1., 0., 0.], [0., 1., 0.], [0., 0., 1.], [0., 0., 0.]],
        rgb_cam: [[1., 0., 0., 0.], [0., 1., 0., 0.], [0., 0., 1., 0.]],
        default_crop: [2, 2, 252, 188],
        has_gain_map: false,
        has_opcode_list: false,
        opcode_lists: [None, None, None],
    }
}

#[test]
fn lr8m_relinked_original_exports_like_an_ordinary_import() {
    let cfa = raw_decode::CfaImage::from_linear(
        256,
        192,
        (0..256 * 192)
            .map(|i| 0.04 + (i % 256) as f32 / 900. + (i / 256) as f32 / 1400.)
            .collect(),
    )
    .unwrap();
    let ordinary = metadata(6, None);
    // What the bridge opens for a relinked original (tessera-ffi export
    // Source::open): the catalog orientation is the display orientation.
    let relinked = metadata(6, Some(6));
    let dir = tempfile::tempdir().unwrap();
    let settings = ExportSettings {
        output_dir: dir.path().into(),
        metadata: Metadata::None,
        format: Format::Png,
        apply_orientation: true,
        ..Default::default()
    };
    let mut recipe = Recipe::default();
    recipe
        .edit(EditMeta::user("LR-8m relink edits", 1), |s| {
            s.tone.exposure = 0.3;
            s.geometry.crop.rect.left = 0.125;
            s.geometry.crop.rect.bottom = 0.75;
        })
        .unwrap();
    let export = |metadata: &raw_decode::RawMetadata, name| {
        let image = ExportImage {
            source: RenderSource::Cfa {
                image: &cfa,
                metadata,
            },
            name,
            sequence: 1,
            date: "",
            metadata: None,
        };
        let rendered = render_one_cancellable(
            &image,
            &recipe,
            &settings,
            &CancellationToken::new(),
            None,
            None,
        )
        .unwrap();
        let used_gpu = rendered.used_gpu();
        let path = rendered.finish(&CancellationToken::new()).unwrap();
        (used_gpu, image::open(path).unwrap().to_rgb8())
    };
    let (ordinary_gpu, expected) = export(&ordinary, "ordinary");
    let (relinked_gpu, actual) = export(&relinked, "relinked");
    assert_eq!(
        relinked_gpu, ordinary_gpu,
        "a relinked original takes the same export backend as an ordinary import"
    );
    assert_eq!(actual.dimensions(), expected.dimensions());
    assert!(
        actual.width() < actual.height(),
        "oriented once, at the end"
    );
    assert!(actual == expected, "relinked export pixels differ");
}
