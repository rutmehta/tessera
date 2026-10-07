//! Profile dispatch is explicit in the recipe; native pixels remain independent.
use engine_api::{
    id::ImageId,
    recipe::{DevelopSettings, ProcessVersion},
};
use image_core::{PixelRect, RawImage, Renderer, RendererConfig};
#[allow(dead_code)]
#[path = "../../raw-decode/tests/support/mod.rs"]
mod support;

fn pixels(renderer: &Renderer, image: &RawImage, settings: &DevelopSettings) -> Vec<u8> {
    let extent = Renderer::output_extent(image, settings, 0).unwrap();
    renderer
        .render_region(image, settings, 0, PixelRect::full(extent))
        .unwrap()
        .iter()
        .flat_map(|t| t.samples::<u8>().unwrap().to_vec())
        .collect()
}

fn proxy(bytes: &[u8]) -> RawImage {
    let dng = raw_decode::lossy_dng::read(&mut std::io::Cursor::new(bytes))
        .unwrap()
        .unwrap();
    let proxy = pipeline_cpu::CameraLinearProxy::from_dng(dng)
        .unwrap()
        .with_embedded_profile(Some(bytes.to_vec()));
    RawImage::from_camera_linear_proxy(ImageId(810), ImageId(899), std::sync::Arc::new(proxy))
        .unwrap()
}

#[test]
fn native_adobe_named_recipe_does_not_switch_pipeline() {
    let image = proxy(&support::lossy_dng(false, false));
    let mut settings = DevelopSettings::default();
    let renderer = Renderer::new(Default::default());
    let expected = pixels(&renderer, &image, &settings);
    settings.camera_profile.profile.name = "Adobe Color".into();
    assert_eq!(pixels(&renderer, &image, &settings), expected);
    assert_eq!(renderer.profile_notice(&image, &settings), None);
}

#[test]
fn only_adobe_named_proxy_recipes_use_embedded_profile() {
    let bytes = support::lossy_dng(false, false);
    let image = proxy(&bytes);
    let bare = {
        let dng = raw_decode::lossy_dng::read(&mut std::io::Cursor::new(&bytes))
            .unwrap()
            .unwrap();
        RawImage::from_camera_linear_proxy(
            ImageId(812),
            ImageId(899),
            std::sync::Arc::new(pipeline_cpu::CameraLinearProxy::from_dng(dng).unwrap()),
        )
        .unwrap()
    };
    let renderer = Renderer::new(RendererConfig {
        process_version: ProcessVersion::adobe(6),
        ..Default::default()
    });
    let mut settings = DevelopSettings::default();
    for name in ["", "Camera Standard"] {
        settings.camera_profile.profile.name = name.into();
        assert_eq!(
            pixels(&renderer, &image, &settings),
            pixels(&renderer, &bare, &settings)
        );
    }
    settings.camera_profile.profile.name = "Adobe Color".into();
    assert_eq!(
        renderer.profile_notice(&image, &settings),
        Some(pipeline_adobe::SUBSTITUTED_PROFILE_NOTICE)
    );
    assert_eq!(
        renderer.profile_notice(&bare, &settings),
        Some(pipeline_adobe::UNAVAILABLE_PROFILE_NOTICE)
    );
    assert_ne!(
        pixels(&renderer, &image, &settings),
        pixels(&renderer, &bare, &settings)
    );
}

#[test]
fn malformed_embedded_profile_does_not_fail_a_proxy_render() {
    let bytes = support::lossy_dng(false, false);
    let dng = raw_decode::lossy_dng::read(&mut std::io::Cursor::new(&bytes))
        .unwrap()
        .unwrap();
    let bare = pipeline_cpu::CameraLinearProxy::from_dng(dng).unwrap();
    let invalid = bare
        .clone()
        .with_embedded_profile(Some(b"invalid profile".to_vec()));
    let a =
        RawImage::from_camera_linear_proxy(ImageId(813), ImageId(899), std::sync::Arc::new(bare))
            .unwrap();
    let b = RawImage::from_camera_linear_proxy(
        ImageId(814),
        ImageId(899),
        std::sync::Arc::new(invalid),
    )
    .unwrap();
    let renderer = Renderer::new(RendererConfig {
        process_version: ProcessVersion::adobe(6),
        ..Default::default()
    });
    let mut settings = DevelopSettings::default();
    settings.camera_profile.profile.name = "Adobe Color".into();
    assert_eq!(
        pixels(&renderer, &a, &settings),
        pixels(&renderer, &b, &settings)
    );
    assert_eq!(
        renderer.profile_notice(&b, &settings),
        Some(pipeline_adobe::UNAVAILABLE_PROFILE_NOTICE)
    );
}

