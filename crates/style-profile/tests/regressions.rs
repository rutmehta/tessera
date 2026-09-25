use engine_api::{
    id::ImageId,
    recipe::{history::EditMeta, settings::WhiteBalanceMode, DevelopSettings, Recipe},
};
use style_profile::{apply_batch, BatchImage, Features, Profile, Questionnaire};
#[test]
fn burst_consistency_includes_wb_mode_not_only_numbers() {
    let p = Profile::new("lib", Questionnaire::default()).unwrap();
    let images = (0..2)
        .map(|i| BatchImage {
            image: ImageId(i),
            features: Features::default(),
            burst: Some("burst".into()),
            people: vec![],
        })
        .collect::<Vec<_>>();
    let mut recipes = vec![Recipe::default(); 2];
    recipes[0]
        .edit(EditMeta::user("wb", 0), |s| {
            s.white_balance.mode = WhiteBalanceMode::Daylight
        })
        .unwrap();
    apply_batch(&p, &images, &mut recipes, 1., 1).unwrap();
    assert_eq!(
        recipes[0].settings.white_balance,
        recipes[1].settings.white_balance
    );
}
#[test]
fn invalid_feedback_and_model_do_not_poison_the_profile() {
    let mut p = Profile::new("lib", Questionnaire::default()).unwrap();
    let f = Features::default();
    p.record_feedback(ImageId(1), &f, &DevelopSettings::default())
        .unwrap();
    let before = p.predict(&f).unwrap().settings;
    let bad = Features {
        embedding_model: "wrong".into(),
        ..f.clone()
    };
    assert!(p
        .record_feedback(ImageId(2), &bad, &DevelopSettings::default())
        .is_err());
    assert_eq!(p.sample_count(), 1);
    assert_eq!(p.predict(&f).unwrap().settings, before);
    let mut json = serde_json::to_value(&p).unwrap();
    json["fitted"]["weights"] = serde_json::json!([]);
    assert!(serde_json::from_value::<Profile>(json).is_err());
}
