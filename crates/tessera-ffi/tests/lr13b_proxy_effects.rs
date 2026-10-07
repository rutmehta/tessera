//! Actual retouch kernels and synthetic depth; no inference or private pixels.
use engine_api::{
    id::{ImageId, RetouchId},
    jobs::CancellationToken,
    recipe::{
        DevelopSettings, EditMeta, MaskComponent, MaskKind, ProcessVersion, Recipe,
        mask::{BrushStroke, RetouchKind, RetouchOperation, RetouchTarget},
        settings::{LensBlur, LensBlurDepth},
    },
};
use image_core::{RawImage, Renderer, depth::DepthProvider, ml_depth::DepthMap};
use pipeline_cpu::{CameraLinearProxy, RenderSource};
use std::sync::Arc;

fn proxy() -> CameraLinearProxy {
    let mut dng = raw_decode::lossy_dng::read(&mut std::io::Cursor::new(include_bytes!(
        "../../raw-decode/tests/fixtures/linear-gradient-jxl.dng"
    )))
    .unwrap()
    .unwrap();
    dng.width = 64;
    dng.height = 48;
    dng.metadata.width = 64;
    dng.metadata.height = 48;
    dng.metadata.default_crop = [0, 0, 64, 48];
    dng.metadata.orientation = 1;
    dng.pixels = (0..64 * 48)
        .map(|i| {
            [if i % 64 < 32 {
                0.06
            } else {
                0.20 + (i % 2) as f32 * 0.12
            }; 3]
        })
        .collect();
    CameraLinearProxy::from_dng(dng).unwrap()
}
fn raw(proxy: &CameraLinearProxy) -> RawImage {
    RawImage::from_camera_linear_proxy(ImageId(1300), ImageId(1301), Arc::new(proxy.clone()))
        .unwrap()
}
fn depth() -> DepthMap {
    DepthMap::from_normalized_inverse(64, 48, vec![0.; 64 * 48]).unwrap()
}
fn settings(blur: bool, retouch: bool) -> DevelopSettings {
    let mut settings = DevelopSettings::default();
    settings.detail.sharpening.amount = 0.;
    settings.detail.noise_reduction.color = 0.;
    if blur {
        settings.effects.lens_blur = Some(LensBlur {
            depth: Some(LensBlurDepth {
                mask_key: Some([92; 32]),
                ..Default::default()
            }),
            ..Default::default()
        });
    }
    if retouch {
        settings.locals.retouch.push(RetouchOperation {
            id: RetouchId(1),
            kind: RetouchKind::Clone {
                source_offset: [0.5, 0.],
            },
            target: RetouchTarget::Area {
                components: vec![MaskComponent::new(MaskKind::Brush {
                    strokes: vec![BrushStroke {
                        points: vec![[0.25, 0.5, 1.]],
                        radius: 0.10,
                        feather: 0.,
                        ..Default::default()
                    }],
                })],
            },
            opacity: 100.,
            feather: 0.,
            enabled: true,
        });
    }
    settings
}
fn shown(renderer: &Renderer, image: &RawImage, settings: &DevelopSettings) -> Vec<u8> {
    renderer
        .render_region(
            image,
            settings,
            0,
            image_core::PixelRect::full(image.active_extent()),
        )
        .unwrap()[0]
        .samples::<u8>()
        .unwrap()
        .to_vec()
}

#[test]
fn proxy_develop_uses_real_depth_and_retouch_in_both_processes() {
    let image = raw(&proxy());
    for process in [ProcessVersion::NATIVE_CURRENT, ProcessVersion::adobe(6)] {
        let renderer = Renderer::new(Default::default())
            .for_process_version(process)
            .with_depth(Arc::new(DepthProvider::from_map(depth())))
            .with_retouch_renderer(Arc::new(brush::render_retouch));
        let base = shown(&renderer, &image, &settings(false, false));
        for (blur, retouch) in [(true, false), (false, true), (true, true)] {
            let edited = settings(blur, retouch);
            let retained = edited.clone();
            assert_ne!(
                shown(&renderer, &image, &edited),
                base,
                "available proxy dependencies must render: blur={blur}, retouch={retouch}"
            );
            assert_eq!(edited, retained);
        }
    }
}

#[test]
fn proxy_depth_with_wrong_extent_is_an_error_not_a_silent_omission() {
    let image = raw(&proxy());
    let renderer = Renderer::new(Default::default()).with_depth(Arc::new(DepthProvider::from_map(
        DepthMap::from_normalized_inverse(2, 2, vec![0.; 4]).unwrap(),
    )));
    assert!(
        renderer
            .render_region(
                &image,
                &settings(true, false),
                0,
                image_core::PixelRect::full(image.active_extent())
            )
            .is_err()
    );
}

#[test]
fn proxy_print_and_file_use_imported_depth_and_registered_retouch() {
    let proxy = proxy();
    let source = export::ExportImage {
        source: RenderSource::CameraLinear(&proxy),
        name: "synthetic",
        sequence: 1,
        date: "",
        metadata: None,
    };
    let root = tempfile::tempdir().unwrap();
    let store =
        image_core::ml_depth::DepthStore::new(root.path().join("previews/depth-cache"), 0).unwrap();
    depth().store_pinned(&store, &[92; 32]).unwrap();
    let request = export::RenderRequest {
        color_space: export::ColorSpace::Srgb,
        resize: export::Resize::None,
        sharpen_for: export::SharpenFor::None,
        scale: 1,
    };
    for process in [ProcessVersion::NATIVE_CURRENT, ProcessVersion::adobe(6)] {
        let recipe = |blur, retouch| {
            let mut r = Recipe {
                process_version: process,
                ..Default::default()
            };
            r.edit(EditMeta::user("Proxy effects", 1), |s| {
                *s = settings(blur, retouch)
            })
            .unwrap();
            r
        };
        let render = |r: &Recipe| {
            export::render_pixels_with_resources(
                &source,
                r,
                &request,
                &CancellationToken::new(),
                None,
                Some(root.path()),
                Some(Arc::new(brush::render_retouch)),
            )
            .unwrap()
        };
        let base = render(&recipe(false, false));
        for (blur, retouch) in [(true, false), (false, true), (true, true)] {
            let recipe = recipe(blur, retouch);
            let printed = render(&recipe);
            assert_ne!(printed, base, "print dropped available proxy effects");
            let out = tempfile::tempdir().unwrap();
            let file = export::export_one(
                &source,
                &recipe,
                &export::ExportSettings {
                    format: export::Format::Png,
                    output_dir: out.path().into(),
                    mask_support: Some(root.path().into()),
                    retouch: Some(Arc::new(brush::render_retouch)),
                    ..Default::default()
                },
            )
            .unwrap();
            let file = image::open(file).unwrap().to_rgb8();
            assert_eq!(file.dimensions(), printed.dimensions());
            for (a, b) in file.as_raw().iter().zip(printed.as_raw()) {
                assert!(
                    (f32::from(*a) - b * 255.).abs() <= 2.,
                    "print/file proxy effects disagree"
                );
            }
        }
    }
}
