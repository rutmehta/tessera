use transform::puppet::{PuppetDensity, PuppetMode, PuppetPin, PuppetWarp};

#[test]
fn puppet_rejects_invalid_input_and_serialized_state() {
    assert!(PuppetWarp::from_alpha(&[], 0, 0, PuppetDensity::Normal, 0).is_err());
    assert!(PuppetWarp::from_alpha(&[0; 16], 4, 4, PuppetDensity::Normal, 0).is_err());
    assert!(PuppetWarp::from_alpha(&[255; 16], usize::MAX, 4, PuppetDensity::Normal, 0).is_err());
    assert!(PuppetWarp::from_alpha(&[255; 16], 4, 4, PuppetDensity::Normal, 65).is_err());
    let warp = PuppetWarp::from_alpha(&[255; 16], 4, 4, PuppetDensity::Normal, 0).unwrap();
    for value in [0, 101, usize::MAX] {
        let mut w = warp.clone();
        w.iterations = value;
        assert!(w.solve().is_err());
    }
    let mut w = warp.clone();
    w.rest_vertices[0][0] = f64::NAN;
    assert!(w.solve().is_err());
    let mut w = warp.clone();
    w.triangles[0][0] = usize::MAX;
    assert!(w.solve().is_err());
    let mut w = warp.clone();
    w.triangles[0] = [0, 0, 0];
    assert!(w.solve().is_err());
    for pin in [
        PuppetPin {
            vertex: usize::MAX,
            target: [0.0, 0.0],
            rotation: None,
        },
        PuppetPin {
            vertex: 0,
            target: [f64::INFINITY, 0.0],
            rotation: None,
        },
        PuppetPin {
            vertex: 0,
            target: [0.0, 0.0],
            rotation: Some(f64::NAN),
        },
    ] {
        let mut w = warp.clone();
        w.pins.push(pin);
        assert!(w.solve().is_err());
    }
    let mut w = warp.clone();
    w.pins = vec![
        PuppetPin {
            vertex: 0,
            target: [0.0, 0.0],
            rotation: None
        };
        2
    ];
    assert!(w.solve().is_err());
    let solved = warp.solve().unwrap();
    assert!(solved.inverse_map([f64::NAN, 0.0]).is_none());
    assert!(solved.inverse_map([-1.0, -1.0]).is_none());
}

#[test]
fn puppet_prepared_mapping_rejects_corrupted_indices_without_panicking() {
    let warp = PuppetWarp::from_alpha(&[255; 16], 4, 4, PuppetDensity::Normal, 0).unwrap();
    let mut prepared = warp.solve().unwrap();
    prepared.triangles = vec![[0, 1, usize::MAX]];
    assert!(prepared.inverse_map([1.0, 1.0]).is_none());
}

#[test]
fn puppet_density_and_expansion_control_coverage() {
    let dense = PuppetWarp::from_alpha(&[255; 256], 16, 16, PuppetDensity::Dense, 0).unwrap();
    let normal = PuppetWarp::from_alpha(&[255; 256], 16, 16, PuppetDensity::Normal, 0).unwrap();
    let sparse = PuppetWarp::from_alpha(&[255; 256], 16, 16, PuppetDensity::Sparse, 0).unwrap();
    assert!(dense.rest_vertices.len() > normal.rest_vertices.len());
    assert!(normal.rest_vertices.len() > sparse.rest_vertices.len());
    let mut alpha = [0; 256];
    alpha[8 * 16 + 8] = 255;
    let small = PuppetWarp::from_alpha(&alpha, 16, 16, PuppetDensity::Dense, 0).unwrap();
    let large = PuppetWarp::from_alpha(&alpha, 16, 16, PuppetDensity::Dense, 3).unwrap();
    assert!(large.triangles.len() > small.triangles.len());
    assert!(small.solve().unwrap().inverse_map([6.0, 6.0]).is_none());
    assert!(large.solve().unwrap().inverse_map([6.0, 6.0]).is_some());
}

#[test]
fn puppet_unpinned_disconnected_island_stays_at_rest() {
    let mut alpha = [0; 32 * 8];
    for y in 0..8 {
        for x in (0..8).chain(24..32) {
            alpha[y * 32 + x] = 255;
        }
    }
    let mut warp = PuppetWarp::from_alpha(&alpha, 32, 8, PuppetDensity::Dense, 0).unwrap();
    warp.pins.push(PuppetPin {
        vertex: 0,
        target: [3.0, 4.0],
        rotation: None,
    });
    let solved = warp.solve().unwrap();
    for (p, q) in warp.rest_vertices.iter().zip(&solved.vertices) {
        if p[0] >= 24.0 {
            assert_eq!(p, q);
        } else {
            assert!((q[0] - p[0] - 3.0).abs() < 1e-6);
            assert!((q[1] - p[1] - 4.0).abs() < 1e-6);
        }
    }
}

#[test]
fn puppet_rigid_mode_changes_multipin_solution() {
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
            rotation: Some(0.0),
        },
        PuppetPin {
            vertex: far,
            target: [10.0, 20.0],
            rotation: None,
        },
    ];
    let normal = warp.solve().unwrap();
    warp.mode = PuppetMode::Rigid;
    let rigid = warp.solve().unwrap();
    assert_ne!(normal.vertices, rigid.vertices);
    assert_eq!(rigid.vertices[far], [10.0, 20.0]);
    warp.pins.reverse();
    assert_eq!(rigid, warp.solve().unwrap());
}

#[test]
fn puppet_subpixel_and_odd_dimensions_have_finite_boundary_mapping() {
    for (width, height) in [(1, 1), (1, 7), (7, 5)] {
        let warp = PuppetWarp::from_alpha(
            &vec![255; width * height],
            width,
            height,
            PuppetDensity::Dense,
            0,
        )
        .unwrap();
        let solved = warp.solve().unwrap();
        assert_eq!(
            solved.inverse_map([width as f64, height as f64]),
            Some([width as f64, height as f64])
        );
    }
}
