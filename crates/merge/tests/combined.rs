use merge::{
    LinearImage,
    hdr::{BracketFrame, Deghost, Exposure, HdrOptions},
    pano::PanoramaOptions,
};
fn scene(x: f64, y: f64) -> [f32; 3] {
    let v = 0.7
        + 0.3 * (x * 0.17 + y * 0.13).sin()
        + 0.2 * (x * 0.31 - y * 0.23).cos()
        + 0.2 * ((x * 0.11).sin() * 7. + y * 0.19).cos();
    [v as f32, v as f32 * 0.8, v as f32 * 0.7]
}
fn group(offset: f64, scale: f64) -> Vec<BracketFrame> {
    [0.25, 1., 4.]
        .into_iter()
        .map(|ev| {
            let gain = ev * scale;
            BracketFrame {
                image: LinearImage {
                    width: 240,
                    height: 160,
                    pixels: (0..240 * 160)
                        .map(|i| {
                            scene((i % 240) as f64 + offset, (i / 240) as f64)
                                .map(|v| (v * gain as f32).min(1.))
                        })
                        .collect(),
                    color_matrix: [[1., 0., 0.], [0., 1., 0.], [0., 0., 1.]],
                    as_shot_neutral: [1.; 3],
                },
                exposure: Exposure {
                    shutter_s: gain,
                    iso: 100.,
                    aperture: 1.,
                },
            }
        })
        .collect()
}
#[test]
fn hdr_panorama_normalizes_bracket_reference_exposures_and_writes_dng() {
    let groups = vec![group(0., 1.), group(85., 0.8), group(170., 1.2)];
    let out = merge::hdr_panorama(
        &groups,
        &HdrOptions {
            reference: 1,
            auto_align: false,
            deghost: Deghost::None,
            ..Default::default()
        },
        &PanoramaOptions::default(),
    )
    .unwrap();
    assert_eq!(out.bracket_masks.len(), 3);
    assert!(out.panorama.image.width > 350);
    let im = &out.panorama.image;
    let mut mse = 0.;
    let mut n = 0;
    for y in 15..im.height - 15 {
        for x in 15..im.width - 15 {
            let xy = [
                x as f64 + out.panorama.origin[0],
                y as f64 + out.panorama.origin[1],
            ];
            if xy[0] > 100. && xy[0] < 300. {
                let expected = scene(xy[0], xy[1]);
                for (a, b) in im.pixels[y * im.width + x].iter().zip(expected) {
                    mse += (*a as f64 - b as f64).powi(2);
                    n += 1;
                }
            }
        }
    }
    let psnr = -10. * (mse / n as f64).log10();
    eprintln!("HDR panorama overlap PSNR {psnr}");
    assert!(psnr > 30.);
    let mut bytes = Vec::new();
    merge::write_dng(&mut bytes, im, &out.panorama.recipe).unwrap();
    let decoded = raw_decode::linear_dng::read(&mut std::io::Cursor::new(bytes)).unwrap();
    assert_eq!((decoded.width, decoded.height), (im.width, im.height));
    assert_eq!(decoded.color_matrix, im.color_matrix);
    assert!(decoded.xmp.contains("<ts:Recipe>"));
    assert!(merge::hdr_panorama(&[], &HdrOptions::default(), &PanoramaOptions::default()).is_err());
}
