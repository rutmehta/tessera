//! Preview reference for levels above zero, assembled from public
//! `pipeline_cpu` operators in contract stage order (`StageId`): the
//! pre-geometry WB image is downsampled first, then level-pixel Detail
//! (Tone/Color/Effects are default here), then level-pixel Geometry with the
//! resolved lens correction. This is the documented interactive approximation
//! of `image_core::render` ("M2 operates on requested-level WB"); level 0 is
//! the full-resolution reference itself.
//!
//! Geometry must not be folded into the downsampled prefix: with
//! `LensProfileSource::Auto`, image auto-calibration can resolve a distortion
//! (the CR3 and RAF fixtures do), and warping at full resolution before
//! level-pixel Detail is neither the engine preview nor contract order.
use engine_api::recipe::DevelopSettings;
use pipeline_cpu::{Image, RenderSource, SigmoidSettings, render_linear_scaled};

pub fn linear(source: &RenderSource<'_>, settings: &DevelopSettings, scale: u32) -> Image {
    assert_eq!(settings.tone, Default::default());
    assert_eq!(settings.color, Default::default());
    assert_eq!(settings.effects, Default::default());
    assert_eq!(settings.geometry, Default::default());
    let RenderSource::Cfa { image, metadata } = source else {
        panic!("preview reference models RAW sources only");
    };
    let mut base = settings.clone();
    base.detail.sharpening.amount = 0.0;
    base.detail.noise_reduction.luminance = 0.0;
    base.detail.noise_reduction.color = 0.0;
    // Lens optics before Detail stay in the prefix; the common distortion
    // belongs to the composed Geometry map applied after Detail.
    base.lens.manual_distortion = 0.0;
    base.lens.distortion_scale = 0.0;
    let input = render_linear_scaled(&base, source, scale).unwrap();
    let mut result = input.clone();
    for coord in input.coords() {
        let mut tile = input
            .tile(coord, pipeline_cpu::detail_halo(&settings.detail), 1)
            .unwrap();
        pipeline_cpu::detail(&mut tile, &settings.detail).unwrap();
        result.put(&tile).unwrap();
    }
    pipeline_cpu::resolve_lens_sensor(
        image.pyramid().pixels(),
        metadata,
        settings,
        &Default::default(),
    )
    .unwrap()
    .apply_geometry(&result, settings)
    .unwrap()
}

pub fn display(image: &Image, settings: &DevelopSettings) -> Vec<u8> {
    let mut bytes = vec![0; image.width() as usize * image.height() as usize * 3];
    for coord in image.coords() {
        let tile = pipeline_cpu::display(
            &image.tile(coord, 0, 1).unwrap(),
            SigmoidSettings::default(),
            settings.output.gamut_mapping,
        )
        .unwrap();
        let l = tile.layout();
        let (ox, oy) = coord.pixel_origin(engine_api::tile::TILE_SIZE);
        for y in 0..l.extent.height {
            for x in 0..l.extent.width {
                for c in 0..3 {
                    bytes[(((oy + y) * image.width() + ox + x) * 3 + c) as usize] = tile
                        .samples::<u8>()
                        .unwrap()[l.index(c as u8, x as i32, y as i32).unwrap()];
                }
            }
        }
    }
    bytes
}
