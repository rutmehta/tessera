use engine_api::{
    recipe::settings::GamutMapping,
    tile::{Extent, Tile, TileCoord, TileLayout},
};
use pipeline_cpu::{SigmoidSettings, display, display_float};

#[test]
fn float_display_preserves_sub_byte_detail_and_matches_legacy_quantization() {
    let values: Vec<_> = (0..512).map(|i| 0.1 + i as f32 * 0.0001).collect();
    let tile = Tile::from_samples(
        TileCoord::new(1, 0, 0),
        TileLayout {
            extent: Extent::new(256, 2),
            halo: 0,
            channels: 3,
        },
        values.repeat(3),
    )
    .unwrap();
    for gamut in [GamutMapping::Clip, GamutMapping::Perceptual] {
        let float = display_float(&tile, SigmoidSettings::default(), gamut).unwrap();
        let legacy = display(&tile, SigmoidSettings::default(), gamut).unwrap();
        let samples = float.samples::<f32>().unwrap();
        assert!(samples[..512].windows(2).all(|p| p[1] > p[0]));
        assert!(
            samples
                .iter()
                .all(|v| v.is_finite() && (0.0..=1.0).contains(v))
        );
        let bytes = legacy.samples::<u8>().unwrap();

        for (i, &v) in samples.iter().enumerate() {
            let bayer = [[0_u8, 8, 2, 10], [12, 4, 14, 6]];
            let noise = (f32::from(bayer[(i % 512) / 256][i % 4]) + 0.5) / 16.0 - 0.5;
            assert_eq!(
                bytes[i],
                (v * 255.0 + noise).round().clamp(0.0, 255.0) as u8,
                "index={i}, value={v}, noise={noise}"
            );
        }
    }
}
