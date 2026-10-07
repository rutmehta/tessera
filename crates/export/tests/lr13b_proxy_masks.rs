//! Synthetic proxy pixels and imported alpha resources only.
use engine_api::{
    jobs::CancellationToken,
    recipe::{
        EditMeta, LocalAdjustment, LocalParams, MaskComponent, MaskKind, ProcessVersion, Recipe,
    },
};
use export::{ColorSpace, ExportImage, ExportSettings, RenderRequest, Resize, SharpenFor};
use pipeline_cpu::{CameraLinearProxy, RenderSource};

fn proxy() -> CameraLinearProxy {
    let mut dng = raw_decode::lossy_dng::read(&mut std::io::Cursor::new(include_bytes!(
        "../../raw-decode/tests/fixtures/linear-gradient-jxl.dng"
    )))
    .unwrap()
    .unwrap();
    dng.metadata.orientation = 1;
    CameraLinearProxy::from_dng(dng).unwrap()
}
fn input(proxy: &CameraLinearProxy) -> ExportImage<'_> {
    ExportImage {
        source: RenderSource::CameraLinear(proxy),
        name: "synthetic",
        sequence: 1,
        date: "",
        metadata: None,
    }
}
fn request() -> RenderRequest {
    RenderRequest {
        color_space: ColorSpace::Srgb,
        resize: Resize::None,
        sharpen_for: SharpenFor::None,
        scale: 1,
    }
}
fn recipe(version: ProcessVersion, depth: bool) -> Recipe {
    let mut r = Recipe {
        process_version: version,
        ..Default::default()
    };
    let mut component = MaskComponent::new(if depth {
        MaskKind::Depth {
            range: [0., 1.],
            feather: 0.,
            model: None,
        }
    } else {
        MaskKind::Subject { model: None }
    });
    component.adobe_ai = Some(engine_api::recipe::mask::AdobeAiMask {
        resource_id: None,
        category: "synthetic".into(),
        mask_key: Some([91; 32]),
        regenerate: false,
    });
    r.edit(EditMeta::user("Imported mask", 1), |s| {
        s.locals.adjustments.push(LocalAdjustment {
            components: vec![component],
            params: LocalParams {
                exposure: 1.,
                ..Default::default()
            },
            ..Default::default()
        });
    })
    .unwrap();
    r
}

#[test]
fn unavailable_proxy_ai_and_depth_masks_fail_print_and_file_export() {
    let proxy = proxy();
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(root.path().join("models/models.toml")).unwrap();
    for version in [ProcessVersion::NATIVE_CURRENT, ProcessVersion::adobe(6)] {
        for depth in [false, true] {
            let r = recipe(version, depth);
            let pixels = export::render_pixels_with_mask_support(
                &input(&proxy),
                &r,
                &request(),
                &CancellationToken::new(),
                None,
                Some(root.path()),
            );
            assert!(
                pixels.is_err(),
                "print silently dropped an unavailable mask"
            );
            let output = root.path().join("output");
            let rendered = export::render_one_cancellable(
                &input(&proxy),
                &r,
                &ExportSettings {
                    output_dir: output.clone(),
                    mask_support: Some(root.path().into()),
                    ..Default::default()
                },
                &CancellationToken::new(),
                None,
                None,
            );
            assert!(
                rendered.is_err(),
                "file export silently dropped an unavailable mask"
            );
            assert!(!output.exists() || std::fs::read_dir(output).unwrap().count() == 0);
        }
    }
}

#[test]
fn stored_proxy_ai_and_depth_masks_change_print_and_export_pixels() {
    let proxy = proxy();
    let root = tempfile::tempdir().unwrap();
    // A malformed model manifest proves that imported resources need no inference.
    std::fs::create_dir_all(root.path().join("models/models.toml")).unwrap();
    let w = proxy.pixels().width();
    let h = proxy.pixels().height();
    let store =
        image_core::ml_depth::DepthStore::new(root.path().join("imported-masks"), 0).unwrap();
    for version in [ProcessVersion::NATIVE_CURRENT, ProcessVersion::adobe(6)] {
        for depth in [false, true] {
            let r = recipe(version, depth);
            let retained = r.clone();
            let mut prints = Vec::new();
            let mut files = Vec::new();
            for alpha in [0., 1.] {
                image_core::ml_depth::DepthMap::from_normalized_inverse(
                    w,
                    h,
                    vec![alpha; (w * h) as usize],
                )
                .unwrap()
                .store_pinned(&store, &[91; 32])
                .unwrap();
                prints.push(
                    export::render_pixels_with_mask_support(
                        &input(&proxy),
                        &r,
                        &request(),
                        &CancellationToken::new(),
                        None,
                        Some(root.path()),
                    )
                    .unwrap(),
                );
                let output = tempfile::tempdir().unwrap();
                let path = export::export_one(
                    &input(&proxy),
                    &r,
                    &ExportSettings {
                        format: export::Format::Png,
                        output_dir: output.path().into(),
                        mask_support: Some(root.path().into()),
                        ..Default::default()
                    },
                )
                .unwrap();
                files.push(image::open(path).unwrap().to_rgb8());
            }
            let mut baseline = r.clone();
            baseline
                .edit(EditMeta::user("No local adjustment", 2), |s| {
                    s.locals.adjustments.clear()
                })
                .unwrap();
            let base = export::render_pixels_with_mask_support(
                &input(&proxy),
                &baseline,
                &request(),
                &CancellationToken::new(),
                None,
                Some(root.path()),
            )
            .unwrap();
            for (actual, expected) in prints[0].as_raw().iter().zip(base.as_raw()) {
                assert!(
                    (actual - expected).abs() < 0.0001,
                    "zero alpha must preserve the selected process: {actual} vs {expected}"
                );
            }
            assert_ne!(prints[0], prints[1], "print must apply the imported alpha");
            assert_ne!(files[0], files[1], "export must apply the imported alpha");
            assert_eq!(r, retained);
        }
    }
}
