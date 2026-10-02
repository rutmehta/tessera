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

#[test]
fn lr10_embedded_linear_raw_matches_explicit_profile_and_named_adobe_dispatch() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("synthetic.dng");
    let bytes = support::lossy_dng(false, false);
    std::fs::write(&path, &bytes).unwrap();
    let image = RawImage::open(ImageId(810), &path).unwrap();
    let mut settings = DevelopSettings::default();
    settings.detail.sharpening.amount = 0.;
    settings.detail.noise_reduction.color = 0.;
    let config = RendererConfig {
        process_version: ProcessVersion::adobe(6),
        ..Default::default()
    };
    let explicit = Renderer::new(config.clone())
        .with_dcp_profile(&bytes)
        .unwrap();
    let expected = pixels(&explicit, &image, &settings);
    assert_eq!(pixels(&Renderer::new(config), &image, &settings), expected);
    settings.camera_profile.profile.name = "Adobe Color".into();
    assert_eq!(
        pixels(&Renderer::new(Default::default()), &image, &settings),
        expected
    );
    settings.camera_profile.profile.name.0.clear();
    let native = Renderer::new(Default::default());
    let before = pixels(&native, &image, &settings);
    let supplied = native.with_dcp_profile(&bytes).unwrap();
    assert_eq!(pixels(&supplied, &image, &settings), before);
    assert_ne!(before, expected);
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
fn lr10_cfa_dng_original_uses_its_embedded_profile() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("original.dng");
    let bytes = cfa_dng();
    std::fs::write(&path, &bytes).unwrap();
    let image = RawImage::open(ImageId(811), &path).unwrap();
    assert!(image.camera_linear_proxy().is_none());
    let settings = DevelopSettings::default();
    let config = RendererConfig {
        process_version: ProcessVersion::adobe(6),
        ..Default::default()
    };
    let explicit = Renderer::new(config.clone())
        .with_dcp_profile(&bytes)
        .unwrap();
    let expected = pixels(&explicit, &image, &settings);
    assert_eq!(pixels(&Renderer::new(config), &image, &settings), expected);
    assert_ne!(
        pixels(&Renderer::new(Default::default()), &image, &settings),
        expected
    );
}
