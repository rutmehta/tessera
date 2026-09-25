use engine_api::{
    id::ImageId,
    recipe::{
        history::{Author, EditMeta},
        DevelopSettings, Recipe,
    },
};
use style_profile::{Features, Profile, Questionnaire, ReferenceEdit};
#[test]
fn collect_only_active_user_edits_reference_and_persist() {
    let mut user = Recipe::default();
    user.edit(EditMeta::user("Exposure", 1), |s| s.tone.exposure = 1.)
        .unwrap();
    let mut agent = Recipe::default();
    agent
        .edit(
            EditMeta {
                author: Author::Agent {
                    name: "test".into(),
                },
                ..EditMeta::default()
            },
            |s| s.tone.exposure = 2.,
        )
        .unwrap();
    let mut undone = user.clone();
    undone.undo().unwrap();
    let mut p = Profile::new("library", Questionnaire::default()).unwrap();
    let f = Features::default();
    p.collect([
        (ImageId(1), f.clone(), user),
        (ImageId(2), f.clone(), agent),
        (ImageId(3), f.clone(), undone),
    ])
    .unwrap();
    assert_eq!(p.sample_count(), 1);
    let mut after = DevelopSettings::default();
    after.tone.contrast = 30.;
    p.seed_references(&[ReferenceEdit {
        image: ImageId(4),
        features: f.clone(),
        before: DevelopSettings::default(),
        after,
    }])
    .unwrap();
    assert_eq!(p.sample_count(), 2);
    let dir = tempfile::tempdir().unwrap();
    p.save(dir.path()).unwrap();
    let loaded = Profile::open(dir.path(), "library").unwrap().unwrap();
    assert_eq!(
        p.predict(&f).unwrap().settings,
        loaded.predict(&f).unwrap().settings
    );
    assert_eq!(loaded.sample_count(), 2);
    assert!(Profile::open(dir.path(), "another").unwrap().is_none());
    let path = Profile::storage_path(dir.path(), "library").unwrap();
    let mut json: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    json["version"] = serde_json::json!(99);
    std::fs::write(&path, serde_json::to_vec(&json).unwrap()).unwrap();
    assert!(Profile::open(dir.path(), "library").is_err());
}
