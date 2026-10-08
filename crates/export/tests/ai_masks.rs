use engine_api::recipe::{
    EditMeta, Recipe,
    mask::{LocalAdjustment, LocalParams, MaskCombine, MaskComponent, MaskKind},
    settings::NormalizedRect,
};
use export::{
    ExportImage, ExportSettings, Metadata, export_one, export_one_with_segmenter,
    mask_ai::{MaskSegmenter, SegmentRequest},
};
use pipeline_cpu::{Image, RenderSource};

struct Fake {
    alpha: f32,
    fail: bool,
    calls: usize,
}
impl MaskSegmenter for Fake {
    fn segment(&mut self, image: &image::RgbImage, _: &SegmentRequest) -> anyhow::Result<Vec<f32>> {
        self.calls += 1;
        anyhow::ensure!(!self.fail, "inference failed");
        Ok(vec![self.alpha; (image.width() * image.height()) as usize])
    }
}
fn fixture() -> Image {
    Image::new(24, 16, vec![vec![0.18; 384]; 3]).unwrap()
}
fn input(pixels: &Image) -> ExportImage<'_> {
    ExportImage {
        source: RenderSource::Rgb(pixels),
        name: "photo",
        sequence: 1,
        date: "",
        metadata: None,
    }
}
fn recipe(components: Vec<MaskComponent>) -> Recipe {
    let mut recipe = Recipe::default();
    recipe
        .edit(EditMeta::user("mask", 0), |s| {
            s.locals.adjustments.push(LocalAdjustment {
                components,
                params: LocalParams {
                    exposure: 1.0,
                    ..Default::default()
                },
                ..Default::default()
            });
        })
        .unwrap();
    recipe
}
fn subject() -> MaskComponent {
    MaskComponent::new(MaskKind::Subject { model: None })
}

#[test]
fn invalid_or_failed_segmentation_never_publishes_image_or_sidecar() {
    let pixels = fixture();
    let input = input(&pixels);
    let recipe = recipe(vec![subject()]);
    for (alpha, fail) in [(f32::NAN, false), (1.1, false), (0.0, true)] {
        let dir = tempfile::tempdir().unwrap();
        let settings = ExportSettings {
            output_dir: dir.path().into(),
            ..Default::default()
        };
        let mut backend = Fake {
            alpha,
            fail,
            calls: 0,
        };
        let error =
            export_one_with_segmenter(&input, &recipe, &settings, &mut backend).unwrap_err();
        assert!(error.to_string().contains(if fail {
            "inference failed"
        } else {
            "invalid segmentation raster"
        }));
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 0);
    }
    // No model at all is the same error (LR-5c ruling 1): the model cannot be
    // loaded from this support root, deterministically and without network.
    let dir = tempfile::tempdir().unwrap();
    let support = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(support.path().join("models/models.toml")).unwrap();
    let settings = ExportSettings {
        output_dir: dir.path().into(),
        mask_support: Some(support.path().into()),
        ..Default::default()
    };
    let error = export_one(&input, &recipe, &settings).unwrap_err();
    assert!(error.to_string().contains("AI mask"), "{error}");
    assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 0);
}

#[test]
fn mixed_ai_composition_runs_before_geometry_and_matches_procedural_reference() {
    let pixels = fixture();
    let input = input(&pixels);
    let dir = tempfile::tempdir().unwrap();
    let settings = ExportSettings {
        output_dir: dir.path().into(),
        metadata: Metadata::None,
        ..Default::default()
    };
    let radial = MaskComponent::new(MaskKind::Radial {
        center: [0.4, 0.5],
        radii: [0.25, 0.4],
        angle: 0.0,
        feather: 30.0,
    });
    let mut intersect = radial.clone();
    intersect.combine = MaskCombine::Intersect;
    let mut ai = recipe(vec![subject(), intersect]);
    let mut reference = recipe(vec![radial]);
    for r in [&mut ai, &mut reference] {
        r.edit(EditMeta::user("crop and tone", 1), |s| {
            s.geometry.crop.rect = NormalizedRect {
                left: 0.25,
                top: 0.0,
                right: 0.75,
                bottom: 1.0,
            };
            s.tone.contrast = 20.0;
            s.color.saturation = 15.0;
        })
        .unwrap();
    }
    let mut backend = Fake {
        alpha: 1.0,
        fail: false,
        calls: 0,
    };
    let actual = export_one_with_segmenter(&input, &ai, &settings, &mut backend).unwrap();
    let actual = image::open(actual).unwrap().to_rgb8();
    let expected = export_one(
        &input,
        &reference,
        &ExportSettings {
            naming: "reference".into(),
            ..settings
        },
    )
    .unwrap();
    let expected = image::open(expected).unwrap().to_rgb8();
    assert_eq!(actual.dimensions(), (12, 16));
    assert_eq!(actual, expected);
    assert_eq!(backend.calls, 1);
}

