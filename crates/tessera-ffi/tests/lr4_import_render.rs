//! Synthetic Lua -> recipe -> scene-linear CPU pixels; no files/images/catalogs.
use import_lrcat::lua_develop;
use pipeline_cpu::{Image, locals_image, masks::MaskOptions};

fn render(mask: &str, expected: &[f32], depth: Option<&[f32]>) {
    let row = format!(
        r#"s = {{ ProcessVersion = "15.4", MaskGroupBasedCorrections = {{ {{ LocalExposure2012 = 1, CorrectionMasks = {{ {mask} }} }} }} }}"#
    );
    let (r, _) = lua_develop::parse(&row, "15.4").unwrap();
    assert_eq!(r.settings.locals.adjustments.len(), 1);
    let i = Image::new(4, 1, vec![vec![0.25; 4]; 3]).unwrap();
    let out = locals_image(
        &i,
        &r.settings.locals.adjustments,
        MaskOptions {
            depth,
            ..Default::default()
        },
    )
    .unwrap();
    let mut maximum = 0f32;
    for plane in out.planes() {
        for (&got, &want) in plane.iter().zip(expected) {
            maximum = maximum.max((got - want).abs());
            assert!((got - want).abs() <= 1e-6, "got {got}, want {want}");
        }
    }
    eprintln!("LR-4 synthetic 4x1: max channel error {maximum}, tolerance 1e-6");
}
#[test]
fn lr4_lua_gradient_local_exposure() {
    render(
        r#"{ What = "Mask/Gradient", FullX = 0, FullY = 0, ZeroX = 1, ZeroY = 0 }"#,
        &[0.46875, 0.40625, 0.34375, 0.28125],
        None,
    );
}
#[test]
fn lr4_lua_radial_local_exposure() {
    render(
        r#"{ What = "Mask/CircularGradient", Left = 0.25, Right = 0.75, Top = 0, Bottom = 1, Feather = 0 }"#,
        &[0.25, 0.5, 0.5, 0.25],
        None,
    );
}
#[test]
fn lr4_lua_depth_local_exposure() {
    render(
        r#"{ What = "Mask/Range", CorrectionRangeMask = { DepthMin = 0.25, DepthMax = 0.75, DepthFeather = 0 } }"#,
        &[0.25, 0.5, 0.5, 0.25],
        Some(&[0.1, 0.3, 0.7, 0.9]),
    );
}
#[test]
fn lr4_lua_luminance_local_exposure() {
    render(
        r#"{ What = "Mask/Range", CorrectionRangeMask = { LumMin = 0.5, LumMax = 0.6, LumFeather = 0 } }"#,
        &[0.5; 4],
        None,
    );
}
#[test]
fn lr4_lua_nested_disabled_add_subtract_intersect() {
    render(
        r#"{ What = "Mask/Group", Masks = {
        { What = "Mask/Gradient", FullX = 0, FullY = 0, ZeroX = 1, ZeroY = 0, MaskActive = false },
        { What = "Mask/CircularGradient", Left = 0.25, Right = 0.75, Top = 0, Bottom = 1, Feather = 0 },
        { What = "Mask/Gradient", FullX = 0, FullY = 0, ZeroX = 1, ZeroY = 0, MaskBlendMode = 2 },
        { What = "Mask/Group", MaskBlendMode = 1, Masks = {
          { What = "Mask/Gradient", FullX = 1, FullY = 0, ZeroX = 0, ZeroY = 0 }
        } }
    } }"#,
        &[0.25, 0.34765625, 0.28515625, 0.25],
        None,
    );
}
