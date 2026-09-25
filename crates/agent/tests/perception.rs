use agent::{Agent, Config, PerceptionHints};
use style_profile::{Profile, Questionnaire};
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
