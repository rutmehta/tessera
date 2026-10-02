use engine_api::{
    EngineError,
    color::ColorMatrix3,
    jobs::CancellationToken,
    recipe::{ProcessVersion, Recipe},
};
use export::{
    ColorSpace, ExportImage, ExportItem, ExportSettings, RenderRequest, Resize, SharpenFor,
};
use pipeline_cpu::{CameraLinearProxy, RenderSource};
use raw_decode::{CfaImage, CfaLayout, RawMetadata};
fn fixture(w: u32, h: u32) -> (CfaImage, RawMetadata) {
    let m = RawMetadata {
        make: "synthetic".into(),
        model: "camera".into(),
        lens: None,
        iso: 100.,
        shutter_s: 0.01,
        aperture: 4.,
        focal_mm: 50.,
        capture_time: 0,
        catalog_orientation: None,
        baseline_exposure: 0.,
        orientation: 6,
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
    let c = CfaImage::from_linear(
        w,
        h,
        (0..w * h)
            .map(|i| 0.1 + (i % w) as f32 / (w as f32) * 0.15 + ((i / w) % 2) as f32 * 0.1)
            .collect(),
    )
    .unwrap();
    (c, m)
}
fn proxy() -> CameraLinearProxy {
    let (c, m) = fixture(8, 8);
    CameraLinearProxy::generate(
        &c,
        &m,
        &Default::default(),
        ProcessVersion::NATIVE_CURRENT,
        [7; 32],
        &Default::default(),
    )
    .unwrap()
}
fn input(p: &CameraLinearProxy) -> ExportImage<'_> {
    ExportImage {
        source: RenderSource::CameraLinear(p),
        name: "proxy",
        sequence: 1,
        date: "",
        metadata: None,
    }
}
fn rejected<T>(result: engine_api::EngineResult<T>) {
    match result {
        Err(EngineError::Unsupported { what }) => {
            assert!(what.contains("original required"), "{what}")
        }
        _ => panic!("expected original-required admission rejection"),
    }
}
#[test]
fn proxy_single_and_staged_export_require_original_without_output() {
    let p = proxy();
    let dir = tempfile::tempdir().unwrap();
    let settings = ExportSettings {
        output_dir: dir.path().join("not-created"),
        ..Default::default()
    };
    let r = Recipe::default();
    let c = CancellationToken::new();
    rejected(export::export_one(&input(&p), &r, &settings));
    rejected(export::export_one_cancellable(
        &input(&p),
        &r,
        &settings,
        &c,
        None,
        None,
    ));
    rejected(export::render_one_cancellable(
        &input(&p),
        &r,
        &settings,
        &c,
        None,
        None,
    ));
    assert!(!settings.output_dir.exists());
    assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 0);
}
#[test]
fn proxy_render_pixels_requires_original_even_when_resize_requests_upscale() {
    let p = proxy();
    rejected(export::render_pixels(
        &input(&p),
        &Recipe::default(),
        &RenderRequest {
            color_space: ColorSpace::Srgb,
            resize: Resize::Percent(400.),
            sharpen_for: SharpenFor::None,
            scale: 1,
        },
        &CancellationToken::new(),
        None,
    ));
}
#[test]
fn mixed_batch_rejects_before_any_original_output_or_progress() {
    let p = proxy();
    let pixels = pipeline_cpu::Image::new(8, 8, vec![vec![0.2; 64]; 3]).unwrap();
    let r = Recipe::default();
    let items = [
        ExportItem {
            image: ExportImage {
                source: RenderSource::Rgb(&pixels),
                name: "original",
                sequence: 0,
                date: "",
                metadata: None,
            },
            recipe: &r,
        },
        ExportItem {
            image: input(&p),
            recipe: &r,
        },
    ];
    let dir = tempfile::tempdir().unwrap();
    let settings = ExportSettings {
        output_dir: dir.path().join("not-created"),
        ..Default::default()
    };
    let count = std::cell::Cell::new(0);
    rejected(export::export_batch(
        &items,
        &settings,
        |_| count.set(count.get() + 1),
        &CancellationToken::new(),
    ));
    rejected(export::export_batch_with_jobs(
        &items,
        &settings,
        |_| count.set(count.get() + 1),
        &CancellationToken::new(),
        1,
    ));
    assert_eq!(count.get(), 0);
    assert!(!settings.output_dir.exists());
}

