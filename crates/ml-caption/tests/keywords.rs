use ml_caption::{Calibration, rank_keywords};

#[test]
fn calibrated_multilabel_ranking_is_stable_and_validated() {
    let labels = vec!["red".to_owned(), "circle".to_owned(), "blue".to_owned()];
    let c = Calibration {
        temperature: 0.1,
        midpoint: 0.2,
    };
    let ranked = rank_keywords(&labels, &[0.4, 0.4, -0.1], c).unwrap();
    assert_eq!(
        ranked.iter().map(|x| x.0.as_str()).collect::<Vec<_>>(),
        ["circle", "red", "blue"]
    );
    assert!((ranked[0].1 - 0.880797).abs() < 0.00001);
    assert!(ranked[2].1 < 0.05);
    assert!(rank_keywords(&labels, &[f32::NAN, 0., 0.], c).is_err());
    assert!(rank_keywords(&labels, &[0.], c).is_err());
    assert!(
        rank_keywords(
            &labels,
            &[0.; 3],
            Calibration {
                temperature: 0.,
                ..c
            }
        )
        .is_err()
    );
}
