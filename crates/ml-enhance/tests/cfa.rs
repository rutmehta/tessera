use ml_enhance::{CfaNoise, SpatialContract, Tiling, denoise_cfa_with, run_tiled};
use ml_runtime::Tensor;

#[test]
fn all_bayer_rotations_roundtrip_sensor_and_mask() -> anyhow::Result<()> {
    use ml_enhance::BayerPacking;
    let sensor: Vec<f32> = (0..35).map(|v| v as f32).collect();
    for turns in 0..4 {
        let packing = BayerPacking::new(7, 5, turns)?;
        assert_eq!(packing.unpack(&packing.pack(&sensor)?)?, sensor);
    }
    Ok(())
}

#[test]
fn zero_amount_and_unselected_sites_preserve_bits() -> anyhow::Result<()> {
    let input = Tensor::new(4, 2, 2, vec![-0.0; 16])?;
    let noise = CfaNoise {
        shot: [0.003; 4],
        read: [0.0008; 4],
    };
    let out = denoise_cfa_with(&input, noise, 0.0, None, |_| panic!("bypass"))?;
    assert!(
        out.data()
            .iter()
            .all(|v| v.to_bits() == (-0.0f32).to_bits())
    );
    let mut mask = vec![0.0; 16];
    mask[6] = 1.0;
    let out = denoise_cfa_with(&input, noise, 50.0, Some(&mask), |x| {
        assert_eq!(x.shape(), [1, 8, 2, 2]);
        Tensor::new(4, 2, 2, vec![1.0; 16])
    })?;
    for (i, v) in out.data().iter().enumerate() {
        if i == 6 {
            assert_eq!(*v, 0.5);
        } else {
            assert_eq!(v.to_bits(), (-0.0f32).to_bits());
        }
    }
    Ok(())
}

#[test]
fn four_channel_tiling_preserves_samples() -> anyhow::Result<()> {
    let input = Tensor::new(4, 11, 13, (0..4 * 11 * 13).map(|i| i as f32).collect())?;
    let out = run_tiled(
        &input,
        SpatialContract {
            scale: 1,
            radius: 0,
            alignment: 2,
        },
        Tiling {
            tile_size: 4,
            halo: 2,
        },
        |x| Ok(x.clone()),
    )?;
    assert_eq!(input.data(), out.data());
    Ok(())
}

#[test]
fn full_strength_handoff_moves_runtime_output_without_reblending_copy() -> anyhow::Result<()> {
    let input = Tensor::new(4, 2, 2, vec![0.1; 16])?;
    let mut runtime_pointer = std::ptr::null();
    let output = denoise_cfa_with(
        &input,
        CfaNoise {
            shot: [0.003; 4],
            read: [0.0008; 4],
        },
        100.0,
        None,
        |_| {
            let result = Tensor::new(4, 2, 2, vec![0.25; 16])?;
            runtime_pointer = result.data().as_ptr();
            Ok(result)
        },
    )?;
    assert_eq!(
        output.data().as_ptr(),
        runtime_pointer,
        "full strength must retain the runtime output allocation"
    );
    Ok(())
}
