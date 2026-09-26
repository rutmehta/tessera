use filters::liquify::{Brush, BrushTool, FaceAware, FaceParams, Interpolation, Mesh};

#[test]
fn clockwise_and_counterclockwise_twirl_are_opposite() {
    let mut cw = Mesh::new(33, 33, 1).unwrap();
    let mut ccw = cw.clone();
    let brush = Brush {
        size: 30.,
        density: 1.,
        pressure: 1.,
        rate: 1.,
    };
    cw.apply_brush(BrushTool::Twirl, [16., 16.], [0., 0.], &brush)
        .unwrap();
    ccw.apply_brush(
        BrushTool::TwirlCounterClockwise,
        [16., 16.],
        [0., 0.],
        &brush,
    )
    .unwrap();
    let a = cw.displacement_at(20., 16.);
    let b = ccw.displacement_at(20., 16.);
    assert!(a[1] < 0. && b[1] > 0.);
    assert!((a[1] + b[1]).abs() < 1e-6);
    assert!((a[0] - b[0]).abs() < 1e-6);
}

#[test]
fn field_interpolation_single_pixel_and_partial_last_cell() {
    let mut m = Mesh::new(12, 10, 4).unwrap();
    let (w, h) = m.grid();
    for y in 0..h {
        for x in 0..w {
            m.displacement[y * w + x] = [x as f32 * 4., y as f32 * 8.];
        }
    }
    assert_eq!(m.displacement_at(10., 9.), [10., 18.]);
    assert_eq!(m.displacement_at(-3., -5.), [0., 0.]);
    assert_eq!(m.displacement_at(200., 200.), [12., 24.]);
    let mut single = Mesh::new(1, 1, 10).unwrap();
    assert_eq!(single.grid(), (1, 1));
    single.displacement[0] = [100., -100.];
    let r = Raster::new(Extent::new(1, 1), 4, Depth::F32, 0.5);
    for interpolation in [Interpolation::Bilinear, Interpolation::Bicubic] {
        assert_eq!(
            single
                .render(&r, interpolation, &AtomicBool::new(false))
                .unwrap()
                .pixel(0, 0),
            r.pixel(0, 0)
        );
    }
    assert!(Mesh::new(u32::MAX, u32::MAX, 1).is_err());
}
#[test]
fn brush_pressure_rate_density_size_and_partial_freeze() {
    let b = Brush {
        size: 20.,
        density: 1.,
        pressure: 0.5,
        rate: 0.5,
    };
    let mut m = Mesh::new(31, 31, 1).unwrap();
    m.freeze[15 * 31 + 15] = 0.5;
    m.apply_brush(BrushTool::ForwardWarp, [15., 15.], [8., 0.], &b)
        .unwrap();
    assert_eq!(m.displacement_at(15., 15.), [-1., 0.]);
    assert_eq!(m.displacement_at(16., 15.), [-2., 0.]);
    assert_eq!(m.displacement_at(25., 15.), [0., 0.]);
    let mut soft = Mesh::new(31, 31, 1).unwrap();
    soft.apply_brush(
        BrushTool::ForwardWarp,
        [15., 15.],
        [8., 0.],
        &Brush { density: 0., ..b },
    )
    .unwrap();
    assert!(soft.displacement_at(20., 15.)[0].abs() < m.displacement_at(20., 15.)[0].abs());
    let before = m.clone();
    assert!(
        m.apply_brush(BrushTool::ForwardWarp, [f32::NAN, 1.], [1., 1.], &b)
            .is_err()
    );
    assert_eq!(m, before);
}
#[test]
fn malformed_serialized_masks_and_versions_are_rejected() {
    let m = Mesh::new(5, 5, 2).unwrap();
    for (field, value) in [
        ("version", serde_json::json!(2)),
        ("freeze", serde_json::json!([])),
        ("displacement", serde_json::json!([[0., 0.]])),
        ("unexpected", serde_json::json!(true)),
    ] {
        let mut v = serde_json::to_value(&m).unwrap();
        v[field] = value;
        assert!(serde_json::from_value::<Mesh>(v).is_err());
    }
    for value in [-0.1, 1.1] {
        let mut v = serde_json::to_value(&m).unwrap();
        v["freeze"][0] = serde_json::json!(value);
        assert!(serde_json::from_value::<Mesh>(v).is_err());
    }
}
#[test]
fn face_roll_covariance_and_multiple_faces_do_not_touch_distant_pixels() {
    let f = face(FaceParams {
        eye_size: 0.5,
        mouth_smile: 0.4,
        chin: 0.3,
        ..Default::default()
    });
    let mut rotated = f.clone();
    for p in &mut rotated.landmarks5 {
        *p = [100. - p[1], p[0]];
    }
    let mut a = Mesh::new(101, 101, 1).unwrap();
    let mut b = a.clone();
    f.apply(&mut a).unwrap();
    rotated.apply(&mut b).unwrap();
    for y in 0..101 {
        for x in 0..101 {
            let da = a.displacement_at(x as f32, y as f32);
            let db = b.displacement_at((100 - y) as f32, x as f32);
            assert!((db[0] + da[1]).abs() < 1e-4 && (db[1] - da[0]).abs() < 1e-4);
        }
    }
    let mut m = Mesh::new(201, 101, 1).unwrap();
    f.apply(&mut m).unwrap();
    let before = m.displacement_at(42., 40.);
    let mut other = f;
    for p in &mut other.landmarks5 {
        p[0] += 100.;
    }
    other.apply(&mut m).unwrap();
    assert_eq!(m.displacement_at(42., 40.), before);
    assert_eq!(m.displacement_at(100., 0.), [0., 0.]);
}

