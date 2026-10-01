use import_lrcat::{lua_develop, xmp};

#[test]
fn legacy_ca_lua_and_xmp_translate_independently() {
    for (key, field) in [
        ("ChromaticAberrationR", "legacy_ca_red"),
        ("ChromaticAberrationB", "legacy_ca_blue"),
    ] {
        for amount in [-100, -25, 0, 35, 100] {
            let lua = format!("s = {{ {key} = {amount} }}");
            let xml = format!(
                r#"<rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#" xmlns:crs="http://ns.adobe.com/camera-raw-settings/1.0/"><rdf:Description crs:{key}="{amount}"/></rdf:RDF>"#
            );
            for (recipe, warnings) in [
                lua_develop::parse(&lua, "5.7").unwrap(),
                xmp::parse(&xml, "5.7").unwrap(),
            ] {
                let value = serde_json::to_value(&recipe).unwrap();
                assert_eq!(
                    value["settings"]["lens"][field].as_f64(),
                    Some(amount as f64),
                    "{key}: {warnings:?}"
                );
                assert!(!recipe.unknown.contains_key(&format!("crs:{key}")));
                assert!(!warnings.iter().any(|w| w.contains(key)), "{warnings:?}");
                assert_eq!(
                    recipe.to_json().unwrap(),
                    engine_api::recipe::Recipe::from_json(&recipe.to_json().unwrap())
                        .unwrap()
                        .to_json()
                        .unwrap()
                );
            }
        }
    }
}

#[test]
fn invalid_legacy_ca_is_retained() {
    for value in ["101", "-101", "'invalid'", "{ 1, 2 }"] {
        let (recipe, warnings) =
            lua_develop::parse(&format!("s = {{ ChromaticAberrationR = {value} }}"), "5.7")
                .unwrap();
        assert!(
            serde_json::to_value(&recipe).unwrap()["settings"]["lens"]
                .get("legacy_ca_red")
                .is_none()
        );
        assert!(!warnings.is_empty());
        assert!(
            recipe
                .to_json()
                .unwrap()
                .windows("ChromaticAberrationR".len())
                .any(|v| v == b"ChromaticAberrationR")
        );
    }
}

#[test]
fn cloud_wording_is_verbatim() {
    for key in [
        "EnableDistractionRemoval",
        "GenerativeRemove",
        "GenerativeFill",
    ] {
        let (_, warnings) = lua_develop::parse(&format!("s = {{ {key} = true }}"), "15.4").unwrap();
        assert!(
            warnings
                .iter()
                .any(|w| w.contains("requires Adobe cloud; not translatable")),
            "{warnings:?}"
        );
    }
}
