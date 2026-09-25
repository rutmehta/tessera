use color_mgmt::*;
#[test]
fn saturated_prophoto_red_warns_but_neutral_does_not() {
    let mut r = Registry::new();
    let working = r.builtin(Builtin::ProPhoto).unwrap();
    let display = r.builtin(Builtin::Srgb).unwrap();
    let t = Transform::new(&working, &display, Default::default()).unwrap();
    assert!(t.gamut_warning([1., 0., 0.]).monitor);
    assert!(!t.gamut_warning([0.5; 3]).monitor);
    assert!(!t.gamut_warning([1., 0., 0.]).proof);
}
