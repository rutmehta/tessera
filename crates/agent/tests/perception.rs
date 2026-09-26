use agent::{Agent, Config, PerceptionHints};
use style_profile::{Profile, Questionnaire};

#[test]
fn skin_delta_uses_full_resolution_face_pixels() {
    use agent::metrics::{SkinBand, measure, measure_console};
    use engine_api::{recipe::settings::NormalizedRect, tools::FaceScore};
    let d = tempfile::tempdir().unwrap();
    let path = d.path().join("face.jpg");
    image::RgbImage::from_fn(2050, 64, |x, y| {
        image::Rgb(if (x + y) % 3 == 0 {
            [255, 30, 10]
        } else {
            [40, 90, 180]
        })
    })
    .save(&path)
    .unwrap();
    let mut console = tessera_mcp::Console::open(d.path().join("console")).unwrap();
    let id = console.open_image(&path).unwrap();
    let faces = [FaceScore {
        region: NormalizedRect {
            left: 0.25,
            top: 0.25,
            right: 0.5,
            bottom: 0.75,
        },
        ..Default::default()
    }];
    let band = SkinBand {
        low: [45., 10., 10.],
        high: [55., 20., 20.],
        max_delta_e: 10.,
    };
    let expected = measure(&console.render_final(id).unwrap(), &faces, Some(&band)).unwrap();
    let (actual, _) = measure_console(&console, id, &faces, Some(&band)).unwrap();
    assert_eq!(actual.skin_delta_e, expected.skin_delta_e);
}
#[test]
fn critic_packet_counts_native_pixels_not_vlm_preview() {
    let d = tempfile::tempdir().unwrap();
    let path = d.path().join("sparse.jpg");
    image::RgbImage::from_fn(2050, 12, |x, _| {
        image::Rgb(if x % 4 == 0 { [255, 0, 0] } else { [64; 3] })
    })
    .save(&path)
    .unwrap();
    let mut agent = Agent::open(
        d.path().join("agent"),
        Profile::new("test", Questionnaire::default()).unwrap(),
        Config::default(),
    )
    .unwrap();
    let packet = agent.perceive(&path).unwrap();
    assert_eq!(packet.histogram.red.iter().sum::<u32>(), 2050 * 12);
    let mut console = tessera_mcp::Console::open(d.path().join("reference")).unwrap();
    let id = console.open_image(&path).unwrap();
    let full = console.render_final(id).unwrap();
    let reference = agent::metrics::measure(&full, &[], None).unwrap();
    assert!((reference.highlight_clipping - packet.metrics.highlight_clipping).abs() < 1e-4);
    assert!((reference.shadow_clipping - packet.metrics.shadow_clipping).abs() < 1e-4);
    assert!((reference.mean_luminance - packet.metrics.mean_luminance).abs() < 1e-4);
}
#[test]
fn cached_perception_preserves_identity_caption_and_depth() {
    let d = tempfile::tempdir().unwrap();
    let p = d.path().join("image.jpg");
    image::RgbImage::from_pixel(32, 32, image::Rgb([100, 100, 100]))
        .save(&p)
        .unwrap();
    let mut a = Agent::open(
        d.path().join("app"),
        Profile::new("test", Questionnaire::default()).unwrap(),
        Config::default(),
    )
    .unwrap();
    let id = a.perceive(&p).unwrap().image;
    a.perception_hints.insert(
        id,
        PerceptionHints {
            nearest_caption: Some("outdoor portrait".into()),
            faces: Some(vec![engine_api::tools::FaceScore {
                person: Some(engine_api::id::PersonId(7)),
                ..Default::default()
            }]),
            depth_available: Some(true),
            ..Default::default()
        },
    );
    let packet = a.perceive(&p).unwrap();
    assert_eq!(packet.scene_labels, vec!["outdoor portrait"]);
    assert_eq!(packet.faces[0].person, Some(engine_api::id::PersonId(7)));
    assert_eq!(packet.depth_available, Some(true));
}