#[test]
fn bicubic_uses_catmull_rom_not_bilinear() {
    let mut r = Raster::new(Extent::new(17, 5), 4, Depth::F32, 0.);
    r.edit_region(Rect::new(8, 0, 9, 5), 1, |_, _, p| *p = [2., 1., 0.5, 0.5])
        .unwrap();
    let mut m = Mesh::new(17, 5, 4).unwrap();
    m.displacement.fill([0.5, 0.]);
    let cancel = AtomicBool::new(false);
    let linear = m
        .render(&r, Interpolation::Bilinear, &cancel)
        .unwrap()
        .pixel(8, 2);
    let cubic = m
        .render(&r, Interpolation::Bicubic, &cancel)
        .unwrap()
        .pixel(8, 2);
    let t = 0.5_f64;
    let weight = 1.5 * t * t * t - 2.5 * t * t + 1.;
    assert!((f64::from(cubic[0]) - 2. * weight).abs() < 1e-6);
    assert!((linear[0] - 1.).abs() < 1e-6);
    assert!(cubic[0] > linear[0]);
}
#[test]
fn render_preserves_depth_channels_and_crosses_tile_seam() {
    for depth in [Depth::U8, Depth::U16, Depth::F32] {
        for channels in [1, 3, 4] {
            let mut r = Raster::new(Extent::new(273, 3), channels, depth, 0.);
            r.edit_region(Rect::of_extent(r.extent()), 7, |x, _, p| {
                *p = [x as f32 / 300., 0.2, 0.3, 0.6]
            })
            .unwrap();
            let mut m = Mesh::new(273, 3, 8).unwrap();
            m.displacement.fill([-1., 0.]);
            for interpolation in [Interpolation::Bilinear, Interpolation::Bicubic] {
                let out = m
                    .render(&r, interpolation, &AtomicBool::new(false))
                    .unwrap();
                assert_eq!(out.depth(), depth);
                assert_eq!(out.channels(), channels);
                assert_eq!(out.pixel(256, 1), r.pixel(255, 1));
                assert_eq!(out.max_rev(), 8);
            }
        }
    }
}

