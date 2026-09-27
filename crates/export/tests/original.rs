use engine_api::{
    jobs::CancellationToken,
    recipe::{EditMeta, Recipe},
};
use export::export_original;
use sidecar::{Sidecar, XmpPacket};

fn recipe() -> Recipe {
    let mut recipe = Recipe::default();
    recipe
        .edit(EditMeta::user("exposure", 0), |s| s.tone.exposure = 1.25)
        .unwrap();
    recipe
}

#[test]
fn original_bytes_and_foreign_xmp_survive_recipe_merge() {
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("source.nef");
    let dest = dir.path().join("out/copy.nef");
    let bytes = b"opaque original RAW bytes\0\xff";
    std::fs::write(&source, bytes).unwrap();
    let packet = XmpPacket::parse(r#"<rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#"><rdf:Description xmlns:f="urn:foreign" f:owner="Keep Me"/></rdf:RDF>"#).unwrap();
    let source_packet = packet.serialize().to_owned();
    export_original(
        &source,
        &dest,
        Some(&recipe()),
        Some(&packet),
        &CancellationToken::new(),
    )
    .unwrap();
    assert_eq!(std::fs::read(&dest).unwrap(), bytes);
    assert_eq!(std::fs::read(&source).unwrap(), bytes);
    assert_eq!(packet.serialize(), source_packet);
    let copied = Sidecar::read_xmp(Sidecar::paths(&dest).xmp).unwrap();
    assert!(copied.serialize().contains("Keep Me"));
    assert_eq!(
        copied.to_recipe().unwrap().recipe.settings.tone.exposure,
        1.25
    );
}

#[test]
fn dng_copy_preserves_samples_and_merges_embedded_xmp() {
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("source.dng");
    let dest = dir.path().join("copy.dng");
    let image = merge::LinearImage {
        width: 2,
        height: 1,
        pixels: vec![[0.1, 0.2, 0.3], [1.2, 0.4, 0.5]],
        color_matrix: [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]],
        as_shot_neutral: [1.0; 3],
    };
    let packet = r#"<rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#"><rdf:Description xmlns:f="urn:foreign" f:owner="Embedded Owner"/></rdf:RDF>"#;
    merge::dng::write(&mut std::fs::File::create(&source).unwrap(), &image, packet).unwrap();
    let original = std::fs::read(&source).unwrap();
    export_original(
        &source,
        &dest,
        Some(&recipe()),
        None,
        &CancellationToken::new(),
    )
    .unwrap();
    let decoded = raw_decode::linear_dng::read(&mut std::fs::File::open(&dest).unwrap()).unwrap();
    assert_eq!(decoded.pixels, image.pixels);
    let copied = XmpPacket::parse(decoded.xmp).unwrap();
    assert!(copied.serialize().contains("Embedded Owner"));
    assert_eq!(
        copied.to_recipe().unwrap().recipe.settings.tone.exposure,
        1.25
    );
    assert!(!Sidecar::paths(&dest).xmp.exists());
    assert_eq!(std::fs::read(&source).unwrap(), original);
}

#[test]
fn original_copy_never_overwrites_or_publishes_on_cancel() {
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("source.nef");
    let dest = dir.path().join("copy.nef");
    std::fs::write(&source, b"original").unwrap();
    let cancel = CancellationToken::new();
    cancel.cancel();
    assert!(export_original(&source, &dest, Some(&recipe()), None, &cancel).is_err());
    assert!(!dest.exists());
    assert!(!Sidecar::paths(&dest).xmp.exists());
    std::fs::write(&dest, b"existing").unwrap();
    assert!(
        export_original(
            &source,
            &dest,
            Some(&recipe()),
            None,
            &CancellationToken::new()
        )
        .is_err()
    );
    assert_eq!(std::fs::read(&dest).unwrap(), b"existing");
    assert!(!Sidecar::paths(&dest).xmp.exists());
    assert!(
        export_original(
            &source,
            &source,
            Some(&recipe()),
            None,
            &CancellationToken::new()
        )
        .is_err()
    );
    assert_eq!(std::fs::read(&source).unwrap(), b"original");
}
