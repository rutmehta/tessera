use transform::{
    Image, Kernel, Operation, TransformOp, free::FreeTransform, seam::ContentAwareScale,
};
#[test]
fn automatic_chooses_exact_lattice_downsample_and_reconstruction() {
    let mut op = TransformOp {
        version: 1,
        operation: Operation::Free(FreeTransform::identity()),
        kernel: Kernel::Automatic,
    };
    assert_eq!(op.effective_kernel(0), Kernel::Nearest);
    op.operation = Operation::Free(FreeTransform::translate(1., 0.).unwrap());
    assert_eq!(op.effective_kernel(0), Kernel::Nearest);
    assert_eq!(op.effective_kernel(1), Kernel::Bicubic);
    op.operation = Operation::Free(FreeTransform::scale(0.5, 0.5, [0.; 2]).unwrap());
    assert_eq!(op.effective_kernel(0), Kernel::Lanczos3);
    op.operation = Operation::Free(FreeTransform::rotate(0.2, [0.; 2]).unwrap());
    assert_eq!(op.effective_kernel(0), Kernel::Bicubic);
}
#[test]
fn protected_seams_work_on_supplied_mip_grid() {
    let image = Image::new(
        3,
        1,
        [vec![0.1, 0.4, 0.9], vec![0.; 3], vec![0.; 3], vec![1.; 3]],
    )
    .unwrap();
    let op = TransformOp {
        version: 1,
        kernel: Kernel::Bilinear,
        operation: Operation::ContentAwareScale(ContentAwareScale {
            target_width: 4,
            target_height: 2,
            amount: 1.,
            protect: Some(vec![1., 0., 1.]),
        }),
    };
    assert_eq!(op.apply(&image, 2, 1, 1).unwrap().planes[0], vec![0.1, 0.9]);
}
