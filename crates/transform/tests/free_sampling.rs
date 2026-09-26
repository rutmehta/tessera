use transform::{Image, Kernel, sample::sample};

#[test]
fn transparent_extension_keeps_premultiplied_color() {
    let image = Image::new(1, 1, [vec![0.4], vec![0.2], vec![0.1], vec![0.5]]).unwrap();
    assert_eq!(
        sample(&image, [0., 0.5], Kernel::Bilinear),
        [0.2, 0.1, 0.05, 0.25]
    );
    for k in [
        Kernel::Nearest,
        Kernel::Bilinear,
        Kernel::Bicubic,
        Kernel::Lanczos3,
        Kernel::Automatic,
    ] {
        assert_eq!(sample(&image, [-1e20; 2], k), [0.; 4]);
        assert_eq!(sample(&image, [f32::NAN; 2], k), [0.; 4]);
        let s = sample(&image, [0.25, 0.75], k);
        assert!((s[0] - s[3] * 0.8).abs() < 1e-6);
    }
}

fn reference_weight(x: f32, k: Kernel) -> f32 {
    let x = x.abs();
    match k {
        Kernel::Bilinear => (1. - x).max(0.),
        Kernel::Bicubic | Kernel::Automatic => {
            if x < 1. {
                ((1.5 * x - 2.5) * x) * x + 1.
            } else if x < 2. {
                ((-0.5 * x + 2.5) * x - 4.) * x + 2.
            } else {
                0.
            }
        }
        Kernel::Lanczos3 => {
            if x == 0. {
                1.
            } else if x >= 3. {
                0.
            } else {
                let p = std::f32::consts::PI * x;
                (p.sin() / p) * ((p / 3.).sin() / (p / 3.))
            }
        }
        _ => unreachable!(),
    }
}
#[test]
fn kernels_match_scalar_reference_at_fractional_positions() {
    let image = Image::new(
        7,
        5,
        std::array::from_fn(|c| {
            (0..35)
                .map(|i| ((i * 17 + c * 3) % 37) as f32 / 40.)
                .collect()
        }),
    )
    .unwrap();
    for k in [
        Kernel::Bilinear,
        Kernel::Bicubic,
        Kernel::Lanczos3,
        Kernel::Automatic,
    ] {
        let radius = match k {
            Kernel::Bilinear => 1,
            Kernel::Lanczos3 => 3,
            _ => 2,
        };
        for p in [[2.13_f32, 2.78], [0.12, 0.96], [-0.3, 3.24], [7.12, 4.43]] {
            let [x, y] = [p[0] - 0.5, p[1] - 0.5];
            let x0 = x.floor() as i32 - radius + 1;
            let y0 = y.floor() as i32 - radius + 1;
            let mut wx: Vec<_> = (0..2 * radius)
                .map(|i| reference_weight(x - (x0 + i) as f32, k))
                .collect();
            let mut wy: Vec<_> = (0..2 * radius)
                .map(|i| reference_weight(y - (y0 + i) as f32, k))
                .collect();
            if k == Kernel::Lanczos3 {
                let sx: f32 = wx.iter().sum();
                let sy: f32 = wy.iter().sum();
                for v in &mut wx {
                    *v /= sx;
                }
                for v in &mut wy {
                    *v /= sy;
                }
            }
            let mut expected = [0.; 4];
            for (j, &b) in wy.iter().enumerate() {
                for (i, &a) in wx.iter().enumerate() {
                    let (xx, yy) = (x0 + i as i32, y0 + j as i32);
                    if (0..7).contains(&xx) && (0..5).contains(&yy) {
                        for (c, v) in expected.iter_mut().enumerate() {
                            *v += image.planes[c][yy as usize * 7 + xx as usize] * (a * b);
                        }
                    }
                }
            }
            assert_eq!(sample(&image, p, k), expected, "{k:?} {p:?}");
        }
    }
}
