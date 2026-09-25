use merge::{
    LinearImage,
    hdr::{BracketFrame, Deghost, Exposure, HdrOptions, hdr},
};
#[test]
fn clipping_is_an_interval_not_an_excuse_to_ignore_motion() {
    let image = LinearImage {
        width: 32,
        height: 32,
        pixels: vec![[0.4; 3]; 1024],
        color_matrix: [[1., 0., 0.], [0., 1., 0.], [0., 0., 1.]],
        as_shot_neutral: [1.; 3],
    };
    let frames: Vec<_> = [1., 0.25, 4.]
        .into_iter()
        .map(|gain| {
            let mut im = image.clone();
            for p in &mut im.pixels {
                for v in p {
                    *v = (*v * gain).min(1.);
                }
            }
            BracketFrame {
                image: im,
                exposure: Exposure {
                    shutter_s: gain as f64,
                    iso: 100.,
                    aperture: 1.,
                },
            }
        })
        .collect();
    let opts = HdrOptions {
        auto_align: false,
        deghost: Deghost::High,
        ..Default::default()
    };
    let still = hdr(&frames, &opts).unwrap();
    assert!(still.deghost_mask.iter().all(|v| !*v));
    let mut moving = frames;
    moving[1].image.pixels[400] = [1.; 3];
    let out = hdr(&moving, &opts).unwrap();
    assert!(out.deghost_mask[400]);
    assert_eq!(out.image.pixels[400], [0.4; 3]);
}
