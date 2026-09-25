use engine_api::{
    recipe::DevelopSettings,
    tile::{Extent, TileCoord},
};
use pipeline_cpu::{Image, RenderSource, SigmoidSettings, render, render_scaled, sigmoid};

#[test]
fn sigmoid_is_black_anchored_monotone_and_pivoted() {
    for contrast in [0.25, 1.0, 1.5, 4.0] {
        for skew in [-1.0, 0.0, 1.0] {
            let s = SigmoidSettings { contrast, skew };
            assert_eq!(sigmoid(0.0, s), 0.0);
            assert!((sigmoid(0.18, s) - 0.18).abs() < 1e-6);
            let mut last = 0.0;
            for i in 0..10000 {
                let v = sigmoid(i as f32 / 100.0, s);
                assert!((last..=1.0).contains(&v));
                last = v;
            }
        }
    }
}

#[test]
fn rgb_render_handles_edge_tiles_and_is_deterministic() {
    let image = Image::new(259, 3, vec![vec![0.18; 777]; 3]).unwrap();
    let edge = image.tile(TileCoord::new(0, 1, 0), 0, 1).unwrap();
    assert_eq!(edge.layout().extent, Extent::new(3, 3));
    let s = DevelopSettings::default();
    let source = RenderSource::Rgb(&image);
    let a = render(&s, &source).unwrap();
    assert_eq!(a, render(&s, &source).unwrap());
    for pixel in a.pixels() {
        assert_eq!(pixel[0], pixel[1]);
        assert_eq!(pixel[1], pixel[2]);
        assert!((116..=119).contains(&pixel[0]));
    }
    assert_eq!(render_scaled(&s, &source, 8).unwrap().dimensions(), (33, 1));
    assert!(render_scaled(&s, &source, 0).is_err());
    assert!(Image::new(0, 1, vec![vec![]; 3]).is_err());
    assert!(Image::new(2, 1, vec![vec![0.0]; 3]).is_err());
}
