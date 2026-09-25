use ml_enhance::{SpatialContract, Tiling, run_tiled};
use ml_runtime::{Session, SessionOptions, Tensor};

#[test]
fn local_runtime_model_has_no_tiling_seams() -> anyhow::Result<()> {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../ml-runtime/tests/data/conv-dynamic.onnx");
    let mut session = Session::load(path, SessionOptions::cpu())?;
    let input = Tensor::new(
        3,
        71,
        83,
        (0..3 * 71 * 83).map(|i| (i % 97) as f32 / 97.0).collect(),
    )?;
    let whole = session.run(&input)?;
    let tiled = run_tiled(
        &input,
        SpatialContract {
            scale: 1,
            radius: 1,
            alignment: 1,
        },
        Tiling {
            tile_size: 16,
            halo: 1,
        },
        |patch| session.run(patch),
    )?;
    assert_eq!(whole.shape(), tiled.shape());
    assert!(
        whole
            .data()
            .iter()
            .zip(tiled.data())
            .all(|(a, b)| (a - b).abs() < 1e-4)
    );
    Ok(())
}

#[test]
fn scaling_crop_preserves_edge_and_odd_dimensions() -> anyhow::Result<()> {
    // Nearest-neighbour is a coordinate oracle, NOT an SR quality/model test.
    let input = Tensor::new(
        3,
        7,
        9,
        (0..3 * 7 * 9)
            .map(|i| if i % 9 < 4 { 0.0 } else { 1.0 })
            .collect(),
    )?;
    for factor in [2, 4] {
        let output = run_tiled(
            &input,
            SpatialContract {
                scale: factor,
                radius: 0,
                alignment: 2,
            },
            Tiling {
                tile_size: 4,
                halo: 2,
            },
            |patch| {
                let [_, _, h, w] = patch.shape();
                let mut data = Vec::new();
                for c in 0..3 {
                    for y in 0..h * factor {
                        for x in 0..w * factor {
                            data.push(patch.data()[c * h * w + (y / factor) * w + x / factor]);
                        }
                    }
                }
                Tensor::new(3, h * factor, w * factor, data)
            },
        )?;
        assert_eq!(output.shape(), [1, 3, 7 * factor, 9 * factor]);
        for c in 0..3 {
            for y in 0..7 * factor {
                for x in 0..9 * factor {
                    assert_eq!(
                        output.data()[c * 7 * 9 * factor * factor + y * 9 * factor + x],
                        if x < 4 * factor { 0.0 } else { 1.0 }
                    );
                }
            }
        }
    }
    Ok(())
}

#[test]
fn refuses_insufficient_halo_and_bad_alignment() {
    let input = Tensor::new(3, 3, 3, vec![0.0; 27]).unwrap();
    for tiling in [
        Tiling {
            tile_size: 0,
            halo: 2,
        },
        Tiling {
            tile_size: 4,
            halo: 0,
        },
        Tiling {
            tile_size: 3,
            halo: 2,
        },
    ] {
        assert!(
            run_tiled(
                &input,
                SpatialContract {
                    scale: 1,
                    radius: 1,
                    alignment: 2
                },
                tiling,
                |_| panic!("invalid plan must fail before inference")
            )
            .is_err()
        );
    }
}
