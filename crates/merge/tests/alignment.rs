use merge::{LinearImage, alignment::align};
fn texture(x: f64, y: f64) -> [f32; 3] {
    let v = 0.35
        + 0.10 * (x * 0.23 + y * 0.17).sin()
        + 0.09 * (x * 0.41 - y * 0.13).cos()
        + 0.08 * ((x * 0.07).sin() * 7. + y * 0.31).cos();
    [v as f32, (v * 0.8) as f32, (v * 0.6) as f32]
}
fn image(w: usize, h: usize, f: impl Fn(f64, f64) -> [f32; 3]) -> LinearImage {
    LinearImage {
        width: w,
        height: h,
        pixels: (0..w * h)
            .map(|i| f((i % w) as f64, (i / w) as f64))
            .collect(),
        color_matrix: [[1., 0., 0.], [0., 1., 0.], [0., 0., 1.]],
        as_shot_neutral: [1.; 3],
    }
}
#[test]
fn phase_alignment_recovers_translation_and_small_rotation() {
    let (w, h) = (180, 140);
    let (dx, dy, angle) = (5.3, -3.7, 1.7_f64.to_radians());
    let a = image(w, h, texture);
    let b = image(w, h, |x, y| {
        let u = x - 89.5 - dx;
        let v = y - 69.5 - dy;
        texture(
            angle.cos() * u + angle.sin() * v + 89.5,
            -angle.sin() * u + angle.cos() * v + 69.5,
        )
        .map(|v| v * 0.25)
    });
    let t = align(&a, &b, 0.25).unwrap();
    eprintln!(
        "HDR alignment: {:?}, angle {} degrees",
        t.translation,
        t.rotation_radians.to_degrees()
    );
    assert!((t.translation[0] - dx).abs() < 0.25);
    assert!((t.translation[1] - dy).abs() < 0.25);
    assert!((t.rotation_radians - angle).abs() < 0.1_f64.to_radians());
    let mut error = 0.;
    for y in 15..125 {
        for x in 15..165 {
            let p = t.map(x as f64, y as f64);
            let q = b.sample(p[0], p[1]).unwrap();
            error += (q[0] as f64 / 0.25 - a.pixels[y * w + x][0] as f64).abs();
        }
    }
    assert!(error / (110. * 150.) < 0.004);
}
#[test]
fn tile_refinement_tracks_local_residual_without_moving_flat_tiles() {
    let a = image(192, 128, texture);
    let b = image(192, 128, |x, y| {
        texture(x - 0.7 * (y / 128. * std::f64::consts::PI * 2.).sin(), y)
    });
    let t = align(&a, &b, 1.).unwrap();
    assert!(t.tile_offsets.iter().any(|p| p[0].abs() > 0.15));
    let flat = image(64, 64, |_, _| [0.2; 3]);
    let t = align(&flat, &flat, 1.).unwrap();
    assert_eq!(t.translation, [0., 0.]);
    assert!(t.tile_offsets.iter().all(|p| *p == [0., 0.]));
}
