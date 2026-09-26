//! Run with `cargo run -p transform --release --example affine_bench`.
use std::{hint::black_box, time::Instant};
use transform::{Image, Kernel, Operation, TransformOp, free::FreeTransform};
fn main() {
    let (width, height) = (6000, 6000);
    let image = Image::new(
        width,
        height,
        std::array::from_fn(|c| {
            (0..width * height)
                .map(|i| {
                    if c == 3 {
                        1.
                    } else {
                        (i % 1024) as f32 / 1024.
                    }
                })
                .collect()
        }),
    )
    .unwrap();
    let op = TransformOp {
        version: 1,
        kernel: Kernel::Bicubic,
        operation: Operation::Free(FreeTransform::rotate(0.04, [3000., 3000.]).unwrap()),
    };
    black_box(op.apply(&image, width, height, 0).unwrap());
    for run in 1..=3 {
        let start = Instant::now();
        let output = op.apply(&image, width, height, 0).unwrap();
        let elapsed = start.elapsed();
        assert!(output.planes.iter().flatten().all(|v| v.is_finite()));
        black_box(&output);
        println!(
            "36MP rotated bicubic run {run}: {:.3} ms (includes input validation, output allocation and rendering; target <400 ms)",
            elapsed.as_secs_f64() * 1000.
        );
    }
}
