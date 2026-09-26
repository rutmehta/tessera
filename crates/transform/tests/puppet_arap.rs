use transform::puppet::{PuppetDensity, PuppetMode, PuppetPin, PuppetWarp};

#[test]
fn puppet_pin_translation_rotation_and_determinism() {
    let mut warp = PuppetWarp::from_alpha(&[255; 64], 8, 8, PuppetDensity::Dense, 0).unwrap();
    warp.pins.push(PuppetPin {
        vertex: 0,
        target: [10.0, 20.0],
        rotation: Some(std::f64::consts::FRAC_PI_2),
    });
    for mode in [PuppetMode::Normal, PuppetMode::Rigid] {
        warp.mode = mode;
        let solved = warp.solve().unwrap();
        assert_eq!(solved, warp.solve().unwrap());
        for (p, q) in warp.rest_vertices.iter().zip(&solved.vertices) {
            assert!((q[0] - (10.0 - p[1])).abs() < 1e-6);
            assert!((q[1] - (20.0 + p[0])).abs() < 1e-6);
        }
        assert_eq!(solved.vertices[0], [10.0, 20.0]);
        let p = solved.inverse_map([7.0, 22.0]).unwrap();
        assert!((p[0] - 2.0).abs() < 1e-6 && (p[1] - 3.0).abs() < 1e-6);
    }
}

#[test]
fn puppet_multiple_pins_deform_interior() {
    let mut warp = PuppetWarp::from_alpha(&[255; 256], 16, 16, PuppetDensity::Dense, 0).unwrap();
    let far = warp
        .rest_vertices
        .iter()
        .position(|p| *p == [16.0, 16.0])
        .unwrap();
    warp.pins = vec![
        PuppetPin {
            vertex: 0,
            target: [0.0, 0.0],
            rotation: None,
        },
        PuppetPin {
            vertex: far,
            target: [19.0, 18.0],
            rotation: None,
        },
    ];
    let solved = warp.solve().unwrap();
    assert_eq!(solved.vertices[0], [0.0, 0.0]);
    assert_eq!(solved.vertices[far], [19.0, 18.0]);
    let mid = warp
        .rest_vertices
        .iter()
        .position(|p| *p == [8.0, 8.0])
        .unwrap();
    assert!(solved.vertices[mid][0] > 8.1 && solved.vertices[mid][0] < 11.0);
    assert!(solved.vertices.iter().flatten().all(|x| x.is_finite()));
}

#[test]
fn puppet_alpha_mesh_identity_and_hole() {
    let mut alpha = vec![255; 16 * 16];
    for y in 4..12 {
        for x in 4..12 {
            alpha[y * 16 + x] = 0;
        }
    }
    let warp = PuppetWarp::from_alpha(&alpha, 16, 16, PuppetDensity::Dense, 0).unwrap();
    let solved = warp.solve().unwrap();
    assert_eq!(solved.vertices, warp.rest_vertices);
    assert!(solved.inverse_map([8.0, 8.0]).is_none());
    let p = solved.inverse_map([1.0, 1.0]).unwrap();
    assert!((p[0] - 1.0).abs() < 1e-9 && (p[1] - 1.0).abs() < 1e-9);
}
