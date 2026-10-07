//! LR-8e3: Native renders only the two default Adobe identities (approximated,
//! with a visible note); every other non-default profile is refused as on
//! main. Adobe-named proxies with Auto white balance render through every
//! public entry point.
mod common;
use engine_api::{
    id::ImageId,
    jobs::CancellationToken,
    recipe::{DevelopSettings, ProcessVersion, settings::WhiteBalanceMode},
    tile::{TILE_SIZE, TileCoord},
};
use image_core::{PixelRect, RawImage, RenderOutput, Renderer, RendererConfig, Viewport};
use std::sync::Arc;
#[allow(dead_code)]
#[path = "../../raw-decode/tests/support/mod.rs"]
mod support;

const REFUSED: [&str; 8] = [
    "Adobe Monochrome",
    "Adobe Vivid",
    "Adobe Landscape",
    "Adobe Portrait",
    "Adobe Neutral",
    "Adobe Standard B&W",
    "Adobe Color Monochrome",
    "Camera Standard",
];

fn original() -> RawImage {
    let m = common::metadata(32, 24, common::RGGB, [0, 0, 32, 24]);
    let raw =
        raw_decode::CfaImage::from_linear(32, 24, common::samples(32, 24, common::RGGB)).unwrap();
    RawImage::new(ImageId(830), Arc::new(raw), Arc::new(m)).unwrap()
}

fn proxy() -> RawImage {
    let bytes = support::lossy_dng(false, false);
    let dng = raw_decode::lossy_dng::read(&mut std::io::Cursor::new(&bytes))
        .unwrap()
        .unwrap();
    let proxy = pipeline_cpu::CameraLinearProxy::from_dng(dng)
        .unwrap()
        .with_embedded_profile(Some(bytes));
    RawImage::from_camera_linear_proxy(ImageId(831), ImageId(899), Arc::new(proxy)).unwrap()
}

fn settings(name: &str) -> DevelopSettings {
    let mut s = DevelopSettings::default();
    s.detail.sharpening.amount = 0.;
    s.detail.noise_reduction.color = 0.;
    s.camera_profile.profile.name = name.into();
    s
}

fn render(renderer: &Renderer, image: &RawImage, s: &DevelopSettings) -> Option<Vec<u8>> {
    let extent = Renderer::output_extent(image, s, 0).unwrap();
    let tiles = renderer
        .render_region(image, s, 0, PixelRect::full(extent))
        .inspect_err(|e| eprintln!("{}: {e}", s.camera_profile.profile.name.0))
        .ok()?;
    Some(common::assemble_u8(extent, &tiles))
}

#[test]
fn native_refuses_profiles_it_cannot_reproduce_as_on_main() {
    for name in REFUSED {
        assert!(
            pipeline_cpu::validate_settings(&settings(name)).is_err(),
            "{name}"
        );
        for image in [original(), proxy()] {
            let renderer = Renderer::new(Default::default());
            assert_eq!(render(&renderer, &image, &settings(name)), None, "{name}");
        }
    }
}

#[test]
fn native_approximates_adobe_standard_and_color_with_a_note() {
    for image in [original(), proxy()] {
        let renderer = Renderer::new(Default::default());
        let plain = render(&renderer, &image, &settings("")).unwrap();
        assert_eq!(renderer.profile_notice(&image, &settings("")), None);
        for name in ["Adobe Standard", "Adobe Color"] {
            assert!(pipeline_cpu::validate_settings(&settings(name)).is_ok());
            // Byte-identical to the default-profile render that main produces.
            assert_eq!(render(&renderer, &image, &settings(name)).unwrap(), plain);
            assert_eq!(
                renderer.profile_notice(&image, &settings(name)),
                Some(pipeline_cpu::NATIVE_APPROXIMATED_PROFILE_NOTICE),
                "{name}"
            );
        }
    }
}

#[test]
fn host_listed_ignored_profiles_render_like_main() {
    // Hosts that keep imported Adobe identities in drawn settings (for Adobe
    // process substitution) list the rest as ignored in Native, as main did.
    for image in [original(), proxy()] {
        let renderer = Renderer::new(Default::default()).with_host_ignored_native_profiles();
        let plain = render(&renderer, &image, &settings("")).unwrap();
        for name in ["Adobe Monochrome", "Adobe Vivid"] {
            assert_eq!(render(&renderer, &image, &settings(name)).unwrap(), plain);
            assert_eq!(renderer.profile_notice(&image, &settings(name)), None);
        }
        // Not a general escape hatch: non-Adobe identities still fail.
        assert_eq!(
            render(&renderer, &image, &settings("Camera Standard")),
            None
        );
    }
}

#[test]
fn adobe_named_proxy_with_auto_white_balance_renders_through_every_entry_point() {
    let image = proxy();
    let renderer = Renderer::new(RendererConfig {
        process_version: ProcessVersion::adobe(6),
        ..Default::default()
    });
    let mut s = settings("Adobe Color");
    s.white_balance.mode = WhiteBalanceMode::Auto;
    let extent = Renderer::output_extent(&image, &s, 0).unwrap();
    let region = renderer
        .render_region(&image, &s, 0, PixelRect::full(extent))
        .unwrap();
    assert_eq!(
        renderer.profile_notice(&image, &s),
        Some(pipeline_adobe::SUBSTITUTED_PROFILE_NOTICE)
    );
    let expected = common::assemble_u8(extent, &region);

    let coords: Vec<TileCoord> = (0..extent.height.div_ceil(TILE_SIZE))
        .flat_map(|y| (0..extent.width.div_ceil(TILE_SIZE)).map(move |x| TileCoord::new(0, x, y)))
        .collect();
    let mut tiles = Vec::new();
    renderer
        .render_tiles(
            &image,
            &s,
            &coords,
            RenderOutput::Display,
            &CancellationToken::new(),
            &mut |t| tiles.push(t),
        )
        .unwrap();
    assert_eq!(common::assemble_u8(extent, &tiles), expected);

    let mut progressive = Vec::new();
    renderer
        .render_progressive(
            &image,
            &s,
            &Viewport::new(PixelRect::full(extent)),
            RenderOutput::Display,
            &CancellationToken::new(),
            &mut |t| progressive.push(t),
        )
        .unwrap();
    let finest: Vec<_> = progressive
        .into_iter()
        .filter(|t| t.coord().level == 0)
        .collect();
    assert_eq!(common::assemble_u8(extent, &finest), expected);
}
