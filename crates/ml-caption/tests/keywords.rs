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

#[test]
fn model_version_is_computable_without_loading_the_model() {
    use ml_caption::{Calibration, keyword_model_version, vocabulary};
    let labels = vocabulary();
    let a = keyword_model_version(&labels, Calibration::default());
    assert_eq!(a, keyword_model_version(&labels, Calibration::default()));
    assert!(a.starts_with(ml_embed::MODEL_VERSION));
    let tuned = Calibration {
        temperature: 0.07,
        ..Calibration::default()
    };
    assert_ne!(a, keyword_model_version(&labels, tuned));
    assert_ne!(
        a,
        keyword_model_version(&labels[1..], Calibration::default())
    );
}
