use engine_api::recipe::{LocalAdjustment, LocalParams, MaskComponent, MaskKind};
use pipeline_cpu::{Image, adjust_local, blend_local, locals_image};

fn input() -> Image {
    Image::new(
        4,
        1,
        vec![
            vec![0.4, 0.6, -0.1, 2.0],
            vec![0.2, 0.1, 0.3, 1.0],
            vec![0.1, 0.2, 0.4, 0.5],
        ],
    )
    .unwrap()
}
fn full(params: LocalParams) -> LocalAdjustment {
    LocalAdjustment {
        components: vec![MaskComponent::new(MaskKind::Radial {
            center: [0.5, 0.5],
            radii: [1., 1.],
            angle: 0.,
            feather: 0.,
        })],
        params,
        ..Default::default()
    }
}
#[test]
fn signed_hue_is_scaled_before_wrapping() {
    let image = input();
    let a = adjust_local(
        &image,
        &LocalParams {
            hue: -60.,
            ..Default::default()
        },
        50.,
    )
    .unwrap();
    let b = adjust_local(
        &image,
        &LocalParams {
            hue: -30.,
            ..Default::default()
        },
        100.,
    )
    .unwrap();
    for (a, b) in a.planes().iter().flatten().zip(b.planes().iter().flatten()) {
        assert!((a - b).abs() < 1e-6);
    }
}
#[test]
fn amount_scales_ev_not_alpha_and_disabled_groups_are_identity() {
    let image = input();
    let mut g = full(LocalParams {
        exposure: 1.,
        ..Default::default()
    });
    g.amount = 200.;
    let out = locals_image(&image, &[g.clone()], Default::default()).unwrap();
    for (a, b) in out
        .planes()
        .iter()
        .flatten()
        .zip(image.planes().iter().flatten())
    {
        assert!((a - 4. * b).abs() < 1e-6);
    }
    for amount in [0., 100.] {
        g.amount = amount;
        g.enabled = false;
        assert_eq!(
            locals_image(&image, &[g.clone()], Default::default())
                .unwrap()
                .planes(),
            image.planes()
        );
    }
    assert_eq!(
        adjust_local(&image, &LocalParams::default(), 100.)
            .unwrap()
            .planes(),
        image.planes()
    );
}
#[test]
fn invalid_controls_and_blend_layouts_reject() {
    let image = input();
    for amount in [f32::NAN, -1., 201.] {
        assert!(adjust_local(&image, &LocalParams::default(), amount).is_err());
    }
    for p in [
        LocalParams {
            tint: f32::NAN,
            ..Default::default()
        },
        LocalParams {
            moire: f32::INFINITY,
            ..Default::default()
        },
        LocalParams {
            defringe: 1.,
            ..Default::default()
        },
        LocalParams {
            color_overlay: Some([0., 50.]),
            ..Default::default()
        },
    ] {
        assert!(adjust_local(&image, &p, 100.).is_err());
    }
    assert!(blend_local(&image, &image, &[0.; 3]).is_err());
    assert!(blend_local(&image, &image, &[-1.; 4]).is_err());
    assert!(
        blend_local(
            &image,
            &Image::new(2, 2, image.planes().to_vec()).unwrap(),
            &[1.; 4]
        )
        .is_err()
    );
    let mono = Image::new(4, 1, vec![vec![0.1; 4]]).unwrap();
    assert!(adjust_local(&mono, &LocalParams::default(), 100.).is_err());
    assert!(blend_local(&mono, &image, &[1.; 4]).is_err());
}
#[test]
fn moire_is_a_documented_neutral_placeholder() {
    let image = input();
    let out = adjust_local(
        &image,
        &LocalParams {
            moire: 100.,
            ..Default::default()
        },
        100.,
    )
    .unwrap();
    assert_eq!(out.planes(), image.planes());
}
