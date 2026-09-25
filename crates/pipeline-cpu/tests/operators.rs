use engine_api::{
    recipe::settings::ToneSettings,
    tile::{Extent, Tile, TileCoord, TileLayout},
};
use pipeline_cpu::tone;

#[test]
fn profile_then_cat16_neutralises_grey() {
    use engine_api::{color::ColorMatrix3, recipe::settings::WhiteBalanceSettings};
    use pipeline_cpu::{apply_matrix, camera_to_xyz, white_balance_matrix};
    let cam = ColorMatrix3([[0.7, 0.2, 0.1], [0.1, 0.8, 0.1], [0.05, 0.1, 0.85]]);
    let matrix = camera_to_xyz(cam).unwrap();
    let working = engine_api::color::WorkingSpace::LinearRec2020.to_xyz();
    let mut tile = Tile::from_samples(
        TileCoord::new(0, 0, 0),
        TileLayout {
            extent: Extent::new(1, 1),
            halo: 0,
            channels: 3,
        },
        vec![0.1f32, 0.2, 0.125],
    )
    .unwrap();
    apply_matrix(&mut tile, working.inverse().unwrap() * matrix).unwrap();
    let wb = white_balance_matrix(
        &WhiteBalanceSettings::default(),
        matrix,
        [2.0, 1.0, 1.6, 1.0],
    )
    .unwrap();
    apply_matrix(&mut tile, wb).unwrap();
    let rgb = tile.samples::<f32>().unwrap();
    assert!((rgb[0] - rgb[1]).abs() < 1e-6, "{rgb:?}");
    assert!((rgb[2] - rgb[1]).abs() < 1e-6, "{rgb:?}");
    assert!(camera_to_xyz(ColorMatrix3([[0.0; 3]; 3])).is_err());
}

#[test]
fn exposure_doubles_linear_before_display() {
    let mut tile = Tile::from_samples(
        TileCoord::new(0, 0, 0),
        TileLayout {
            extent: Extent::new(1, 1),
            halo: 0,
            channels: 3,
        },
        vec![0.1f32, 0.2, 0.4],
    )
    .unwrap();
    tone(
        &mut tile,
        &ToneSettings {
            exposure: 1.0,
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(tile.samples::<f32>().unwrap(), &[0.2, 0.4, 0.8]);
}
