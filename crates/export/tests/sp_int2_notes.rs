//! SP-INT2 (REV-SP-A nits): proxy notes are sentences and keep the
//! embedded-profile substitution note (LR-8e2) on export.
use engine_api::recipe::{EditMeta, ProcessVersion, Recipe};
use pipeline_cpu::{CameraLinearProxy, RenderSource};

#[test]
fn sp_int2_proxy_notes_are_sentences_and_keep_the_profile_note() {
    let bytes = include_bytes!("../../raw-decode/tests/fixtures/linear-gradient-jxl.dng");
    let proxy = CameraLinearProxy::from_dng(
        raw_decode::lossy_dng::read(&mut std::io::Cursor::new(bytes))
            .unwrap()
            .unwrap(),
    )
    .unwrap()
    .with_embedded_profile(Some(bytes.to_vec()));
    let mut recipe = Recipe {
        process_version: ProcessVersion::adobe(6),
        ..Default::default()
    };
    recipe
        .edit(EditMeta::user("look", 1), |s| {
            s.camera_profile.profile.name = "Adobe Color".into();
            s.camera_profile.look = Some(
                serde_json::from_value(serde_json::json!({"style":"unavailable","amount":100.0}))
                    .unwrap(),
            );
        })
        .unwrap();
    let notes = export::proxy_notes(&RenderSource::CameraLinear(&proxy), &recipe, false, false);
    assert!(notes[0].contains("Smart Preview"), "{notes:?}");
    assert!(
        notes
            .iter()
            .any(|n| n.contains("Creative look unavailable")),
        "{notes:?}"
    );
    assert!(notes.iter().all(|n| !n.contains(" /")), "{notes:?}");
    assert!(
        notes.iter().any(
            |n| n.contains(image_core::pipeline_adobe::SUBSTITUTED_PROFILE_NOTICE)
                || n.contains(image_core::pipeline_adobe::UNAVAILABLE_PROFILE_NOTICE)
        ),
        "the embedded-profile note survives on export: {notes:?}"
    );
    recipe.process_version = ProcessVersion::NATIVE_CURRENT;
    let native = export::proxy_notes(&RenderSource::CameraLinear(&proxy), &recipe, false, false);
    assert!(
        native.iter().all(|n| !n.contains("profile")),
        "Native never substitutes the embedded profile: {native:?}"
    );
}
