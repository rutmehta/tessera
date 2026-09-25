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
    let reference = export_one(
        &input,
        &Recipe::default(),
        &ExportSettings {
            naming: "reference".into(),
            ..settings
        },
    )
    .unwrap();
    assert_eq!(
        std::fs::read(path).unwrap(),
        std::fs::read(reference).unwrap()
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