fn face(params: FaceParams) -> FaceAware {
    FaceAware {
        landmarks5: [[40., 40.], [60., 40.], [50., 53.], [43., 65.], [57., 65.]],
        params,
    }
}
#[test]
fn face_controls_are_local_finite_and_individually_effective() {
    let blank = Mesh::new(101, 101, 1).unwrap();
    let mut m = blank.clone();
    face(FaceParams::default()).apply(&mut m).unwrap();
    assert_eq!(m, blank);
    let fields = [
        "eye_size",
        "eye_height",
        "eye_width",
        "eye_tilt",
        "nose_width",
        "nose_height",
        "mouth_smile",
        "mouth_width",
        "mouth_height",
        "face_width",
        "jaw",
        "chin",
        "forehead",
    ];
    for field in fields {
        let mut json = serde_json::to_value(FaceParams::default()).unwrap();
        json[field] = serde_json::json!(0.7);
        let params: FaceParams = serde_json::from_value(json).unwrap();
        let mut m = blank.clone();
        face(params).apply(&mut m).unwrap();
        assert!(
            m.displacement
                .iter()
                .any(|d| d[0].abs() + d[1].abs() > 1e-3),
            "{field}"
        );
        assert!(m.displacement.iter().flatten().all(|x| x.is_finite()));
        assert_eq!(m.displacement_at(0., 0.), [0., 0.], "{field}");
        let mut frozen = blank.clone();
        frozen.freeze.fill(1.);
        face(params).apply(&mut frozen).unwrap();
        assert_eq!(frozen.displacement, blank.displacement);
    }
    let before = m.clone();
    let mut bad = face(FaceParams::default());
    bad.landmarks5[1] = bad.landmarks5[0];
    assert!(bad.apply(&mut m).is_err());
    assert_eq!(m, before);
    assert!(
        face(FaceParams {
            eye_size: 2.,
            ..Default::default()
        })
        .apply(&mut m)
        .is_err()
    );
}
#[test]
fn face_eye_expansion_and_smile_have_correct_inverse_directions() {
    let mut m = Mesh::new(101, 101, 1).unwrap();
    face(FaceParams {
        eye_size: 1.,
        ..Default::default()
    })
    .apply(&mut m)
    .unwrap();
    assert!(m.displacement_at(42., 40.)[0] < 0.);
    assert!(m.displacement_at(38., 40.)[0] > 0.);
    let mut m = Mesh::new(101, 101, 1).unwrap();
    face(FaceParams {
        mouth_smile: 1.,
        ..Default::default()
    })
    .apply(&mut m)
    .unwrap();
    assert!(m.displacement_at(43., 65.)[1] > 0.);
    assert!(m.displacement_at(57., 65.)[1] > 0.);
}

#[test]
fn brushes_freeze_reconstruct_and_controls() {
    let mut m = Mesh::new(33, 33, 2).unwrap();
    let b = Brush {
        size: 24.,
        density: 1.,
        pressure: 1.,
        rate: 1.,
    };
    m.apply_brush(BrushTool::Freeze, [16., 16.], [4., 0.], &b)
        .unwrap();
    m.apply_brush(BrushTool::ForwardWarp, [16., 16.], [4., 0.], &b)
        .unwrap();
    assert_eq!(m.displacement_at(16., 16.), [0., 0.]);
    m.apply_brush(BrushTool::Thaw, [16., 16.], [0., 0.], &b)
        .unwrap();
    m.apply_brush(BrushTool::ForwardWarp, [16., 16.], [4., 0.], &b)
        .unwrap();
    assert_eq!(m.displacement_at(16., 16.), [-4., 0.]);
    assert_eq!(m.displacement_at(0., 0.), [0., 0.]);
    m.apply_brush(BrushTool::Reconstruct, [16., 16.], [0., 0.], &b)
        .unwrap();
    assert_eq!(m.displacement_at(16., 16.), [0., 0.]);
    for tool in [
        BrushTool::Twirl,
        BrushTool::Pucker,
        BrushTool::Bloat,
        BrushTool::PushLeft,
    ] {
        let mut m = Mesh::new(33, 33, 2).unwrap();
        m.apply_brush(tool, [16., 16.], [0., 4.], &b).unwrap();
        let d = m.displacement_at(20., 16.);
        match tool {
            BrushTool::Twirl => assert!(d[1] < 0.),
            BrushTool::Pucker => assert!(d[0] > 0.),
            BrushTool::Bloat => assert!(d[0] < 0.),
            BrushTool::PushLeft => assert!(d[0] < 0.),
            _ => unreachable!(),
        }
    }
    let before = m.clone();
    m.apply_brush(
        BrushTool::Bloat,
        [16., 16.],
        [0., 0.],
        &Brush { pressure: 0., ..b },
    )
    .unwrap();
    assert_eq!(m, before);
    assert!(
        m.apply_brush(
            BrushTool::Bloat,
            [16., 16.],
            [0., 0.],
            &Brush {
                size: f32::NAN,
                ..b
            }
        )
        .is_err()
    );
    assert_eq!(m, before);
}

