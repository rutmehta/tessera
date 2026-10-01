//! LR-SCHEMA pin: an envelope newer than this build can write reads
//! best-effort; every sidecar write path refuses it. Written against
//! `max_writable_schema_version() + 1`, so it holds once schema 4 lands.
use engine_api::recipe::{Selection, max_writable_schema_version};
use sidecar::{MarkPreset, RecipeDocument, Sidecar, XmpPacket};

#[test]
fn newer_than_writable_envelope_reads_and_every_write_refuses() {
    // std temp dir: sidecar has no tempfile dev-dependency (avoids a lockfile change).
    let dir = std::env::temp_dir().join(format!("tessera-schema-v4-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("photo.json");
    let newer = max_writable_schema_version() + 1;
    let bytes = format!(
        r#"{{"recipe":{{"schema_version":{newer},"lens_blur_future":{{"amount":3}}}},"vector_clock":{{"mac":2}}}}"#
    );
    std::fs::write(&path, &bytes).unwrap();

    let doc = Sidecar::read_recipe(&path).unwrap();
    assert_eq!(doc.recipe.schema_version, newer);
    assert_eq!(doc.recipe.unknown["lens_blur_future"]["amount"], 3);
    assert_eq!(doc.vector_clock["mac"], 2);

    assert!(Sidecar::write_recipe(&path, &doc).is_err());
    assert_eq!(std::fs::read(&path).unwrap(), bytes.as_bytes());
    let fresh = dir.join("fresh.json");
    assert!(Sidecar::write_recipe(&fresh, &doc).is_err());
    assert!(!fresh.exists());

    let packet = XmpPacket::from_selection(&Selection::default(), &MarkPreset::default());
    assert!(packet.with_recipe(&doc.recipe).is_err());

    // The envelope's direct serde projection is not a write path; it keeps the version.
    let projected: serde_json::Value = serde_json::to_value(&doc).unwrap();
    assert_eq!(projected["recipe"]["schema_version"], newer);
    let decoded: RecipeDocument = serde_json::from_value(projected).unwrap();
    assert_eq!(decoded.recipe, doc.recipe);
    std::fs::remove_dir_all(dir).unwrap();
}
