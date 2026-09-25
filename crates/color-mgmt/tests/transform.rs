use color_mgmt::*;

#[test]
fn srgb_identity_across_cube_and_intents() {
    let p = Registry::new().builtin(Builtin::Srgb).unwrap();
    for intent in [
        Intent::Perceptual,
        Intent::RelativeColorimetric,
        Intent::Saturation,
        Intent::AbsoluteColorimetric,
    ] {
        let t = Transform::new(
            &p,
            &p,
            TransformOptions {
                intent,
                ..Default::default()
            },
        )
        .unwrap();
        for r in 0..9 {
            for g in 0..9 {
                for b in 0..9 {
                    let rgb = [r as f32 / 8.0, g as f32 / 8.0, b as f32 / 8.0];
                    let result = t.apply(rgb);
                    for c in 0..3 {
                        assert!((rgb[c] - result[c]).abs() < 0.0001, "{rgb:?} -> {result:?}");
                    }
                }
            }
        }
    }
}
