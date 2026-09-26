use vector::*;
#[test]
fn svg_continuation_after_close() {
    let p = Path::from_svg_data("M0 0 L2 0 L0 2 Z L5 5").unwrap();
    assert_eq!(p.subpaths.len(), 2);
    assert!(p.subpaths[0].closed);
    assert!(!p.subpaths[1].closed);
    assert_eq!(p.subpaths[1].anchors[0].point, Point::ZERO);
}
#[test]
fn psd_crate_accepts_records_and_flags_render() {
    let p = Shape::Rectangle {
        rect: Rect::new(0., 0., 0.5, 1.),
        radii: [0.; 4],
    }
    .path()
    .unwrap()
    .with_rule(FillRule::EvenOdd);
    let m = PsdVectorMask::from_path(&p, 0).unwrap();
    let bytes = m.encode();
    let parsed = psd::metadata::parse_vector_mask(&bytes).unwrap();
    assert_eq!(parsed.records.len(), m.records.len());
    assert_eq!(parsed.flags, 0);
    let view = Viewport {
        width: 2,
        height: 1,
        origin: Point::ZERO,
        level: 0,
    };
    let renderer = VectorRenderer::default();
    let mut m = PsdVectorMask::decode(&bytes).unwrap();
    assert_eq!(
        m.coverage(&renderer, Vec2::new(2., 1.), view).unwrap().data,
        [1., 0.]
    );
    m.flags = 1;
    assert_eq!(
        m.coverage(&renderer, Vec2::new(2., 1.), view).unwrap().data,
        [0., 1.]
    );
    m.flags = 4;
    assert_eq!(
        m.coverage(&renderer, Vec2::new(2., 1.), view).unwrap().data,
        [1., 1.]
    );
    let mut r = [7; 26];
    r[..2].copy_from_slice(&99_u16.to_be_bytes());
    m.records.push(PsdPathRecord::read(&r).unwrap());
    assert_eq!(PsdVectorMask::decode(&m.encode()).unwrap(), m);
    assert!(m.path().is_err());
}
#[test]
fn malformed_svg_is_rejected() {
    for svg in [
        "<svg><path d='M0 0'/>",
        "<svg/><path d='M0 0'/>",
        "<path d='M0 0' transform='scale(2)'/>",
    ] {
        assert!(Path::from_svg(svg).is_err(), "{svg}");
    }
}
