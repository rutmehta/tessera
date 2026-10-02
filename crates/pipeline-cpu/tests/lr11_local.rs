use engine_api::recipe::mask::LocalParams;
use pipeline_cpu::{Image, adjust_local};
#[test]
fn local_curve_and_overlay_change_pixels() {
    let input = Image::new(2,1,vec![vec![0.2;2],vec![0.3;2],vec![0.4;2]]).unwrap();
    for json in [
        r#"{"curves":{"rgb":[{"x":0,"y":0},{"x":1,"y":0.5}]}}"#,
        r#"{"color_overlay":[120,50]}"#,
    ] {
        let p: LocalParams = serde_json::from_str(json).unwrap();
        let out = adjust_local(&input,&p,100.).unwrap();
        assert_ne!(input.planes(),out.planes(),"{json}");
        assert_eq!(input.planes(),adjust_local(&input,&p,0.).unwrap().planes());
    }
}
