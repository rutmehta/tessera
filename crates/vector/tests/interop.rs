use vector::*;
#[test]
fn psd_records_roundtrip_and_topology() {
    let mut p = Path::curvature_pen(
        &[
            Point::new(-0.25, 0.25),
            Point::new(0.5, 0.75),
            Point::new(1., 0.),
        ],
        true,
    )
    .unwrap();
    p.fill_rule = FillRule::EvenOdd;
    let mask = PsdVectorMask::from_path(&p, 5).unwrap();
    let bytes = mask.encode();
    assert_eq!((bytes.len() - 8) % 26, 0);
    let decoded = PsdVectorMask::decode(&bytes).unwrap();
    assert_eq!(decoded.encode(), bytes);
    assert_eq!(decoded.flags, 5);
    let q = decoded.path().unwrap();
    assert_eq!(q.subpaths[0].anchors.len(), 3);
    for (a, b) in p.subpaths[0].anchors.iter().zip(&q.subpaths[0].anchors) {
        assert!(a.point.distance(b.point) < 1e-7);
        assert!(a.incoming.distance(b.incoming) < 1e-7);
    }
    assert!(PsdVectorMask::decode(&bytes[..bytes.len() - 1]).is_err());
    let mut broken = decoded.clone();
    broken.records.remove(2);
    assert!(broken.path().is_err());
}
#[test]
fn svg_roundtrip() {
    let p = Path::from_svg_data("M 0 0 C 1 2 3 4 5 6 L 10 0 Z M 20 20 l 2 3").unwrap();
    let q = Path::from_svg_data(&p.to_svg_data()).unwrap();
    assert_eq!(q, p);
    assert!(Path::from_svg_data("not a path").is_err());
    let svg = p.to_svg();
    let restored = Path::from_svg(&svg).unwrap();
    assert_eq!(restored, p);
}
