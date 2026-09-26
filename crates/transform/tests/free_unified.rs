use transform::{
    Image, Kernel, Operation, TransformOp,
    free::FreeTransform,
    perspective::PerspectiveWarp,
    puppet::{PuppetDensity, PuppetWarp},
    seam::ContentAwareScale,
    warp::WarpMesh,
};
fn op(operation: Operation) -> TransformOp {
    TransformOp {
        version: 1,
        operation,
        kernel: Kernel::Nearest,
    }
}
fn image() -> Image {
    Image::new(2, 2, std::array::from_fn(|_| vec![0.1, 0.2, 0.3, 0.4])).unwrap()
}
#[test]
fn unified_geometry_dispatch_roundtrips_real_types() {
    let quad = [[0., 0.], [2., 0.], [2., 2.], [0., 2.]];
    let operations = [
        Operation::Warp(WarpMesh::identity(2., 2.)),
        Operation::Perspective(PerspectiveWarp::from_quads(vec![quad], vec![quad]).unwrap()),
        Operation::Puppet(
            PuppetWarp::from_alpha(&[255; 4], 2, 2, PuppetDensity::Dense, 0).unwrap(),
        ),
    ];
    for operation in operations {
        let t = op(operation);
        assert_eq!(t.apply(&image(), 2, 2, 0).unwrap(), image());
        assert_eq!(
            serde_json::from_str::<TransformOp>(&serde_json::to_string(&t).unwrap()).unwrap(),
            t
        );
        let field = t.displacement(3, 3, 0).unwrap();
        assert_eq!(field[0], [0.5, 0.5]);
        assert_eq!(field[8], [-1e20; 2]);
    }
}
#[test]
fn translation_respects_level_and_fixed_canvas() {
    let t = op(Operation::Free(FreeTransform::translate(2., 0.).unwrap()));
    let out = t.apply(&image(), 2, 2, 1).unwrap();
    assert_eq!(out.planes[0], vec![0., 0.1, 0., 0.3]);
    assert_eq!(
        t.displacement(2, 1, 1).unwrap(),
        vec![[-0.5, 0.5], [0.5, 0.5]]
    );
}
#[test]
fn content_aware_dispatch_checks_canvas_and_no_geometry_field() {
    let t = op(Operation::ContentAwareScale(ContentAwareScale {
        target_width: 1,
        target_height: 2,
        amount: 1.,
        protect: None,
    }));
    let result = t.apply(&image(), 1, 2, 0).unwrap();
    assert_eq!((result.width, result.height), (1, 2));
    assert!(t.displacement(1, 2, 0).is_err());
    assert!(t.apply(&image(), 2, 2, 0).is_err());
    let mut invalid = t.clone();
    invalid.version = 2;
    assert!(invalid.validate().is_err());
}
#[test]
fn rejects_bad_public_images_and_canvases_without_panicking() {
    let t = op(Operation::Free(FreeTransform::identity()));
    let mut input = image();
    input.planes[0].clear();
    assert!(t.apply(&input, 2, 2, 0).is_err());
    assert!(t.apply(&image(), usize::MAX, 2, 0).is_err());
    assert!(t.displacement(0, 2, 0).is_err());
    let pole = op(Operation::Free(FreeTransform {
        matrix: [[1., 0., 0.], [0., 1., 0.], [1., 0., -1.]],
    }));
    assert!(pole.apply(&image(), 2, 2, 0).is_err());
}
#[test]
fn homography_render_uses_inverse_not_forward() {
    let t = op(Operation::Free(FreeTransform {
        matrix: [[1., 0., 0.], [0., 1., 0.], [0.1, 0., 1.]],
    }));
    let field = t.displacement(2, 2, 0).unwrap();
    assert!((field[0][0] - 0.5 / 0.95).abs() < 1e-6);
    let actual = t.apply(&image(), 2, 2, 0).unwrap();
    for (i, p) in field.into_iter().enumerate() {
        assert_eq!(
            actual.planes[0][i],
            transform::sample::sample(&image(), p, Kernel::Nearest)[0]
        );
    }
}
