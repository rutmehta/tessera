use engine_api::{
    id::{Digest, ImageId},
    pinned_raw::{PinnedRawDecoderRoute, PinnedRawDescriptor, PinnedRawInput},
    recipe::Recipe,
};

fn input(recipe_json: Vec<u8>) -> PinnedRawInput {
    PinnedRawInput {
        asset_digest: Digest::derive("tessera pinned RAW asset v1", b"fixture bytes"),
        asset_byte_len: 13,
        recipe_image_id: ImageId(1),
        recipe_json,
        decoder_route: PinnedRawDecoderRoute::LibRawCfaV1,
        suffix_hint: "arw".into(),
        locator_hint: Some("/not-authoritative/camera.arw".into()),
    }
}
fn bytes() -> Vec<u8> {
    Recipe::new(ImageId(1)).to_json().unwrap()
}

#[test]
fn roundtrip_preserves_exact_recipe_and_identity() {
    let b = bytes();
    let d = PinnedRawDescriptor::new(input(b.clone())).unwrap();
    let reopened = PinnedRawDescriptor::from_json(&d.to_json().unwrap()).unwrap();
    assert_eq!(reopened.recipe_json(), b.as_slice());
    assert_eq!(reopened.input_identity(), d.input_identity());
}
#[test]
fn locator_does_not_affect_identity_but_asset_and_owner_do() {
    let b = bytes();
    let first = PinnedRawDescriptor::new(input(b.clone())).unwrap();
    let mut other = input(b.clone());
    other.locator_hint = Some("elsewhere.arw".into());
    assert_eq!(
        first.input_identity(),
        PinnedRawDescriptor::new(other).unwrap().input_identity()
    );
    let mut other = input(b);
    other.asset_digest = Digest::derive("asset", b"different");
    assert_ne!(
        first.input_identity(),
        PinnedRawDescriptor::new(other).unwrap().input_identity()
    );
    let mut other = bytes();
    let mut recipe: serde_json::Value = serde_json::from_slice(&other).unwrap();
    recipe["image_id"] = serde_json::to_value(ImageId(2)).unwrap();
    other = serde_json::to_vec(&recipe).unwrap();
    assert!(PinnedRawDescriptor::new(input(other)).is_err());
}
#[test]
fn explicit_schema_and_owner_are_required() {
    let mut v: serde_json::Value = serde_json::from_slice(&bytes()).unwrap();
    for version in [2, 4] {
        v["schema_version"] = version.into();
        assert!(PinnedRawDescriptor::new(input(serde_json::to_vec(&v).unwrap())).is_err());
    }
    v.as_object_mut().unwrap().remove("schema_version");
    assert!(PinnedRawDescriptor::new(input(serde_json::to_vec(&v).unwrap())).is_err());
}
#[test]
fn raw_snapshot_headers_and_default_geometry_are_required() {
    let mut v: serde_json::Value = serde_json::from_slice(&bytes()).unwrap();
    v["source_kind"] = "rgb".into();
    assert!(PinnedRawDescriptor::new(input(serde_json::to_vec(&v).unwrap())).is_err());
    v = serde_json::from_slice(&bytes()).unwrap();
    v["process_version"]["family"] = "adobe".into();
    assert!(PinnedRawDescriptor::new(input(serde_json::to_vec(&v).unwrap())).is_err());
    v = serde_json::from_slice(&bytes()).unwrap();
    v["settings"]["geometry"]["orientation"] = 2.into();
    assert!(PinnedRawDescriptor::new(input(serde_json::to_vec(&v).unwrap())).is_err());
}
#[test]
fn zero_length_bad_suffix_and_duplicate_keys_reject() {
    let mut i = input(bytes());
    i.asset_byte_len = 0;
    assert!(PinnedRawDescriptor::new(i).is_err());
    let mut i = input(bytes());
    i.suffix_hint = "../arw".into();
    assert!(PinnedRawDescriptor::new(i).is_err());
    let valid = String::from_utf8(bytes()).unwrap();
    let duplicate = valid.replacen(
        "\"schema_version\": 3",
        "\"schema_version\": 3, \"schema_version\": 3",
        1,
    );
    assert!(PinnedRawDescriptor::new(input(duplicate.into_bytes())).is_err());
    let escaped = valid.replacen(
        "\"schema_version\": 3",
        "\"schema_version\": 3, \"schema_\\u0076ersion\": 3",
        1,
    );
    assert!(PinnedRawDescriptor::new(input(escaped.into_bytes())).is_err());
}

#[test]
fn nested_unknowns_and_extreme_numbers_reject() {
    let mut v: serde_json::Value = serde_json::from_slice(&bytes()).unwrap();
    v["settings"]["tone"]["future"] = true.into();
    assert!(PinnedRawDescriptor::new(input(serde_json::to_vec(&v).unwrap())).is_err());
    let raw = String::from_utf8(bytes())
        .unwrap()
        .replace("\"exposure\": 0.0", "\"exposure\": 1e100");
    assert!(PinnedRawDescriptor::new(input(raw.into_bytes())).is_err());
}

