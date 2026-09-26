use compositor::{
    geom::Rect,
    raster::{Depth, Raster},
};
use engine_api::tile::Extent;
use ml_filters::{Cancel, NeuralFilter, Params, SkinSmoothing};

#[test]
fn smooths_only_face_skin_and_keeps_texture_alpha_and_edges() -> anyhow::Result<()> {
    let mut src = Raster::new(Extent::new(40, 32), 4, Depth::F32, 0.0);
    src.edit_region(Rect::new(0, 0, 40, 32), 1, |x, y, p| {
        let bump = if (10..20).contains(&x) { 0.04 } else { 0.0 };
        let texture = if (x + y) % 2 == 0 { 0.008 } else { -0.008 };
        *p = if x < 30 {
            [
                0.65 + bump + texture,
                0.4 + bump + texture,
                0.3 + bump + texture,
                0.7,
            ]
        } else {
            [0.1, 0.3, 0.8, 0.7]
        };
    })?;
    let filter = SkinSmoothing;
    let params = Params {
        faces: vec![[2.0, 2.0, 26.0, 28.0]],
        blur: 4.0,
        smoothness: 1.0,
        ..Params::default()
    };
    let out = filter.apply(&src, &params, &Cancel::new())?;
    let again = filter.apply(&src, &params, &Cancel::new())?;
    assert!(!filter.requires_weights());
    assert!((out.pixel(10, 16)[0] - src.pixel(10, 16)[0]).abs() > 0.0001);
    assert!(
        (out.pixel(14, 16)[0] - out.pixel(15, 16)[0]).abs() > 0.01,
        "texture removed"
    );
    for y in 0..32 {
        for x in 0..40 {
            assert_eq!(out.pixel(x, y), again.pixel(x, y));
            assert_eq!(out.pixel(x, y)[3], src.pixel(x, y)[3]);
            if !(2..28).contains(&x) {
                assert_eq!(out.pixel(x, y), src.pixel(x, y));
            }
        }
    }
    let cancel = Cancel::new();
    cancel.cancel();
    assert!(filter.apply(&src, &params, &cancel).is_err());
    let zero = Params {
        smoothness: 0.0,
        ..params.clone()
    };
    assert!(
        filter
            .apply(&src, &zero, &Cancel::new())?
            .shares_all_tiles_with(&src)
    );
    assert!(
        filter
            .apply(
                &src,
                &Params {
                    blur: f32::NAN,
                    ..params
                },
                &Cancel::new()
            )
            .is_err()
    );
    Ok(())
}
