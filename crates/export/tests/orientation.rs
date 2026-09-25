//! Opt-in display orientation, recorded density and file-less renders (print).
use engine_api::{jobs::CancellationToken, recipe::Recipe};
use export::*;
use pipeline_cpu::RenderSource;

fn metadata(orientation: u16) -> raw_decode::RawMetadata {
    raw_decode::RawMetadata {
        make: "test".into(),
        model: "test".into(),
        lens: None,
        iso: 100.,
        shutter_s: 0.01,
        aperture: 4.,
        focal_mm: 50.,
        capture_time: 0,
        orientation,
        width: 32,
        height: 24,
        cfa_layout: raw_decode::CfaLayout::Bayer([[0, 1], [3, 2]]),
        black_levels: [0.; 4],
        white_level: 65535,
        as_shot_wb: [1.; 4],
        camera_to_xyz: engine_api::color::ColorMatrix3::IDENTITY,
        cam_xyz: [[1., 0., 0.], [0., 1., 0.], [0., 0., 1.], [0., 0., 0.]],
        rgb_cam: [[1., 0., 0., 0.], [0., 1., 0., 0.], [0., 0., 1., 0.]],
        default_crop: [3, 1, 26, 22],
        has_gain_map: false,
        has_opcode_list: false,
        opcode_lists: [None, None, None],
    }
}

#[test]
fn apply_orientation_rotates_raw_exports_only_when_asked() {
    let cfa = raw_decode::CfaImage::from_linear(32, 24, vec![0.05; 768]).unwrap();
    let metadata = metadata(6);
    let dir = tempfile::tempdir().unwrap();
    let image = |name| ExportImage {
        source: RenderSource::Cfa {
            image: &cfa,
            metadata: &metadata,
        },
        name,
        sequence: 1,
        date: "",
        metadata: None,
    };
    let settings = ExportSettings {
        output_dir: dir.path().into(),
        metadata: Metadata::None,
        ..Default::default()
    };
    let sensor = export_one(&image("sensor"), &Recipe::default(), &settings).unwrap();
    assert_eq!(image::image_dimensions(sensor).unwrap(), (26, 22));
    let oriented = ExportSettings {
        apply_orientation: true,
        dpi: Some(300),
        ..settings
    };
    let cancel = CancellationToken::new();
    let path = export_one_cancellable(
        &image("display"),
        &Recipe::default(),
        &oriented,
        &cancel,
        None,
        None,
    )
    .unwrap();
    assert_eq!(image::image_dimensions(&path).unwrap(), (22, 26));
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(&path).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o644, "exports are ordinary readable files");
    }
    // Binned rendering for small outputs keeps the requested size.
    let binned = ExportSettings {
        render_scale: 2,
        resize: Resize::LongEdge(10),
        ..oriented.clone()
    };
    let path = export_one(&image("binned"), &Recipe::default(), &binned).unwrap();
    assert_eq!(image::image_dimensions(path).unwrap(), (8, 10));
    let invalid = ExportSettings {
        render_scale: 3,
        ..binned
    };
    assert!(export_one(&image("invalid"), &Recipe::default(), &invalid).is_err());
    // A cancelled token writes nothing.
    cancel.cancel();
    assert!(
        export_one_cancellable(
            &image("cancelled"),
            &Recipe::default(),
            &oriented,
            &cancel,
            None,
            None
        )
        .is_err()
    );
    assert!(!dir.path().join("cancelled-1.jpg").exists());
}

#[test]
fn render_pixels_orients_resizes_and_bins() {
    let cfa = raw_decode::CfaImage::from_linear(32, 24, vec![0.05; 768]).unwrap();
    let metadata = metadata(8);
    let image = ExportImage {
        source: RenderSource::Cfa {
            image: &cfa,
            metadata: &metadata,
        },
        name: "print",
        sequence: 1,
        date: "",
        metadata: None,
    };
    let request = RenderRequest {
        color_space: ColorSpace::ProPhoto,
        resize: Resize::Fit(11, 13),
        sharpen_for: SharpenFor::Matte,
        scale: 1,
    };
    let cancel = CancellationToken::new();
    let rgb = render_pixels(&image, &Recipe::default(), &request, &cancel, None).unwrap();
    assert_eq!(rgb.dimensions(), (11, 13));
    let binned = render_pixels(
        &image,
        &Recipe::default(),
        &RenderRequest {
            scale: 2,
            ..request
        },
        &cancel,
        None,
    )
    .unwrap();
    assert_eq!(binned.dimensions(), (11, 13));
    assert!(
        render_pixels(
            &image,
            &Recipe::default(),
            &RenderRequest {
                scale: 3,
                ..request
            },
            &cancel,
            None
        )
        .is_err()
    );
    assert!(!color_space_icc(ColorSpace::ProPhoto).unwrap().is_empty());
    assert!(!needs_segmenter(&Recipe::default()));
}