#[test]
fn descriptor_wire_and_current_headers_fail_closed() {
    let d = PinnedRawDescriptor::new(input(bytes())).unwrap();
    let mut wire: serde_json::Value = serde_json::from_slice(&d.to_json().unwrap()).unwrap();
    wire["descriptor_version"] = 2.into();
    assert!(PinnedRawDescriptor::from_json(&serde_json::to_vec(&wire).unwrap()).is_err());
    wire["descriptor_version"] = 1.into();
    wire["decoder_route"] = "future_route".into();
    assert!(PinnedRawDescriptor::from_json(&serde_json::to_vec(&wire).unwrap()).is_err());
    wire["decoder_route"] = "lib_raw_cfa_v1".into();
    wire["recipe_hash"] = "00".repeat(32).into();
    assert!(PinnedRawDescriptor::from_json(&serde_json::to_vec(&wire).unwrap()).is_err());
    let mut v: serde_json::Value = serde_json::from_slice(&bytes()).unwrap();
    v.as_object_mut().unwrap().remove("settings");
    assert!(PinnedRawDescriptor::new(input(serde_json::to_vec(&v).unwrap())).is_err());
    v = serde_json::from_slice(&bytes()).unwrap();
    v.as_object_mut().unwrap().remove("source_kind");
    assert!(PinnedRawDescriptor::new(input(serde_json::to_vec(&v).unwrap())).is_err());
    v = serde_json::from_slice(&bytes()).unwrap();
    v["process_version"]["revision"] = 1.into();
    assert!(PinnedRawDescriptor::new(input(serde_json::to_vec(&v).unwrap())).is_err());
    v = serde_json::from_slice(&bytes()).unwrap();
    v["settings"]["geometry"]["orientation"] = 2.into();
    assert!(PinnedRawDescriptor::new(input(serde_json::to_vec(&v).unwrap())).is_err());
}
#[test]
fn suffix_normalizes_and_recipe_history_is_opaque() {
    let mut i = input(bytes());
    i.suffix_hint = "ARW".into();
    let mut value: serde_json::Value = serde_json::from_slice(&i.recipe_json).unwrap();
    value["history"] = serde_json::json!("future unparsed history");
    i.recipe_json = serde_json::to_vec(&value).unwrap();
    let d = PinnedRawDescriptor::new(i).unwrap();
    let round = PinnedRawDescriptor::from_json(&d.to_json().unwrap()).unwrap();
    assert_eq!(round.input_identity(), d.input_identity());
}
#[test]
fn duplicate_nested_key_is_rejected() {
    let base = String::from_utf8(bytes()).unwrap();
    let changed = base.replacen("\"tone\": {", "\"tone\": {\"future\": 1, \"future\": 2,", 1);
    assert!(PinnedRawDescriptor::new(input(changed.into_bytes())).is_err());
}

#[test]
fn exact_metadata_and_payload_identity_are_preserved() {
    let raw =
        String::from_utf8(bytes())
            .unwrap()
            .replacen("{", "{\"future_top\": {\"kept\": true},", 1);
    let mut i = input(raw.as_bytes().to_vec());
    let d = PinnedRawDescriptor::new(i.clone()).unwrap();
    assert_eq!(d.recipe_json(), raw.as_bytes());
    let reopened = PinnedRawDescriptor::from_json(&d.to_json().unwrap()).unwrap();
    assert_eq!(reopened.recipe_json(), raw.as_bytes());
    let whitespace = raw.replace(": ", ":");
    i.recipe_json = whitespace.into_bytes();
    assert_ne!(
        PinnedRawDescriptor::new(i).unwrap().input_identity(),
        d.input_identity()
    );
}
#[test]
fn distinct_recipe_owners_have_distinct_descriptors() {
    let a = PinnedRawDescriptor::new(input(bytes())).unwrap();
    let mut value: serde_json::Value = serde_json::from_slice(&bytes()).unwrap();
    value["image_id"] = serde_json::to_value(ImageId(2)).unwrap();
    let mut binput = input(serde_json::to_vec(&value).unwrap());
    binput.recipe_image_id = ImageId(2);
    let b = PinnedRawDescriptor::new(binput).unwrap();
    assert_ne!(a.input_identity(), b.input_identity());
}

#[test]
fn opaque_history_does_not_override_current_settings_and_enum_extensions_reject() {
    let mut v: serde_json::Value = serde_json::from_slice(&bytes()).unwrap();
    v["history"] = serde_json::json!({"head":"unparseable", "future": [1, 2, 3]});
    let d = PinnedRawDescriptor::new(input(serde_json::to_vec(&v).unwrap())).unwrap();
    assert_eq!(d.recipe_json(), serde_json::to_vec(&v).unwrap());
    v["settings"]["denoise"]["method"] = serde_json::json!({"kind":"off", "model":{"id":"future"}});
    assert!(PinnedRawDescriptor::new(input(serde_json::to_vec(&v).unwrap())).is_err());
}
#[test]
fn exact_recipe_bytes_include_whitespace_in_input_identity() {
    let first = bytes();
    let mut second = first.clone();
    second.insert(1, b' ');
    let a = PinnedRawDescriptor::new(input(first)).unwrap();
    let b = PinnedRawDescriptor::new(input(second)).unwrap();
    assert_ne!(a.input_identity(), b.input_identity());
}
