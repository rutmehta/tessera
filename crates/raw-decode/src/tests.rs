use super::*;

fn raw(layout: CfaLayout) -> libraw_ffi::CfaImage {
    libraw_ffi::CfaImage {
        width: 4,
        height: 2,
        data: vec![0, 550, 2000, 550, 600, 650, 600, 650],
        cfa_layout: layout,
        black: [0.0, 100.0, 200.0, 300.0],
        white: 1000,
        wb_coeffs: [1.0; 4],
        color_matrix: [[0.0; 3]; 3],
        cam_xyz: [[0.0; 3]; 4],
        rgb_cam: [[0.0; 4]; 3],
        crop: [0, 0, 4, 2],
    }
}

#[test]
fn normalization_preserves_headroom_and_channel_black() {
    let image = linearize(raw(CfaLayout::Bayer([[0, 1], [2, 3]]))).unwrap();
    assert_eq!(
        image.pyramid.pixels,
        [0.0, 0.5, 1.2, 0.5, 0.5, 0.5, 0.5, 0.5]
    );
}

#[test]
fn xtrans_uses_full_six_by_six_pattern() {
    let pattern = std::array::from_fn(|y| std::array::from_fn(|x| ((y * 2 + x) % 3) as u8));
    let mut input = raw(CfaLayout::XTrans(pattern));
    input.width = 13;
    input.height = 13;
    input.data = (0..169)
        .map(|i| {
            let c = input.cfa_layout.channel_at(i % 13, i / 13);
            ((1000.0 + input.black[c]) * 0.5) as u16
        })
        .collect();
    assert!(
        linearize(input)
            .unwrap()
            .pyramid
            .pixels
            .iter()
            .all(|&v| v == 0.5)
    );
}

#[test]
fn malformed_planes_and_unsupported_layouts_are_errors() {
    assert!(linearize(raw(CfaLayout::Unsupported)).is_err());
    let mut input = raw(CfaLayout::Bayer([[0, 1], [2, 3]]));
    input.data.pop();
    assert!(linearize(input).is_err());
    let mut input = raw(CfaLayout::Bayer([[0, 1], [2, 3]]));
    input.black[1] = input.white as f32;
    assert!(linearize(input).is_err());
}

#[test]
fn edge_tiles_are_single_plane_zero_halo_level_zero() {
    let p = CfaPyramid {
        extent: Extent::new(259, 257),
        pixels: (0..259 * 257).map(|i| i as f32).collect(),
    };
    assert_eq!(p.level_count(), 1);
    assert_eq!(p.format(), TileFormat::F32Planar);
    assert_eq!(p.channels(), 1);
    assert_eq!(p.halo(), 0);
    for y in 0..2 {
        for x in 0..2 {
            let coord = TileCoord::new(0, x, y);
            let tile = p.tile(coord).unwrap();
            assert_eq!(tile.layout().extent, p.tile_extent(coord));
            assert_eq!(tile.halo(), 0);
            let e = tile.layout().extent;
            let samples = tile.samples::<f32>().unwrap();
            for row in 0..e.height {
                for col in 0..e.width {
                    assert_eq!(
                        samples[(row * e.width + col) as usize],
                        ((y * TILE_SIZE + row) * 259 + x * TILE_SIZE + col) as f32
                    );
                }
            }
        }
    }
    for c in [
        TileCoord::new(1, 0, 0),
        TileCoord::new(0, 2, 0),
        TileCoord::new(0, u32::MAX, u32::MAX),
    ] {
        assert!(p.tile(c).is_err());
    }
}

#[test]
fn from_linear_validates_and_wraps_samples() {
    let image = CfaImage::from_linear(3, 2, vec![0.0, 0.5, 1.2, 4.0, -0.1, 1.0]).unwrap();
    assert_eq!(image.pyramid().extent(), Extent::new(3, 2));
    assert_eq!(image.pyramid.pixels[3], 4.0);
    assert!(CfaImage::from_linear(3, 2, vec![0.0; 5]).is_err());
    assert!(CfaImage::from_linear(0, 2, vec![]).is_err());
    assert!(CfaImage::from_linear(1, 1, vec![f32::NAN]).is_err());
}
