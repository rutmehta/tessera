use engine_api::jobs::CancellationToken;
use transform::{
    Error, Image, Kernel, Operation, TransformOp,
    free::FreeTransform,
    seam::{ContentAwareScale, apply_with_cancel, apply_with_skin_protection_and_cancel},
};

fn image() -> Image {
    Image::new(3, 3, std::array::from_fn(|_| vec![0.25; 9])).unwrap()
}

fn seam_params() -> ContentAwareScale {
    ContentAwareScale {
        target_width: 2,
        target_height: 2,
        amount: 1.0,
        protect: None,
    }
}

#[test]
fn seam_rejects_a_precancelled_token_even_for_identity() {
    let token = CancellationToken::new();
    token.cancel();
    let mut identity = seam_params();
    identity.target_width = 3;
    identity.target_height = 3;
    assert!(matches!(
        apply_with_cancel(&image(), &identity, &token),
        Err(Error::Cancelled)
    ));
}

#[test]
fn skin_hook_cancellation_stops_before_the_next_pixel() {
    let token = CancellationToken::new();
    let mut calls = 0;
    let result = apply_with_skin_protection_and_cancel(
        &image(),
        &seam_params(),
        |_, _, _| {
            calls += 1;
            if calls == 2 {
                token.cancel();
            }
            0.0
        },
        &token,
    );
    assert!(matches!(result, Err(Error::Cancelled)));
    assert_eq!(calls, 2);
}

#[test]
fn transform_op_passes_cancellation_into_content_aware_scale() {
    let token = CancellationToken::new();
    token.cancel();
    let op = TransformOp {
        version: 1,
        operation: Operation::ContentAwareScale(seam_params()),
        kernel: Kernel::Automatic,
    };
    assert!(matches!(
        op.apply_with_cancel(&image(), 2, 2, 0, &token),
        Err(Error::Cancelled)
    ));
}

#[test]
fn transform_op_nonseam_path_observes_a_precancelled_token() {
    let token = CancellationToken::new();
    token.cancel();
    let op = TransformOp {
        version: 1,
        operation: Operation::Free(FreeTransform::identity()),
        kernel: Kernel::Automatic,
    };
    assert!(matches!(
        op.apply_with_cancel(&image(), 3, 3, 0, &token),
        Err(Error::Cancelled)
    ));
}
