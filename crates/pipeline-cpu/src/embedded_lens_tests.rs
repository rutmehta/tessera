use super::*;
use crate::Image;
use engine_api::recipe::settings::LensSettings;
use lens::opcodes::GainMap;

fn embedded(op: CorrectionOpcode, stage: usize) -> Embedded {
    let mut e = Embedded {
        size: [8., 8.],
        crop: [1., 1., 6., 6.],
        ..Default::default()
    };
    e.stages[stage].push(op);
    e
}
fn gain() -> GainMap {
    GainMap {
        area: [1, 1, 7, 7],
        plane: 0,
        planes: 1,
        pitch: [2, 2],
        points: [2, 2],
        spacing: [0.5, 0.5],
        origin: [0.25, 0.25],
        map_planes: 1,
        gains: vec![1., 2., 3., 4.],
    }
}
#[test]
fn gain_map_bilinear_origin_pitch_and_area() {
    let e = embedded(CorrectionOpcode::GainMap(gain()), 0);
    let image = Image::new(8, 8, vec![vec![1.; 64]]).unwrap();
    let actual = e
        .apply(
            image,
            0,
            Some(raw_decode::CfaLayout::Bayer([[0, 1], [3, 2]])),
            &LensSettings::default(),
        )
        .unwrap();
    // Pixel (3,3) lies 0.375 along both map axes: 1 + .375 + .75.
    assert_eq!(actual.planes()[0][3 * 8 + 3], 2.125);
    assert_eq!(actual.planes()[0][9], 1.);
    assert_eq!(actual.planes()[0][5 * 8 + 5], 3.625);
    assert_eq!(actual.planes()[0][3 * 8 + 2], 1.);
    assert_eq!(actual.planes()[0][7 * 8 + 7], 1.);
}
#[test]
fn list1_warp_preserves_all_bayer_phases_and_xtrans_phases() {
    for cfa in [
        raw_decode::CfaLayout::Bayer([[0, 1], [3, 2]]),
        raw_decode::CfaLayout::XTrans([[0, 1, 2, 1, 0, 1]; 6]),
    ] {
        let step = if matches!(cfa, raw_decode::CfaLayout::Bayer(_)) {
            2
        } else {
            6
        };
        let mut e = embedded(
            CorrectionOpcode::WarpRectilinear(WarpRectilinear {
                coefficients: vec![
                    [0.8, 0., 0., 0., 0., 0.],
                    [1., 0., 0., 0., 0., 0.],
                    [0.9, 0., 0., 0., 0., 0.],
                ],
                center: [0.5, 0.5],
            }),
            0,
        );
        e.size = [12., 12.];
        e.crop = [1., 1., 10., 10.];
        let plane: Vec<_> = (0..144)
            .map(|i| ((i % 12) % step + ((i / 12) % step) * step) as f32)
            .collect();
        let image = Image::new(12, 12, vec![plane.clone()]).unwrap();
        let actual = e
            .apply(image, 0, Some(cfa), &LensSettings::default())
            .unwrap();
        assert!(
            actual.planes()[0]
                .iter()
                .zip(plane)
                .all(|(a, b)| (a - b).abs() < 1e-6)
        );
    }
}
#[test]
fn file_order_warp_then_vignette_is_not_reversed() {
    let warp = CorrectionOpcode::WarpRectilinear(WarpRectilinear {
        coefficients: vec![[0.5, 0., 0., 0., 0., 0.]],
        center: [0.5, 0.5],
    });
    let vignette = CorrectionOpcode::FixVignetteRadial(FixVignetteRadial {
        coefficients: [1., 0., 0., 0., 0.],
        center: [0.5, 0.5],
    });
    let mut a = embedded(warp.clone(), 2);
    a.stages[2].push(vignette.clone());
    let mut b = embedded(vignette, 2);
    b.stages[2].push(warp);
    let image = Image::new(8, 8, vec![vec![1.; 64]; 3]).unwrap();
    let x = a
        .apply(image.clone(), 2, None, &LensSettings::default())
        .unwrap();
    let y = b.apply(image, 2, None, &LensSettings::default()).unwrap();
    assert!((x.planes()[0][0] - 1.765625).abs() < 1e-6);
    assert!(x.planes()[0][0] > y.planes()[0][0] + 0.4);
}
#[test]
fn invalid_stage_plane_range_fails_closed() {
    let mut g = gain();
    g.plane = 1;
    let e = embedded(CorrectionOpcode::GainMap(g), 0);
    assert!(
        e.apply(
            Image::new(8, 8, vec![vec![1.; 64]]).unwrap(),
            0,
            None,
            &LensSettings::default()
        )
        .is_err()
    );
}
