//! Synthetic LR-3 fixtures. No catalog-derived data.
use engine_api::recipe::{
    MaskKind,
    mask::{RetouchKind, RetouchTarget},
};
use import_lrcat::develop;

const SPOT: &str = "{ centerX=0.25, centerY=0.5, radius=0.0625, sourceX=0.75, sourceY=0.5, spotType='clone', sourceState='sourceSetExplicitly', opacity=0.5, feather=0.5 }";

#[test]
fn lr3_spot_coordinates_offsets_and_units() {
    for key in ["RetouchAreas", "RetouchInfo"] {
        let (r, _) = develop(1, &format!("s = {{ {key} = {{ {SPOT} }} }}"), "15.4").unwrap();
        assert_eq!(r.settings.locals.retouch.len(), 1, "{key}");
        let op = &r.settings.locals.retouch[0];
        assert_eq!(
            op.kind,
            RetouchKind::Clone {
                source_offset: [0.5, 0.0]
            }
        );
        assert_eq!(op.opacity, 50.0);
        let RetouchTarget::Area { components } = &op.target else {
            panic!("area required")
        };
        let MaskKind::Brush { strokes } = &components[0].kind else {
            panic!("width-normalized circle")
        };
        assert_eq!(strokes[0].points, vec![[0.25, 0.5, 1.0]]);
        assert_eq!(strokes[0].radius, 0.0625);
        assert_eq!(strokes[0].feather, 50.0);
        assert!(r.unknown.contains_key("lrcat_develop_source"));
    }
}

#[test]
fn lr3_legacy_string_heal_and_empty_alias() {
    let row = "s = { RetouchAreas = {}, RetouchInfo = { 'centerX = 0.25, centerY = 0.5, radius = 0.0625, sourceX = 0.75, sourceY = 0.5, spotType = heal' } }";
    let (r, _) = develop(1, row, "15.4").unwrap();
    assert_eq!(r.settings.locals.retouch.len(), 1);
    assert_eq!(
        r.settings.locals.retouch[0].kind,
        RetouchKind::Heal {
            source_offset: [0.5, 0.0]
        }
    );
    assert!(r.unknown.contains_key("lrcat_develop_source"));
}

#[test]
fn lr3_brush_dabs_and_source_anchor() {
    let row = "s = { RetouchAreas = {{ SpotType='heal', SourceX=0.75, SourceY=0.5, Opacity=0.6, Feather=0.3, Masks={{ What='Mask/Paint', Radius=0.02, Flow=1, Dabs={'d 0.25 0.5', 'd 0.3 0.6'} }} }} }";
    let (r, _) = develop(1, row, "15.4").unwrap();
    assert_eq!(r.settings.locals.retouch.len(), 1);
    let op = &r.settings.locals.retouch[0];
    assert_eq!(op.opacity, 60.0);
    assert_eq!(
        op.kind,
        RetouchKind::Heal {
            source_offset: [0.5, 0.0]
        }
    );
    let RetouchTarget::Area { components } = &op.target else {
        panic!()
    };
    let MaskKind::Brush { strokes } = &components[0].kind else {
        panic!()
    };
    assert_eq!(strokes[0].points, vec![[0.25, 0.5, 1.0], [0.3, 0.6, 1.0]]);
    assert!(r.unknown.contains_key("lrcat_develop_source"));
}

#[test]
fn lr3_xmp_attributes_and_legacy_items() {
    let xml = r#"<rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#"><rdf:Description xmlns:crs="http://ns.adobe.com/camera-raw-settings/1.0/"><crs:RetouchAreas><rdf:Seq><rdf:li crs:SpotType="clone" crs:CenterX="0.25" crs:CenterY="0.5" crs:Radius="0.0625" crs:SourceX="0.75" crs:SourceY="0.5" crs:Opacity="0.5" /></rdf:Seq></crs:RetouchAreas></rdf:Description></rdf:RDF>"#;
    let (r, _) = develop(1, xml, "15.4").unwrap();
    assert_eq!(r.settings.locals.retouch.len(), 1);
    assert_eq!(r.settings.locals.retouch[0].opacity, 50.0);
    assert!(r.unknown.contains_key("lrcat_develop_source"));
}

