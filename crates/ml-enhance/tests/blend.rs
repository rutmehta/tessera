use ml_enhance::{NoiseModelHint, denoise_with};
use ml_runtime::Tensor;

#[test]
fn mask_amount_blend_and_zero_mask_bypass() -> anyhow::Result<()> {
    let input = Tensor::new(
        3,
        1,
        3,
        vec![-0.0, 1.0, 2.0, -0.0, 1.0, 2.0, -0.0, 1.0, 2.0],
    )?;
    let output = denoise_with(&input, 50.0, None, Some(&[0.0, 0.5, 1.0]), |_| {
        Tensor::new(3, 1, 3, vec![4.0; 9])
    })?;
    for plane in output.data().chunks(3) {
        assert_eq!(plane[0].to_bits(), (-0.0f32).to_bits());
        assert_eq!(plane[1], 1.75);
        assert_eq!(plane[2], 3.0);
    }
    denoise_with(&input, 100.0, None, Some(&[0.0; 3]), |_| {
        panic!("zero mask loaded model")
    })?;
    Ok(())
}

#[test]
fn rejects_invalid_inputs_before_inference_and_invalid_results() -> anyhow::Result<()> {
    let input = Tensor::new(3, 1, 2, vec![0.5; 6])?;
    for amount in [-1.0, 101.0, f32::NAN, f32::INFINITY] {
        assert!(denoise_with(&input, amount, None, None, |_| panic!("invalid amount")).is_err());
    }
    for mask in [
        vec![0.0],
        vec![-1.0, 0.0],
        vec![f32::NAN, 0.0],
        vec![0.0, 2.0],
    ] {
        assert!(denoise_with(&input, 0.0, None, Some(&mask), |_| panic!("invalid mask")).is_err());
    }
    let noise = NoiseModelHint {
        read: [-1.0; 3],
        shot: [0.0; 3],
    };
    assert!(denoise_with(&input, 0.0, Some(noise), None, |_| panic!("invalid noise")).is_err());
    assert!(
        denoise_with(&input, 100.0, None, None, |_| Tensor::new(
            1,
            1,
            2,
            vec![0.0; 2]
        ))
        .is_err()
    );
    assert!(
        denoise_with(&input, 100.0, None, None, |_| Tensor::new(
            3,
            1,
            2,
            vec![f32::NAN; 6]
        ))
        .is_err()
    );
    Ok(())
}
