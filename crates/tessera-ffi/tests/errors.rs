use tessera_ffi::{Decision, Engine, ImageQuery, Selection};

#[test]
fn invalid_inputs_do_not_mutate_sidecars() {
    let dir = tempfile::tempdir().unwrap();
    image::RgbImage::new(8, 8)
        .save(dir.path().join("one.jpg"))
        .unwrap();
    let engine = Engine::open(dir.path().join("db").to_string_lossy().into_owned()).unwrap();
    assert!(
        engine
            .index_folder(dir.path().join("missing").to_string_lossy().into_owned())
            .is_err()
    );
    engine
        .index_folder(dir.path().to_string_lossy().into_owned())
        .unwrap();
    let row = engine.list_images(ImageQuery::default()).unwrap().remove(0);
    assert!(engine.get_recipe("wrong-id".into()).is_err());
    assert!(
        engine
            .get_recipe("00000000000000000000000000000000".into())
            .is_err()
    );
    assert!(
        engine
            .set_selection(
                row.id.clone(),
                Selection {
                    decision: Decision::Keep,
                    grade: Some(4),
                    mark: None
                }
            )
            .is_err()
    );
    assert!(
        !sidecar::Sidecar::paths(dir.path().join("one.jpg"))
            .recipe
            .exists()
    );
    engine
        .set_selection(
            row.id.clone(),
            Selection {
                decision: Decision::Reject,
                grade: Some(3),
                mark: Some("Review".into()),
            },
        )
        .unwrap();
    let rows = engine
        .list_images(ImageQuery {
            decision: Some(Decision::Reject),
            ..Default::default()
        })
        .unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].selection.grade, None);
    assert_eq!(rows[0].recipe_hash, row.recipe_hash);
    let xmp = sidecar::Sidecar::read_xmp(sidecar::Sidecar::paths(dir.path().join("one.jpg")).xmp)
        .unwrap();
    assert_eq!(
        xmp.selection().unwrap().decision,
        engine_api::recipe::Decision::Reject
    );
    let original = engine.get_recipe(row.id.clone()).unwrap();
    let mut future: serde_json::Value = serde_json::from_str(&original).unwrap();
    future["schema_version"] = 999.into();
    assert!(
        engine
            .set_recipe_json(row.id.clone(), future.to_string())
            .is_err()
    );
    assert_eq!(engine.get_recipe(row.id).unwrap(), original);
}
