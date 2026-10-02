//! Synthetic only: conservative predicates, Lua/XMP equivalence and exact source retention.
use import_lrcat::{
    diagnostics, lua_develop,
    noop::{RULES, Rule},
    xmp,
};

fn packet(attrs: &str, body: &str) -> String {
    format!(
        r#"<rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#" xmlns:crs="http://ns.adobe.com/camera-raw-settings/1.0/"><rdf:Description {attrs}>{body}</rdf:Description></rdf:RDF>"#
    )
}
#[test]
fn xmp_every_default_is_silent_and_retains_source() {
    for &(key, rule) in RULES {
        if key.contains('*') {
            continue;
        }
        let (attrs,body) = match rule {
            Rule::Provenance => (format!("crs:{key}=\"synthetic\""),String::new()),
            Rule::False => (format!("crs:{key}=\"False\""),String::new()),
            Rule::Empty | Rule::PointColors => (String::new(),format!("<crs:{key}><rdf:Seq/></crs:{key}>")),
            Rule::Zero => (format!("crs:{key}=\"0\""),String::new()),
            Rule::Number(n) | Rule::Legacy(n) | Rule::Upright(n) | Rule::Sdr(n) => (format!("crs:{key}=\"{n}\""),String::new()),
            Rule::LensBlur => (String::new(),"<crs:LensBlur rdf:parseType=\"Resource\"><crs:Active>False</crs:Active></crs:LensBlur>".into()),
            Rule::CurveName => ("crs:ToneCurveName2012=\"Synthetic preset name\"".into(),"<crs:ToneCurvePV2012><rdf:Seq><rdf:li>0, 0</rdf:li><rdf:li>255, 255</rdf:li></rdf:Seq></crs:ToneCurvePV2012>".into()),
        };
        let source = packet(&attrs, &body);
        let (recipe, warnings) = xmp::parse(&source, "15.4").unwrap();
        assert!(warnings.is_empty(), "{key}: {warnings:?}");
        assert!(!diagnostics::entries(&recipe).contains_key(key), "{key}");
        assert_eq!(recipe.unknown["sidecar_xmp"], source);
        assert!(
            recipe.unknown["lrcat_develop_source"]["properties"]
                .get(key)
                .is_some(),
            "{key}"
        );
    }
}
#[test]
fn unsafe_contexts_and_malformed_controls_still_warn() {
    for source in [
        "s={Brightness=50}", // PV2010 is translated, tested separately below.
        "s={ToneCurveName2012='Synthetic name'}",
        "s={ToneCurveName2012='Custom',ToneCurvePV2012={0,0,0,255}}",
        "s={UprightCenterNormX=0.5,PerspectiveUpright=1}",
        "s={UprightCenterNormX=0.5,PerspectiveUpright='malformed'}",
        "s={CurveRefineSaturation=99}",
        "s={SDRBrightness=1,HDREditMode=1}",
        "s={SDRBrightness='malformed',HDREditMode=0}",
        "s={LensBlur={Active='malformed'}}",
        "s={PointColors={[2]='-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1'}}",
    ] {
        let version = if source.contains("Brightness=50") {
            "5.7"
        } else {
            "15.4"
        };
        let (recipe, warnings) = lua_develop::parse(source, version).unwrap();
        assert!(
            !warnings.is_empty() || !diagnostics::entries(&recipe).is_empty(),
            "{source}"
        );
    }
}
#[test]
fn placeholders_and_inactive_hdr_are_silent() {
    for source in [
        "s={PointColors={'-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1'}}",
        "s={PointColors={[1]='-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1'}}",
        "s={SDRBlend=60,SDRBrightness=20,HDREditMode=0}",
        "s={AutoToneDigestFuture='synthetic'}",
    ] {
        let (recipe, warnings) = lua_develop::parse(source, "15.4").unwrap();
        assert!(warnings.is_empty(), "{warnings:?}");
        assert!(diagnostics::entries(&recipe).is_empty());
    }
}

#[test]
fn duplicate_controls_are_not_silenced() {
    assert!(
        lua_develop::parse(
            "s={CurveRefineSaturation=100,CurveRefineSaturation=99}",
            "15.4"
        )
        .is_err()
    );
    let source = packet(
        "crs:CurveRefineSaturation=\"100\"",
        "<crs:CurveRefineSaturation>99</crs:CurveRefineSaturation>",
    );
    assert!(!xmp::parse(&source, "15.4").unwrap().1.is_empty());
}

#[test]
fn empty_filter_payload_and_enabled_empty_panel_are_not_effects() {
    for source in [
        "s={AILook={}}",
        "s={FilterList={}}",
        "s={EnableDistractionRemoval=true,FilterList={}}",
    ] {
        let (recipe, warnings) = lua_develop::parse(source, "15.4").unwrap();
        assert!(warnings.is_empty(), "{warnings:?}");
        assert!(diagnostics::entries(&recipe).is_empty());
    }
    for source in [
        "s={AILook={Amount=1}}",
        "s={FilterList={{What='synthetic-filter'}}}",
        "s={EnableDistractionRemoval=true,FilterList={{What='synthetic-filter'}}}",
    ] {
        assert!(!lua_develop::parse(source, "15.4").unwrap().1.is_empty());
    }
}
