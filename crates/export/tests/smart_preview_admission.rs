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
