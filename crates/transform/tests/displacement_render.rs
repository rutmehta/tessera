use transform::adaptive::{Adaptive, CameraModel, Projection};
use transform::{Image, Kernel, Operation, TransformOp};

#[test]
fn adaptive_operation_uses_shared_cpu_and_gpu_coordinate_contract() {
    let a = Adaptive::new(
        8,
        8,
        CameraModel::Manual {
            focal_px: 50.,
            center: [4., 4.],
            projection: Projection::Rectilinear,
        },
    );
    let op = TransformOp {
        version: 1,
        operation: Operation::Displacement(a.solve().unwrap()),
        kernel: Kernel::Nearest,
    };
    let decoded: TransformOp = serde_json::from_str(&serde_json::to_string(&op).unwrap()).unwrap();
    assert_eq!(op, decoded);
    for level in [0, 1, 2] {
        let w = 8 >> level;
        let input = Image::new(
            w,
            w,
            std::array::from_fn(|_| (0..w * w).map(|v| v as f32).collect()),
        )
        .unwrap();
        assert_eq!(op.apply(&input, w, w, level).unwrap(), input);
        let field = op.displacement(w, w, level).unwrap();
        for (i, p) in field.iter().enumerate() {
            assert_eq!(*p, [(i % w) as f32 + 0.5, (i / w) as f32 + 0.5]);
        }
    }
}
