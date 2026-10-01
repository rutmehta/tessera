use engine_api::recipe::{
    settings::{GuideLine, UprightMode},
    EditMeta, Recipe,
};
fn saved() -> Recipe {
    let mut r = Recipe::default();
    r.edit(EditMeta::user("seed",0),|s| {
        s.geometry.upright = serde_json::from_value(serde_json::json!({"mode":"auto","homography_mode":"auto","homography":[[1.,0.,0.],[0.,1.,0.],[0.,0.,1.]]})).unwrap();
    }).unwrap();
    r
}
#[test]
fn mode_changes_invalidate_saved_matrix() {
    for mode in [
        UprightMode::Vertical,
        UprightMode::Level,
        UprightMode::Off,
        UprightMode::Guided,
    ] {
        let mut r = saved();
        r.edit(EditMeta::user("mode", 0), |s| {
            s.geometry.upright.mode = mode
        })
        .unwrap();
        assert!(r.settings.geometry.upright.homography.is_none());
    }
}
#[test]
fn guide_changes_invalidate_saved_matrix() {
    let mut r = saved();
    r.edit(EditMeta::user("guides", 0), |s| {
        s.geometry.upright.guides.push(GuideLine {
            start: [0.1, 0.1],
            end: [0.2, 0.9],
        })
    })
    .unwrap();
    assert!(r.settings.geometry.upright.homography.is_none());
}
#[test]
fn invalid_matrix_fails_recipe_validation() {
    let mut r = saved();
    r.edit(EditMeta::user("invalid", 0), |s| {
        s.geometry.upright.homography = Some([[0.; 3]; 3])
    })
    .unwrap();
    assert!(r.validate().is_err());
}
