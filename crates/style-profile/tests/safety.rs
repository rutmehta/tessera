use engine_api::{
    id::ImageId,
    recipe::{history::EditMeta, Recipe},
};
use style_profile::{
    apply_batch, group_amount, BatchImage, Features, Profile, Questionnaire, RecipeStore,
    SidecarStore,
};
#[test]
fn amount_endpoints_noops_and_invalid_batch_are_safe() {
    let p = Profile::new(
        "lib",
        Questionnaire {
            brightness: 1.,
            ..Default::default()
        },
    )
    .unwrap();
    let input = BatchImage {
        image: ImageId(1),
        features: Features::default(),
        burst: None,
        people: vec![],
    };
    let mut recipes = vec![Recipe::default()];
    let before = recipes.clone();
    assert!(apply_batch(&p, std::slice::from_ref(&input), &mut recipes, f64::NAN, 1).is_err());
    assert_eq!(recipes, before);
    let queue = apply_batch(&p, std::slice::from_ref(&input), &mut recipes, 0., 1).unwrap();
    assert_eq!(recipes[0].settings, before[0].settings);
    assert_eq!(recipes[0].history.entries.len(), 1);
    let group = group_amount(&recipes[0], queue[0].group).unwrap();
    assert_eq!(group.settings_at(0.5).unwrap().tone.exposure, 0.5);
    let mut duplicates = vec![Recipe::default(); 2];
    let old = duplicates.clone();
    assert!(apply_batch(&p, &[input.clone(), input], &mut duplicates, 1., 1).is_err());
    assert_eq!(duplicates, old);
}
#[test]
fn sidecar_store_refuses_stale_edits_and_colliding_images() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("a.jpg");
    std::fs::write(&path, []).unwrap();
    let mut store = SidecarStore::new([(ImageId(1), path.clone())], "machine").unwrap();
    let expected = store.load(ImageId(1)).unwrap();
    let mut doc = sidecar::RecipeDocument::default();
    doc.recipe
        .edit(EditMeta::user("manual", 1), |s| s.tone.exposure = 3.)
        .unwrap();
    let file = sidecar::Sidecar::paths(&path).recipe;
    sidecar::Sidecar::write_recipe(&file, &doc).unwrap();
    let bytes = std::fs::read(&file).unwrap();
    let mut next = expected.clone();
    next.image_id = Some(ImageId(1));
    assert!(store.commit(ImageId(1), &expected, &next, 2).is_err());
    assert_eq!(std::fs::read(file).unwrap(), bytes);
    std::fs::write(dir.path().join("a.dng"), []).unwrap();
    assert!(SidecarStore::new([(ImageId(1), path)], "machine").is_err());
}
#[test]
fn store_rejects_directories() {
    let dir = tempfile::tempdir().unwrap();
    assert!(SidecarStore::new([(ImageId(1), dir.path().to_owned())], "machine").is_err());
}