#[test]
fn lr13_external_proxy_optional_settings_export_with_warning_and_adobe_pixels() {
    let dng = raw_decode::lossy_dng::read(&mut std::io::Cursor::new(include_bytes!(
        "../../raw-decode/tests/fixtures/linear-gradient-jxl.dng"
    )))
    .unwrap()
    .unwrap();
    let proxy = CameraLinearProxy::from_dng(dng)
        .unwrap()
        .with_catalog_orientation(6)
        .unwrap();
    let mut recipe = Recipe {
        process_version: ProcessVersion::adobe(6),
        ..Default::default()
    };
    let settings = serde_json::from_value(serde_json::json!({"tone":{"exposure":0.7,"contrast":20.0},"camera_profile":{"look":{"style":"unavailable","amount":100.0}},"output":{"hdr":true,"hdr_headroom_stops":2.0,"gamut_mapping":"clip"},"effects":{"lens_blur":{}}})).unwrap();
    recipe
        .edit(
            engine_api::recipe::EditMeta::user("Proxy settings", 1),
            |s| *s = settings,
        )
        .unwrap();
    let retained = recipe.clone();
    let dir = tempfile::tempdir().unwrap();
    let settings = ExportSettings {
        output_dir: dir.path().into(),
        metadata: export::Metadata::None,
        ..Default::default()
    };
    let rendered = export::render_one_cancellable(
        &input(&proxy),
        &recipe,
        &settings,
        &CancellationToken::new(),
        None,
        None,
    )
    .unwrap();
    assert!(
        rendered.warnings().iter().any(|w| w.contains("proxy")),
        "proxy quality warning is required"
    );
    let path = rendered.finish(&CancellationToken::new()).unwrap();
    assert!(path.is_file());
    assert_eq!(recipe, retained);
    // The pixel-only API must take the same optional-feature policy.
    let actual = export::render_pixels(
        &input(&proxy),
        &recipe,
        &RenderRequest {
            color_space: ColorSpace::Srgb,
            resize: Resize::None,
            sharpen_for: SharpenFor::None,
            scale: 1,
        },
        &CancellationToken::new(),
        None,
    )
    .unwrap();
    let expected = image_core::pipeline_adobe::render_scaled(
        &recipe.settings,
        &RenderSource::CameraLinear(&proxy),
        1,
    )
    .unwrap();
    assert_eq!(actual.dimensions(), expected.dimensions());
    for (a, b) in actual.as_raw().iter().zip(expected.as_raw()) {
        assert!(
            (a * 255.0 - f32::from(*b)).abs() <= 2.0,
            "Adobe export parity: {a} vs {b}"
        );
    }
}

#[test]
fn lr13_proxy_auto_white_balance_uses_as_shot_without_rewriting_recipe() {
    use engine_api::recipe::{EditMeta, settings::WhiteBalanceMode};
    let dng = raw_decode::lossy_dng::read(&mut std::io::Cursor::new(include_bytes!(
        "../../raw-decode/tests/fixtures/linear-gradient.dng"
    )))
    .unwrap()
    .unwrap();
    let proxy = CameraLinearProxy::from_dng(dng).unwrap();
    for version in [ProcessVersion::NATIVE_CURRENT, ProcessVersion::adobe(6)] {
        let mut recipe = Recipe {
            process_version: version,
            ..Default::default()
        };
        recipe
            .edit(EditMeta::user("White balance", 1), |s| {
                s.white_balance.mode = WhiteBalanceMode::Auto;
            })
            .unwrap();
        let retained = recipe.clone();
        let mut expected = recipe.clone();
        expected
            .edit(EditMeta::user("As shot", 2), |s| {
                s.white_balance.mode = WhiteBalanceMode::AsShot;
            })
            .unwrap();
        let request = RenderRequest {
            color_space: ColorSpace::Srgb,
            resize: Resize::None,
            sharpen_for: SharpenFor::None,
            scale: 1,
        };
        let render = |r: &Recipe| {
            export::render_pixels(&input(&proxy), r, &request, &CancellationToken::new(), None)
                .unwrap()
        };
        assert_eq!(render(&recipe), render(&expected));
        assert_eq!(recipe, retained);
        assert!(
            proxy
                .render_plan(&recipe.settings, false)
                .1
                .contains(&"/white_balance/mode")
        );
    }
}
