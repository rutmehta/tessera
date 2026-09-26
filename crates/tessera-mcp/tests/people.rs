use engine_api::{id::ImageId, tools::LibraryToolRequest};
use serde_json::{Value, json};
use tessera_mcp::Console;

fn request(value: Value) -> LibraryToolRequest {
    serde_json::from_value(value).unwrap()
}

#[test]
fn library_tools_are_advertised_with_opt_in_defaults() {
    let tools = tessera_mcp::schema::tools();
    for name in engine_api::tools::LibraryToolCall::NAMES {
        let entries: Vec<_> = tools.iter().filter(|t| t["name"] == name).collect();
        assert_eq!(entries.len(), 1, "{name}");
        let props = &entries[0]["inputSchema"]["properties"];
        assert!(props.get("writes").is_some(), "{name}");
    }
    let call = request(json!({"tool":"name_person","person_id":1,"name":null}));
    let engine_api::tools::LibraryToolCall::NamePerson { writes, .. } = call.call else {
        panic!()
    };
    assert!(!writes.write_sidecars && !writes.person_keywords);
}

fn fixture() -> (tempfile::TempDir, Console, index::Index, ImageId) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("face.jpg");
    image::RgbImage::new(32, 32).save(&path).unwrap();
    let mut console = Console::open(dir.path().join("app")).unwrap();
    let id = console.open_image(path).unwrap();
    let index = index::Index::open(dir.path().join("app/index.sqlite")).unwrap();
    index.create_person("1", None, None).unwrap();
    index.create_person("2", Some("Grace"), None).unwrap();
    index
        .replace_faces(
            id,
            &[index::FaceRecord {
                id: 0,
                bbox: [0., 0., 16., 16.],
                landmarks5: [[0.; 2]; 5],
                confidence: 0.1,
                embedding: None,
                sharpness: 0.,
                eyes_open: None,
            }],
        )
        .unwrap();
    (dir, console, index, id)
}

#[test]
fn assign_descriptorless_face_and_name_without_sidecars() {
    let (dir, mut console, index, id) = fixture();
    console.start_recording("People").unwrap();
    console
        .run_library(request(json!({"tool":"assign_person", "person_id":1,
        "faces":[{"image_id":id,"ordinal":0}]})))
        .unwrap();
    console
        .run_library(request(
            json!({"tool":"name_person", "person_id":1,"name":"Ada"}),
        ))
        .unwrap();
    let assigned = index.face_assignments(id).unwrap();
    assert_eq!(assigned[0].person_name.as_deref(), Some("Ada"));
    assert!(!assigned[0].confirmed);
    let action = console.stop_recording().unwrap();
    assert_eq!(action.steps.len(), 2);
    assert!(!dir.path().join("face.jpg.xmp").exists());
    assert!(!dir.path().join("face.xmp").exists());
    console
        .run_library(request(
            json!({"tool":"name_person", "person_id":1,"name":null}),
        ))
        .unwrap();
    assert_eq!(index.people().unwrap()[0].name, None);
    let replay = console.play_action(&action, &[]).unwrap();
    assert!(replay.ok(), "{replay:?}");
    assert_eq!(
        index.face_assignments(id).unwrap()[0]
            .person_name
            .as_deref(),
        Some("Ada")
    );
}

#[test]
fn confirm_merge_split_preserve_identity_contracts() {
    let (_dir, mut console, index, id) = fixture();
    let face = json!({"image_id":id,"ordinal":0});
    console
        .run_library(request(
            json!({"tool":"assign_person","person_id":1,"faces":[face]}),
        ))
        .unwrap();
    console
        .run_library(request(
            json!({"tool":"confirm_person","person_id":1,"faces":[face],"confirmed":true}),
        ))
        .unwrap();
    assert!(index.face_assignments(id).unwrap()[0].confirmed);
    console
        .run_library(request(
            json!({"tool":"merge_people","source":1,"target":2}),
        ))
        .unwrap();
    let assignments = index.face_assignments(id).unwrap();
    assert_eq!(assignments[0].person_id, "2");
    assert_eq!(assignments[0].person_name.as_deref(), Some("Grace"));
    assert!(assignments[0].confirmed);
    let result = console
        .run_library(request(
            json!({"tool":"split_person","person_id":2,"faces":[face]}),
        ))
        .unwrap();
    let new_id = result["person_id"].as_u64().unwrap().to_string();
    let assignments = index.face_assignments(id).unwrap();
    assert_eq!(assignments[0].person_id, new_id);
    assert_eq!(assignments[0].person_name, None);
    assert!(!assignments[0].confirmed);
    assert!(index.people().unwrap().iter().any(|p| p.id == "2"));
}

