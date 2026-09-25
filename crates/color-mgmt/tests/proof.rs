use color_mgmt::*;
fn small_gamut(r: &mut Registry) -> std::sync::Arc<Profile> {
    use lcms2::*;
    let xy = |x, y| CIExyY { x, y, Y: 1.0 };
    let curve = ToneCurve::new(2.2);
    let mut p = lcms2::Profile::new_rgb(
        &xy(0.3457, 0.3585),
        &CIExyYTRIPLE {
            Red: xy(0.48, 0.34),
            Green: xy(0.30, 0.48),
            Blue: xy(0.23, 0.20),
        },
        &[&curve, &curve, &curve],
    )
    .unwrap();
    // ICC v2 media white retains the warm, dim paper in absolute intent.
    p.set_version(2.4);
    p.set_device_class(ProfileClassSignature::OutputClass);
    p.write_tag(
        TagSignature::MediaWhitePointTag,
        Tag::CIEXYZ(&CIEXYZ {
            X: 0.80,
            Y: 0.85,
            Z: 0.55,
        }),
    );
    r.load_bytes(&p.icc().unwrap()).unwrap()
}
#[test]
fn small_gamut_proof_warns_separately_and_simulates_paper() {
    let mut r = Registry::new();
    let s = r.builtin(Builtin::Srgb).unwrap();
    let p = small_gamut(&mut r);
    let relative = Transform::proof(&s, &s, &p, Default::default()).unwrap();
    let paper = Transform::proof(
        &s,
        &s,
        &p,
        TransformOptions {
            simulate_paper: true,
            ..Default::default()
        },
    )
    .unwrap();
    assert!(relative.gamut_warning([1., 0., 0.]).proof);
    assert!(!relative.gamut_warning([1., 0., 0.]).monitor);
    assert!(!relative.gamut_warning([0.5; 3]).proof);
    let white = relative.apply([1.; 3]);
    let paper_white = paper.apply([1.; 3]);
    assert!(white.iter().all(|c| (*c - 1.).abs() < 0.01), "{white:?}");
    assert!(
        paper_white[2] < white[2] - 0.05,
        "{white:?} {paper_white:?}"
    );
    assert!(paper_white.iter().all(|c| c.is_finite()));
    let red = relative.apply([1., 0., 0.]);
    assert!(red[1] > 0.1, "{red:?}");
}

#[test]
fn proof_preserves_ink_black_with_source_bpc_enabled() {
    use lcms2::{CIExyY, CIExyYTRIPLE, ToneCurve};
    let xy = |x, y| CIExyY { x, y, Y: 1.0 };
    // Encode nonzero black in the TRC, not a metadata tag a CMM may ignore.
    let samples: Vec<f32> = (0..4096)
        .map(|i| 0.03 + 0.97 * (i as f32 / 4095.0).powf(2.2))
        .collect();
    let curve = ToneCurve::new_tabulated_float(&samples);
    let mut profile = lcms2::Profile::new_rgb(
        &xy(0.3457, 0.3585),
        &CIExyYTRIPLE {
            Red: xy(0.64, 0.33),
            Green: xy(0.30, 0.60),
            Blue: xy(0.15, 0.06),
        },
        &[&curve, &curve, &curve],
    )
    .unwrap();
    profile.set_device_class(lcms2::ProfileClassSignature::OutputClass);
    let mut r = Registry::new();
    let s = r.builtin(Builtin::Srgb).unwrap();
    let p = r.load_bytes(&profile.icc().unwrap()).unwrap();
    for simulate_paper in [false, true] {
        let t = Transform::proof(
            &s,
            &s,
            &p,
            TransformOptions {
                simulate_paper,
                ..Default::default()
            },
        )
        .unwrap();
        let black = t.apply([0.0; 3]);
        assert!(
            black.iter().all(|c| c.is_finite() && *c > 0.1 && *c < 0.3),
            "ink black was crushed: {black:?}"
        );
        let lut_black = t.lut33().sample([0.0; 3]);
        for c in 0..3 {
            assert!((black[c] - lut_black[c]).abs() < 0.00001);
        }
    }
}