#[test]
fn disabled_ai_never_calls_backend_and_matches_default_export() {
    let pixels = fixture();
    let input = input(&pixels);
    let mut recipe = recipe(vec![subject()]);
    recipe
        .edit(EditMeta::user("disable", 1), |s| {
            s.locals.adjustments[0].enabled = false
        })
        .unwrap();
    let dir = tempfile::tempdir().unwrap();
    let settings = ExportSettings {
        output_dir: dir.path().into(),
        metadata: Metadata::None,
        ..Default::default()
    };
    let mut backend = Fake {
        alpha: 0.0,
        fail: true,
        calls: 0,
    };
    let path = export_one_with_segmenter(&input, &recipe, &settings, &mut backend).unwrap();
    // Each export builds its own LittleCMS profile, whose ICC header records
    // creation time. Exercise different seconds rather than relying on two
    // exports happening in the same second for this equality regression.
    std::thread::sleep(std::time::Duration::from_millis(1100));
    let reference = export_one(
        &input,
        &Recipe::default(),
        &ExportSettings {
            naming: "reference".into(),
            ..settings
        },
    )
    .unwrap();
    let without_icc_creation_time = |path| {
        let mut bytes = std::fs::read(path).unwrap();
        let marker = bytes
            .windows(12)
            .position(|b| b == b"ICC_PROFILE\0")
            .unwrap();
        // This small built-in profile occupies one APP2 segment. Confirm the
        // sequence/count and ICC signature before touching header bytes 24–35.
        assert_eq!(&bytes[marker + 12..marker + 14], &[1, 1]);
        let start = marker + 14;
        assert_eq!(&bytes[start + 36..start + 40], b"acsp");
        bytes[start + 24..start + 36].fill(0);
        bytes
    };
    assert_eq!(
        without_icc_creation_time(path),
        without_icc_creation_time(reference)
    );
    assert_eq!(backend.calls, 0);
}

#[test]
fn empty_object_prompt_is_rejected_instead_of_guessing_a_mask() {
    let pixels = fixture();
    let input = input(&pixels);
    let recipe = recipe(vec![MaskComponent::new(MaskKind::Object {
        prompt: Some("dog".into()),
        region: None,
        points: vec![],
        model: None,
    })]);
    let dir = tempfile::tempdir().unwrap();
    let settings = ExportSettings {
        output_dir: dir.path().into(),
        ..Default::default()
    };
    let mut backend = Fake {
        alpha: 1.0,
        fail: false,
        calls: 0,
    };
    assert!(export_one_with_segmenter(&input, &recipe, &settings, &mut backend).is_err());
    assert_eq!(backend.calls, 0);
    let error = export_one(&input, &recipe, &settings).unwrap_err();
    assert!(error.to_string().contains("box or click prompt"));
}

fn metadata() -> raw_decode::RawMetadata {
    raw_decode::RawMetadata {
        make: "test".into(),
        model: "test".into(),
        lens: None,
        iso: 100.,
        shutter_s: 0.01,
        aperture: 4.,
        focal_mm: 50.,
        capture_time: 0,
        catalog_orientation: None,
        baseline_exposure: 0.,
        orientation: 1,
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
        maker_lens: None,
    }
}

