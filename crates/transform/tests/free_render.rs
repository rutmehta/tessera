use transform::{Image, Kernel, Operation, TransformOp, free::FreeTransform};

#[test]
fn identity_render_and_level_centers() {
    let input = Image::new(2, 2, std::array::from_fn(|_| vec![0.2, 0.3, 0.4, 0.5])).unwrap();
    for kernel in [
        Kernel::Nearest,
        Kernel::Bilinear,
        Kernel::Bicubic,
        Kernel::Lanczos3,
        Kernel::Automatic,
    ] {
        let op = TransformOp {
            version: 1,
            operation: Operation::Free(FreeTransform {
                matrix: [[1., 0., 0.], [0., 1., 0.], [0., 0., 1.]],
            }),
            kernel,
        };
        let result = op.apply(&input, 2, 2, 2).unwrap();
        for c in 0..4 {
            for i in 0..4 {
                assert!((result.planes[c][i] - input.planes[c][i]).abs() < 1e-6);
            }
        }
        assert_eq!(
            op.displacement(2, 2, 2).unwrap(),
            vec![[0.5, 0.5], [1.5, 0.5], [0.5, 1.5], [1.5, 1.5]]
        );
        assert_eq!(
            serde_json::from_str::<TransformOp>(&serde_json::to_string(&op).unwrap()).unwrap(),
            op
        );
    }
}
