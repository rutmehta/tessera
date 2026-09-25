use index::FaceRecord;
use style_profile::{Features, PerceptionInput};
#[test]
fn cached_perception_builds_scene_and_face_features() {
    let pixels = vec![[0.05; 3], [0.05; 3], [0.8; 3], [1.; 3]];
    let face = FaceRecord {
        id: 1,
        bbox: [0., 0., 2., 1.],
        landmarks5: [[0.; 2]; 5],
        confidence: 1.,
        embedding: None,
        sharpness: 0.7,
        eyes_open: None,
    };
    let f = Features::from_perception(PerceptionInput {
        embedding_model: "siglip-v1",
        embedding: &[0.5; 64],
        linear_rgb: &pixels,
        width: 2,
        height: 2,
        faces: &[face],
        as_shot_cct: 6000.,
        as_shot_duv: 0.002,
        camera: "camera",
        lens: "lens",
    })
    .unwrap();
    assert_eq!(f.face_count, 1);
    assert!((f.face_fraction - 0.5).abs() < 1e-6);
    assert!((f.face_mean_luminance.unwrap() - 0.05).abs() < 1e-6);
    assert_eq!(f.highlight_clipping, 0.25);
    assert_eq!(f.camera, "camera");
    assert_eq!(f.embedding.len(), 64);
    assert!(f.validate().is_ok());
}