#[test]
fn lr3_malformed_or_unrepresentable_key_is_retained_atomically() {
    for bad in [
        "{ 'opaque' }".to_string(),
        format!("{{ {SPOT}, {{spotType='generative'}} }}"),
        format!("{{ {} }}", SPOT.replace("opacity=0.5", "opacity=2")),
    ] {
        let (r, _) = develop(
            1,
            &format!("s = {{ RetouchAreas = {bad}, FutureThing = 'untouched' }}"),
            "15.4",
        )
        .unwrap();
        assert!(r.settings.locals.retouch.is_empty());
        assert_eq!(
            r.unknown["lrcat_develop_source"]["properties"]["RetouchAreas"],
            bad
        );
        assert_eq!(
            r.unknown["lrcat_develop_source"]["properties"]["FutureThing"],
            "'untouched'"
        );
    }
}

#[test]
fn lr3_unknown_method_is_not_silently_dropped() {
    let spot = SPOT.replace("centerX=", "Method='future-neural', centerX=");
    let (r, _) = develop(1, &format!("s = {{ RetouchAreas = {{ {spot} }} }}"), "15.4").unwrap();
    assert!(r.settings.locals.retouch.is_empty());
    assert!(r.unknown["lrcat_develop_source"]["properties"]["RetouchAreas"].is_string());
}

#[test]
fn lr3_xmp_inherited_namespace_prefix_is_not_semantic() {
    let xml = r#"<r:RDF xmlns:r="http://www.w3.org/1999/02/22-rdf-syntax-ns#"><r:Description xmlns:camera="http://ns.adobe.com/camera-raw-settings/1.0/"><camera:RetouchInfo><r:Seq><r:li>centerX=0.25, centerY=0.5, radius=0.0625, sourceX=0.75, sourceY=0.5, spotType=heal</r:li></r:Seq></camera:RetouchInfo></r:Description></r:RDF>"#;
    let (r, _) = develop(1, xml, "15.4").unwrap();
    assert_eq!(r.settings.locals.retouch.len(), 1);
    r.validate().unwrap();
    assert!(r.unknown.contains_key("lrcat_develop_source"));
}

#[test]
fn lr3d_import_is_one_history_entry_and_approximate_without_warnings() {
    for key in ["RetouchAreas", "RetouchInfo"] {
        let raw = format!("{{ {SPOT} }}");
        let (r, warnings) = develop(1, &format!("s = {{ {key} = {raw} }}"), "15.4").unwrap();
        assert_eq!(r.history.entries.len(), 1);
        assert!(matches!(
            r.history.entries[0].meta.author,
            engine_api::recipe::history::Author::Import { .. }
        ));
        r.validate().unwrap();
        assert_eq!(r.unknown["lrcat_develop_source"]["properties"][key], raw);
        assert!(warnings.is_empty(), "{warnings:?}");
        let entries = r.unknown["lrcat_translation_diagnostics"][key]
            .as_array()
            .unwrap();
        assert!(entries.iter().any(|e| e["level"] == "info"
            && e["status"] == "approximate"
            && e["field"] == "/settings/locals/retouch"));
        assert_eq!(serde_json::to_value(&r).unwrap()["schema_version"], 4);
    }
}

#[test]
fn lr3d_provenance_and_circle_are_approximate_but_center_value_is_retained() {
    for extra in ["Seed=12, MaskDigest='synthetic',", ""] {
        let spot = SPOT.replace("centerX=", &format!("{extra} centerX="));
        let (r, warnings) =
            develop(1, &format!("s = {{RetouchAreas = {{ {spot} }} }}"), "15.4").unwrap();
        assert_eq!(r.settings.locals.retouch.len(), 1);
        assert!(warnings.is_empty());
    }
    let (r, _) = develop(1, "s = {RetouchAreas={{SpotType='clone', SourceX=0.75, SourceY=0.5, Masks={{What='Mask/Circle', CenterX=0.25, CenterY=0.5, Radius=0.0625}}}}}", "15.4").unwrap();
    assert_eq!(r.settings.locals.retouch.len(), 1);
    let spot = SPOT.replace("centerX=", "CenterValue=0.3, centerX=");
    let (r, _) = develop(1, &format!("s = {{RetouchAreas = {{ {spot} }} }}"), "15.4").unwrap();
    assert!(r.settings.locals.retouch.is_empty());
}
