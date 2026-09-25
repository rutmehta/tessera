use engine_api::tile::{Extent, Tile, TileCoord, TileFormat, TileLayout};
use ml_runtime::Tensor;

#[test]
fn planar_tiles_roundtrip_with_halo_and_half_precision() -> anyhow::Result<()> {
    let layout = TileLayout {
        extent: Extent::new(7, 5),
        channels: 3,
        halo: 2,
    };
    let coord = TileCoord::new(0, 2, 1);
    let data: Vec<f32> = (0..layout.len()).map(|i| i as f32 / 100. - 1.).collect();
    let tile = Tile::from_samples(coord, layout, data.clone())?;
    let tensor = Tensor::from_tile(&tile)?;
    assert_eq!(tensor.shape(), [1, 3, 9, 11]);
    assert_eq!(tensor.data(), data);
    let restored = tensor.to_tile(coord, layout, TileFormat::F32Planar)?;
    assert_eq!(restored.samples::<f32>()?, data);
    let half_tile = tensor.to_tile(coord, layout, TileFormat::F16Planar)?;
    let restored = Tensor::from_tile(&half_tile)?;
    for (a, b) in restored.data().iter().zip(&data) {
        assert!((a - b).abs() < 0.002);
    }
    let half = Tensor::from_f16(3, 9, 11, &tensor.to_f16())?;
    assert_eq!(half.data(), restored.data());
    assert!(tensor.to_tile(coord, layout, TileFormat::U8).is_err());
    assert!(Tensor::from_tile(&Tile::zeroed(coord, TileFormat::U8, layout)?).is_err());
    assert!(
        tensor
            .to_tile(
                coord,
                TileLayout { halo: 1, ..layout },
                TileFormat::F32Planar
            )
            .is_err()
    );
    Ok(())
}

#[test]
fn invalid_tensor_shapes_are_errors() {
    assert!(Tensor::new(0, 1, 1, vec![]).is_err());
    assert!(Tensor::new(1, 2, 3, vec![0.; 5]).is_err());
    assert!(Tensor::new(usize::MAX, 2, 3, vec![]).is_err());
}