#[test]
fn invalid_references_are_rejected_before_any_edits() {
    let (_dir, mut console, index, id) = fixture();
    let face = json!({"image_id":id,"ordinal":0});
    let missing = json!({"image_id":id,"ordinal":99});
    console
        .run_library(request(
            json!({"tool":"assign_person","person_id":1,"faces":[face]}),
        ))
        .unwrap();
    let before = index.face_assignments(id).unwrap();
    let people = index.people().unwrap();
    for bad in [
        json!({"tool":"assign_person","person_id":2,"faces":[face,missing]}),
        json!({"tool":"assign_person","person_id":99,"faces":[face]}),
        json!({"tool":"assign_person","person_id":2,"faces":[face,face]}),
        json!({"tool":"assign_person","person_id":2,"faces":[]}),
        json!({"tool":"confirm_person","person_id":1,"faces":[face,missing],"confirmed":true}),
        json!({"tool":"confirm_person","person_id":2,"faces":[face],"confirmed":true}),
        json!({"tool":"merge_people","source":1,"target":1}),
        json!({"tool":"merge_people","source":1,"target":99}),
        json!({"tool":"merge_people","source":99,"target":1}),
        json!({"tool":"split_person","person_id":1,"faces":[face,missing]}),
        json!({"tool":"split_person","person_id":2,"faces":[face]}),
        json!({"tool":"split_person","person_id":1,"faces":[]}),
        json!({"tool":"split_person","person_id":1,"faces":[face,face]}),
        json!({"tool":"name_person","person_id":99,"name":"Missing"}),
        json!({"tool":"assign_person","person_id":2,"faces":[face],"writes":{"write_sidecars":true}}),
    ] {
        assert!(
            console.run_library(request(bad.clone())).is_err(),
            "accepted {bad}"
        );
        assert_eq!(
            index.face_assignments(id).unwrap(),
            before,
            "changed faces for {bad}"
        );
        assert_eq!(index.people().unwrap(), people, "changed people for {bad}");
    }
}

#[test]
fn model_generated_string_ids_have_stable_wire_ids_across_reopen() {
    let (dir, mut c, index, image) = fixture();
    index
        .create_person("face-cluster-uuid", Some("Model cluster"), None)
        .unwrap();
    let id = c
        .people()
        .unwrap()
        .into_iter()
        .find(|p| p.name.as_deref() == Some("Model cluster"))
        .unwrap()
        .id;
    c.run_library(request(json!({"tool":"assign_person","person_id":id,"faces":[{"image_id":image,"ordinal":0}],"confirmed":true}))).unwrap();
    assert_eq!(
        index.face_assignments(image).unwrap()[0].person_id,
        "face-cluster-uuid"
    );
    drop(c);
    index.create_person("aaa", None, None).unwrap();
    let c = Console::open(dir.path().join("app")).unwrap();
    let p = c
        .people()
        .unwrap()
        .into_iter()
        .find(|p| p.name.as_deref() == Some("Model cluster"))
        .unwrap();
    assert_eq!(p.id, id);
    assert_eq!(p.confirmed_count, 1);
}

#[test]
fn write_options_export_names_and_additive_keywords() {
    let (dir, mut c, index, id) = fixture();
    for signal in ["analysis_width", "analysis_height"] {
        index
            .set_score(
                id,
                &index::Score {
                    signal: signal.into(),
                    value: 32.0,
                    model: "test".into(),
                },
            )
            .unwrap();
    }
    let face = json!({"image_id":id,"ordinal":0});
    c.run_library(request(json!({"tool":"assign_person","person_id":2,"faces":[face],"writes":{"write_sidecars":true,"person_keywords":true}}))).unwrap();
    let path = sidecar::Sidecar::paths(dir.path().join("face.jpg")).xmp;
    let text = std::fs::read_to_string(&path).unwrap();
    assert!(text.contains("Grace"));
    assert_eq!(index.images_with_keyword("Grace").unwrap(), vec![id]);
    c.run_library(request(
        json!({"tool":"name_person","person_id":2,"name":"Ada","writes":{"person_keywords":true}}),
    ))
    .unwrap();
    assert_eq!(index.images_with_keyword("Ada").unwrap(), vec![id]);
    assert_eq!(std::fs::read_to_string(path).unwrap(), text);
    c.run_library(request(json!({"tool":"confirm_person","person_id":2,"faces":[face],"confirmed":true,"writes":{"write_sidecars":true}}))).unwrap();
    assert!(index.face_assignments(id).unwrap()[0].confirmed);
    c.run_library(request(
        json!({"tool":"merge_people","source":2,"target":1,"writes":{"write_sidecars":true}}),
    ))
    .unwrap();
    assert_eq!(index.face_assignments(id).unwrap()[0].person_id, "1");
    c.run_library(request(json!({"tool":"split_person","person_id":1,"faces":[face],"writes":{"write_sidecars":true}}))).unwrap();
    assert!(!index.face_assignments(id).unwrap()[0].confirmed);
}

#[test]
fn merged_opaque_identity_cannot_resurrect_an_old_wire_id() {
    let (_dir, mut c, index, _) = fixture();
    index
        .create_person("model-key", Some("Generated"), None)
        .unwrap();
    let old = c
        .people()
        .unwrap()
        .into_iter()
        .find(|p| p.name.as_deref() == Some("Generated"))
        .unwrap()
        .id;
    c.run_library(request(
        json!({"tool":"merge_people","source":old,"target":1}),
    ))
    .unwrap();
    index
        .create_person("model-key", Some("New detection"), None)
        .unwrap();
    let new = c
        .people()
        .unwrap()
        .into_iter()
        .find(|p| p.name.as_deref() == Some("New detection"))
        .unwrap()
        .id;
    assert_ne!(old, new);
    assert!(
        c.run_library(request(
            json!({"tool":"name_person","person_id":old,"name":"Stale"})
        ))
        .is_err()
    );
}