#[test]
fn smooth_uses_snapshot_and_preserves_frozen_nodes() {
    let mut m = Mesh::new(9, 9, 2).unwrap();
    m.displacement[12] = [9., 0.];
    m.freeze[11] = 1.;
    m.apply_brush(
        BrushTool::Smooth,
        [4., 4.],
        [0., 0.],
        &Brush {
            size: 30.,
            density: 1.,
            pressure: 1.,
            rate: 1.,
        },
    )
    .unwrap();
    assert_eq!(m.displacement[12], [1., 0.]);
    assert_eq!(m.displacement[11], [0., 0.]);
    assert_eq!(m.displacement[13], [1., 0.]);
}

use compositor::{
    geom::Rect,
    raster::{Depth, Raster},
};
use engine_api::tile::Extent;
use std::sync::atomic::AtomicBool;

#[test]
fn inverse_render_identity_translation_and_cancel() {
    let mut input = Raster::new(Extent::new(17, 13), 4, Depth::F32, 0.);
    input
        .edit_region(Rect::of_extent(input.extent()), 3, |x, y, p| {
            *p = [x as f32, y as f32, 2., 0.5]
        })
        .unwrap();
    let mut mesh = Mesh::new(17, 13, 4).unwrap();
    let cancel = AtomicBool::new(false);
    for interpolation in [Interpolation::Bilinear, Interpolation::Bicubic] {
        let out = mesh.render(&input, interpolation, &cancel).unwrap();
        assert_eq!(out.max_rev(), 3);
        assert_eq!(out.pixel(8, 6), input.pixel(8, 6));
    }
    mesh.displacement.fill([-1.5, 0.25]);
    for interpolation in [Interpolation::Bilinear, Interpolation::Bicubic] {
        let out = mesh.render(&input, interpolation, &cancel).unwrap();
        let p = out.pixel(8, 6);
        assert!((p[0] - 6.5).abs() < 1e-5 && (p[1] - 6.25).abs() < 1e-5);
        assert_eq!(out.max_rev(), 4);
        assert_eq!(out.pixel(0, 0)[0], 0.);
        assert_eq!(input.pixel(8, 6)[0], 8.);
        assert!(
            mesh.render(&input, interpolation, &AtomicBool::new(true))
                .is_err()
        );
    }
    assert!(
        mesh.render(
            &Raster::new(Extent::new(1, 1), 4, Depth::F32, 0.),
            Interpolation::Bilinear,
            &cancel
        )
        .is_err()
    );
}

#[test]
fn mesh_roundtrip_and_validation() {
    let mesh = Mesh::new(17, 13, 4).unwrap();
    assert_eq!(mesh.grid(), (5, 4));
    assert_eq!(mesh.displacement_at(16., 12.), [0., 0.]);
    let json = serde_json::to_string(&mesh).unwrap();
    let loaded: Mesh = serde_json::from_str(&json).unwrap();
    assert_eq!(loaded, mesh);
    assert!(Mesh::new(0, 10, 4).is_err());
    assert!(Mesh::new(10, 10, 0).is_err());
    for field in ["version", "cell_size", "width"] {
        let mut v = serde_json::to_value(&mesh).unwrap();
        v[field] = 0.into();
        assert!(serde_json::from_value::<Mesh>(v).is_err());
    }
    let mut invalid = mesh.clone();
    invalid.freeze.pop();
    assert!(invalid.validate().is_err());
    let mut invalid = mesh;
    invalid.displacement[0][0] = f32::NAN;
    assert!(invalid.validate().is_err());
}
