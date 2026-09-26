use lyon_path::{Path, math::point};
use typography::*;
#[test]
fn editable_path_roundtrips_and_is_used_by_render() {
    let mut text = TextModel::point("abc", "Noto Sans", 20.0);
    text.path = Some(PathText {
        commands: vec![
            PathCommand::Move([10.0, 50.0]),
            PathCommand::Line([200.0, 50.0]),
        ],
        offset: 5.0,
    });
    let json = text.to_engine_data().unwrap();
    assert_eq!(TextModel::from_engine_data(&json).unwrap(), text);
    let rendered = renderer().render(&text, 1.0).unwrap();
    assert_eq!(rendered.layout.glyphs[0].x, 15.0);
    assert!((rendered.layout.glyphs[0].y - 50.0).abs() < 0.0001);
    text.path.as_mut().unwrap().commands = vec![PathCommand::Line([0.0, 0.0])];
    assert!(text.to_engine_data().is_err());
}
fn renderer() -> TextRenderer {
    let mut r = TextRenderer::new();
    r.fonts_mut()
        .load_font_data(include_bytes!("fonts/NotoSans-Regular.ttf").to_vec());
    r
}
#[test]
fn warp_identity_and_nonzero_mesh_deformation() {
    let r = renderer();
    let mut text = TextModel::point("Typography", "Noto Sans", 30.0);
    let plain = r.render(&text, 1.5).unwrap();
    for kind in [WarpKind::Arc, WarpKind::Flag, WarpKind::Wave] {
        text.warp = Warp { kind, amount: 0.0 };
        let identity = r.render(&text, 1.5).unwrap();
        assert_eq!(plain.raster, identity.raster);
        assert_eq!(plain.bounds, identity.bounds);
        text.warp.amount = 0.3;
        assert_ne!(plain.raster, r.render(&text, 1.5).unwrap().raster);
    }
}
#[test]
fn circular_path_has_expected_origins_and_tangents() {
    let r = renderer();
    let text = TextModel::point("abcde", "Noto Sans", 20.0);
    let flat = r.layout(&text).unwrap();
    let mut b = Path::builder();
    // A finely sampled circle is a lyon path without approximation surprises.
    let radius = 100.0;
    b.begin(point(radius, 0.0));
    for i in 1..=4096 {
        let a = i as f32 * std::f32::consts::TAU / 4096.0;
        b.line_to(point(radius * a.cos(), radius * a.sin()));
    }
    b.end(true);
    let path = TextPath::new(&b.build(), 0.01).unwrap();
    let curved = r.layout_on_path(&text, &path, 0.0).unwrap();
    for (a, g) in flat.glyphs.iter().zip(&curved.glyphs) {
        let theta = a.x / radius;
        assert!((g.x - radius * theta.cos()).abs() < 0.01);
        assert!((g.y - radius * theta.sin()).abs() < 0.01);
        assert!((g.angle - (theta + std::f32::consts::FRAC_PI_2)).abs() < 0.002);
    }
    assert!(r.render_layout(&text, curved, 2.0).unwrap().raster.height > 0);
}
