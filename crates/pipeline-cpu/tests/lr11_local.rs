use engine_api::recipe::mask::LocalParams;
use pipeline_cpu::{Image, adjust_local};
#[test]
fn local_curve_and_overlay_change_pixels() {
    let input = Image::new(2, 1, vec![vec![0.2; 2], vec![0.3; 2], vec![0.4; 2]]).unwrap();
    for json in [
        r#"{"curves":{"rgb":[{"x":0,"y":0},{"x":1,"y":0.5}]}}"#,
        r#"{"color_overlay":[120,50]}"#,
    ] {
        let p: LocalParams = serde_json::from_str(json).unwrap();
        let out = adjust_local(&input, &p, 100.).unwrap();
        assert_ne!(input.planes(), out.planes(), "{json}");
        assert_eq!(
            input.planes(),
            adjust_local(&input, &p, 0.).unwrap().planes()
        );
    }
}

#[test]
fn curve_amount_scales_delta_without_invalidating_monotone_knots() {
    let input = Image::new(1, 1, vec![vec![0.2]; 3]).unwrap();
    let p: LocalParams = serde_json::from_str(
        r#"{"curves":{"rgb":[{"x":0,"y":0.1},{"x":0.2,"y":0.1},{"x":1,"y":0.9}]}}"#,
    )
    .unwrap();
    let full = adjust_local(&input, &p, 100.).unwrap();
    for amount in [50., 200.] {
        let out = adjust_local(&input, &p, amount).unwrap();
        for c in 0..3 {
            assert!(
                (out.planes()[c][0] - (0.2 + amount / 100. * (full.planes()[c][0] - 0.2))).abs()
                    < 1e-7
            );
        }
    }
}

#[test]
fn absent_extensions_preserve_default_local_serialization() {
    let p: LocalParams = serde_json::from_str("{}").unwrap();
    let v = serde_json::to_value(p).unwrap();
    for field in ["curves", "curves_extended", "point_colors"] {
        assert!(v.get(field).is_none());
    }
}