#[test]
fn raw_subject_export_maps_display_mask_back_to_active_sensor_area() {
    struct Left;
    impl MaskSegmenter for Left {
        fn segment(
            &mut self,
            image: &image::RgbImage,
            _: &SegmentRequest,
        ) -> anyhow::Result<Vec<f32>> {
            assert_eq!(image.dimensions(), (22, 26));
            Ok((0..image.width() * image.height())
                .map(|i| {
                    if i % image.width() < image.width() / 2 {
                        1.0
                    } else {
                        0.0
                    }
                })
                .collect())
        }
    }
    let cfa = raw_decode::CfaImage::from_linear(32, 24, vec![0.05; 768]).unwrap();
    let mut metadata = metadata();
    metadata.orientation = 6;
    let input = ExportImage {
        source: RenderSource::Cfa {
            image: &cfa,
            metadata: &metadata,
        },
        name: "raw",
        sequence: 1,
        date: "",
        metadata: None,
    };
    let dir = tempfile::tempdir().unwrap();
    let settings = ExportSettings {
        output_dir: dir.path().into(),
        metadata: Metadata::None,
        ..Default::default()
    };
    let output =
        export_one_with_segmenter(&input, &recipe(vec![subject()]), &settings, &mut Left).unwrap();
    let actual = image::open(output).unwrap().to_rgb8();
    let baseline = export_one(
        &input,
        &Recipe::default(),
        &ExportSettings {
            naming: "baseline".into(),
            ..settings
        },
    )
    .unwrap();
    let baseline = image::open(baseline).unwrap().to_rgb8();
    assert_eq!(actual.dimensions(), (26, 22));
    let delta = |x, y| {
        actual
            .get_pixel(x, y)
            .0
            .into_iter()
            .zip(baseline.get_pixel(x, y).0)
            .map(|(a, b)| a as i32 - b as i32)
            .sum::<i32>()
    };
    assert!(delta(12, 18) > 30);
    assert!(delta(12, 2).abs() < 6);
}

#[test]
fn ai_lens_warp_fails_explicitly_instead_of_exporting_misaligned_masks() {
    let pixels = fixture();
    let input = input(&pixels);
    let mut recipe = recipe(vec![subject()]);
    recipe
        .edit(EditMeta::user("lens", 1), |s| {
            s.lens.manual_distortion = 15.0
        })
        .unwrap();
    let dir = tempfile::tempdir().unwrap();
    let settings = ExportSettings {
        output_dir: dir.path().into(),
        ..Default::default()
    };
    let mut backend = Fake {
        alpha: 1.0,
        fail: false,
        calls: 0,
    };
    let error = export_one_with_segmenter(&input, &recipe, &settings, &mut backend).unwrap_err();
    assert!(error.to_string().contains("lens warps"));
    assert_eq!(backend.calls, 0);
    assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 0);
}

/// LR-8n (REV-LR-8n S1): a catalog-oriented RGB original in its stored frame
/// segments like a RAW. The segmenter sees the displayed orientation and its
/// raster is mapped back to the stored frame the edits live in.
#[test]
fn lr8n_stored_rgb_subject_export_segments_displayed_pixels_and_masks_the_stored_frame() {
    struct Left;
    impl MaskSegmenter for Left {
        fn segment(
            &mut self,
            image: &image::RgbImage,
            _: &SegmentRequest,
        ) -> anyhow::Result<Vec<f32>> {
            assert_eq!(
                image.dimensions(),
                (24, 32),
                "segmentation input is display-oriented (orientation 6)"
            );
            Ok((0..image.width() * image.height())
                .map(|i| {
                    if i % image.width() < image.width() / 2 {
                        1.0
                    } else {
                        0.0
                    }
                })
                .collect())
        }
    }
    // 32 x 24 stored pixels, displayed 24 x 32 (orientation 6).
    let pixels = Image::new(32, 24, vec![vec![0.05; 768]; 3]).unwrap();
    let input = ExportImage {
        source: RenderSource::StoredRgb {
            image: &pixels,
            orientation: 6,
        },
        name: "stored",
        sequence: 1,
        date: "",
        metadata: None,
    };
    let dir = tempfile::tempdir().unwrap();
    // Sensor-oriented output (apply_orientation off) shows the stored frame.
    let settings = ExportSettings {
        output_dir: dir.path().into(),
        metadata: Metadata::None,
        ..Default::default()
    };
    let output =
        export_one_with_segmenter(&input, &recipe(vec![subject()]), &settings, &mut Left).unwrap();
    let actual = image::open(output).unwrap().to_rgb8();
    let baseline = export_one(
        &input,
        &Recipe::default(),
        &ExportSettings {
            naming: "baseline".into(),
            ..settings
        },
    )
    .unwrap();
    let baseline = image::open(baseline).unwrap().to_rgb8();
    assert_eq!(actual.dimensions(), (32, 24), "stored frame");
    let delta = |x, y| {
        actual
            .get_pixel(x, y)
            .0
            .into_iter()
            .zip(baseline.get_pixel(x, y).0)
            .map(|(a, b)| a as i32 - b as i32)
            .sum::<i32>()
    };
    // Displayed left half = stored bottom half under orientation 6.
    assert!(delta(16, 18) > 30, "masked: {}", delta(16, 18));
    assert!(delta(16, 2).abs() < 6, "unmasked: {}", delta(16, 2));
    assert!(
        delta(4, 20) > 30 && delta(28, 20) > 30,
        "mask spans the stored width"
    );
}