fn cfa_dng() -> Vec<u8> {
    let shorts = |v: &[u16]| v.iter().flat_map(|v| v.to_le_bytes()).collect::<Vec<_>>();
    let longs = |v: &[u32]| v.iter().flat_map(|v| v.to_le_bytes()).collect::<Vec<_>>();
    let mut tags = vec![
        (254u16, 4u16, 1u32, longs(&[0])),
        (256, 4, 1, longs(&[64])),
        (257, 4, 1, longs(&[48])),
        (258, 3, 1, shorts(&[16])),
        (259, 3, 1, shorts(&[1])),
        (262, 3, 1, shorts(&[32803])),
        (271, 2, 6, b"NIKON\0".to_vec()),
        (272, 2, 11, b"NIKON D850\0".to_vec()),
        (273, 4, 1, longs(&[0])),
        (274, 3, 1, shorts(&[1])),
        (277, 3, 1, shorts(&[1])),
        (278, 4, 1, longs(&[48])),
        (279, 4, 1, longs(&[64 * 48 * 2])),
        (33421, 3, 2, shorts(&[2, 2])),
        (33422, 1, 4, vec![0, 1, 1, 2]),
        (50706, 1, 4, vec![1, 4, 0, 0]),
        (50707, 1, 4, vec![1, 1, 0, 0]),
        (50708, 2, 15, b"Synthetic Test\0".to_vec()),
        (50710, 1, 3, vec![0, 1, 2]),
        (50711, 3, 1, shorts(&[1])),
        (50714, 5, 1, longs(&[0, 1])),
        (50717, 4, 1, longs(&[65535])),
        (
            50721,
            10,
            9,
            longs(&[1, 1, 0, 1, 0, 1, 0, 1, 1, 1, 0, 1, 0, 1, 0, 1, 1, 1]),
        ),
        (50728, 5, 3, longs(&[1, 2, 1, 1, 2, 3])),
        (50778, 3, 1, shorts(&[21])),
    ];
    // LibRaw's DNG calibration admission expects both illuminants for this
    // invented camera. Both calibrations are synthetic identity matrices.
    let mut second = tags.iter().find(|t| t.0 == 50721).unwrap().clone();
    second.0 = 50722;
    tags.push(second);
    tags.push((50779, 3, 1, shorts(&[17])));
    tags.sort_by_key(|t| t.0);
    let mut out = vec![0; 14 + tags.len() * 12];
    out[..8].copy_from_slice(&[73, 73, 42, 0, 8, 0, 0, 0]);
    out[8..10].copy_from_slice(&(tags.len() as u16).to_le_bytes());
    let mut strip_slot = 0;
    for (i, (tag, kind, count, data)) in tags.into_iter().enumerate() {
        let p = 10 + i * 12;
        out[p..p + 2].copy_from_slice(&tag.to_le_bytes());
        out[p + 2..p + 4].copy_from_slice(&kind.to_le_bytes());
        out[p + 4..p + 8].copy_from_slice(&count.to_le_bytes());
        if tag == 273 {
            strip_slot = p + 8;
        }
        if data.len() <= 4 {
            out[p + 8..p + 8 + data.len()].copy_from_slice(&data);
        } else {
            let offset = out.len() as u32;
            out[p + 8..p + 12].copy_from_slice(&offset.to_le_bytes());
            out.extend(data);
        }
    }
    let offset = out.len() as u32;
    out[strip_slot..strip_slot + 4].copy_from_slice(&offset.to_le_bytes());
    for i in 0..64 * 48 {
        out.extend((4000u16 + (i % 31) as u16 * 500).to_le_bytes());
    }
    out
}

#[test]
fn ordinary_cfa_dng_does_not_use_embedded_fallback() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("original.dng");
    std::fs::write(&path, cfa_dng()).unwrap();
    let image = RawImage::open(ImageId(811), &path).unwrap();
    assert!(image.camera_linear_proxy().is_none());
    let bare = RawImage::new(
        ImageId(812),
        std::sync::Arc::new(
            raw_decode::RawSource::open(&path)
                .unwrap()
                .decode_cfa()
                .unwrap(),
        ),
        std::sync::Arc::new(image.metadata().clone()),
    )
    .unwrap();
    for version in [ProcessVersion::NATIVE_CURRENT, ProcessVersion::adobe(6)] {
        for name in ["", "Adobe Color"] {
            let mut settings = DevelopSettings::default();
            settings.camera_profile.profile.name = name.into();
            let renderer = Renderer::new(RendererConfig {
                process_version: version,
                ..Default::default()
            });
            assert_eq!(
                pixels(&renderer, &image, &settings),
                pixels(&renderer, &bare, &settings)
            );
        }
    }
}

#[test]
fn installed_profile_wins_over_valid_or_malformed_embedded_data() {
    let bytes = support::lossy_dng(false, false);
    let image = proxy(&bytes);
    let invalid = image
        .camera_linear_proxy()
        .unwrap()
        .clone()
        .with_embedded_profile(Some(b"invalid profile".to_vec()));
    let invalid = RawImage::from_camera_linear_proxy(
        ImageId(815),
        ImageId(816),
        std::sync::Arc::new(invalid),
    )
    .unwrap();
    let installed_bytes =
        pipeline_adobe::dcp::read_embedded_profile(&mut std::io::Cursor::new(cfa_dng()))
            .unwrap()
            .unwrap();
    let renderer = Renderer::new(RendererConfig {
        process_version: ProcessVersion::adobe(6),
        ..Default::default()
    })
    .with_dcp_profile(&installed_bytes)
    .unwrap();
    let mut settings = DevelopSettings::default();
    settings.camera_profile.profile.name = "Adobe Color".into();
    settings.white_balance.mode = engine_api::recipe::settings::WhiteBalanceMode::Custom;
    settings.white_balance.temperature = 6504.;
    settings.white_balance.tint = 0.;
    assert_eq!(
        pixels(&renderer, &image, &settings),
        pixels(&renderer, &invalid, &settings)
    );
    assert_eq!(renderer.profile_notice(&image, &settings), None);
    assert_eq!(renderer.profile_notice(&invalid, &settings), None);
}
