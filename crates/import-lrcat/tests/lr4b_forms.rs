//! Entirely invented fixtures. Tests both Adobe Lua and RDF spellings.
use import_lrcat::{lua_develop, xmp};
use serde_json::json;
fn recipes(lua: &str, xml: &str) -> [engine_api::recipe::Recipe; 2] {
    let lua = format!(
        r#"s = {{ ProcessVersion = "15.4", MaskGroupBasedCorrections = {{{{LocalExposure2012 = 1, CorrectionMasks = {{{{{lua}}}}} }}}} }}"#
    );
    let xml = format!(
        r#"<x:xmpmeta xmlns:x="adobe:ns:meta/"><rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#"><rdf:Description xmlns:crs="http://ns.adobe.com/camera-raw-settings/1.0/"><crs:MaskGroupBasedCorrections><rdf:Seq><rdf:li rdf:parseType="Resource"><crs:LocalExposure2012>1</crs:LocalExposure2012><crs:CorrectionMasks><rdf:Seq><rdf:li rdf:parseType="Resource">{xml}</rdf:li></rdf:Seq></crs:CorrectionMasks></rdf:li></rdf:Seq></crs:MaskGroupBasedCorrections></rdf:Description></rdf:RDF></x:xmpmeta>"#
    );
    [
        lua_develop::parse(&lua, "15.4").unwrap().0,
        xmp::parse(&xml, "15.4").unwrap().0,
    ]
}
#[test]
fn lr4b_four_bounds_translate_and_promote() {
    for r in recipes(
        r#"What="Mask/RangeMask", CorrectionRangeMask={Type=2, LumRange="0.1 0.3 0.6 1"}"#,
        r#"<crs:What>Mask/RangeMask</crs:What><crs:CorrectionRangeMask crs:Type="2" crs:LumRange="0.1 0.3 0.6 1"/>"#,
    ) {
        assert_eq!(r.settings.locals.adjustments.len(), 1);
        let c = serde_json::to_value(&r.settings.locals.adjustments[0].components[0]).unwrap();
        assert_eq!(c["kind"], "luminance_range");
        assert_eq!(c["luminance_bounds"], json!([0.1f32, 0.3f32, 0.6f32, 1f32]));
        assert!(r.unknown.contains_key("lrcat_develop_source"));
        assert!(!import_lrcat::diagnostics::entries(&r).is_empty());
    }
}
#[test]
fn lr4b_radial_flipped_is_complement_of_invert_not_a_second_toggle() {
    for (flipped, inverted) in [(true, false), (false, true)] {
        for explicit in [false, true] {
            let l = if explicit {
                format!(", MaskInverted={inverted}")
            } else {
                String::new()
            };
            let x = if explicit {
                format!("<crs:MaskInverted>{inverted}</crs:MaskInverted>")
            } else {
                String::new()
            };
            for r in recipes(
                &format!(
                    r#"What="Mask/CircularGradient", Left=0.25, Right=0.75, Top=0, Bottom=1, Flipped={flipped}{l}"#
                ),
                &format!(
                    r#"<crs:What>Mask/CircularGradient</crs:What><crs:Left>0.25</crs:Left><crs:Right>0.75</crs:Right><crs:Top>0</crs:Top><crs:Bottom>1</crs:Bottom><crs:Flipped>{flipped}</crs:Flipped>{x}"#
                ),
            ) {
                assert_eq!(r.settings.locals.adjustments.len(), 1);
                assert_eq!(
                    r.settings.locals.adjustments[0].components[0].invert,
                    inverted
                );
                assert!(r.unknown.contains_key("lrcat_develop_source"));
                assert!(!import_lrcat::diagnostics::entries(&r).is_empty());
            }
        }
    }
}
#[test]
fn lr4b_subtype_depth_and_luminance() {
    for (ty, lo, hi, kind) in [
        (2, "LumMin", "LumMax", "luminance_range"),
        (3, "DepthMin", "DepthMax", "depth"),
    ] {
        for r in recipes(
            &format!(
                r#"What="Mask/RangeMask", CorrectionRangeMask={{Type={ty}, {lo}=0.2, {hi}=0.8}}"#
            ),
            &format!(
                r#"<crs:What>Mask/RangeMask</crs:What><crs:CorrectionRangeMask crs:Type="{ty}" crs:{lo}="0.2" crs:{hi}="0.8"/>"#
            ),
        ) {
            assert_eq!(r.settings.locals.adjustments.len(), 1);
            assert_eq!(
                serde_json::to_value(&r.settings.locals.adjustments[0].components[0]).unwrap()["kind"],
                kind
            );
            assert!(r.unknown.contains_key("lrcat_develop_source"));
            assert!(!import_lrcat::diagnostics::entries(&r).is_empty());
        }
    }
}
#[test]
fn lr4b_unsupported_payloads_retain_exact_lua() {
    for payload in [
        r#"What="Mask/Paint",Radius=0.05,Flow=0.5,CenterWeight=0.75,MaskValue=1,Dabs={"d 0.125 0.5","r 0.1","f 0.25","h 0.5","d 0.875 0.5"}"#,
        r#"What="Mask/Paint",Dabs={"q 0.5"}"#,
        r#"What="Mask/RangeMask",CorrectionRangeMask={Type=1,AreaModels={"opaque"}}"#,
        r#"What="Mask/RangeMask",CorrectionRangeMask={Type=9,LumMin=0,LumMax=1}"#,
    ] {
        let source = format!("{{{{CorrectionMasks={{{{{payload}}}}}}}}}");
        let row = format!("s={{MaskGroupBasedCorrections={source}}}");
        let r = lua_develop::parse(&row, "15.4").unwrap().0;
        assert_eq!(
            r.unknown["lrcat_develop_source"]["properties"]["MaskGroupBasedCorrections"],
            source
        );
    }
}

#[test]
fn lr4b_ambiguous_new_shapes_are_not_partially_translated() {
    for fields in [
        r#"LumRange="0 0.2 0.8 1",LumMin=0.1"#,
        r#"LumRange="0 0.2 0.8 1",DepthMin=0.1"#,
        r#"LumRange="0 0.2 0.8 1",PointModels={"0.2 0.3 0.4 0.5 0.5 0"}"#,
        r#"Type=3,LumRange="0 0.2 0.8 1""#,
        r#"LumRange="0 0.8 0.2 1""#,
    ] {
        let (r,_)=lua_develop::parse(&format!(r#"s={{MaskGroupBasedCorrections={{{{CorrectionMasks={{{{What="Mask/RangeMask",CorrectionRangeMask={{{fields}}}}}}}}}}}}}"#),"15.4").unwrap();
        assert!(r.settings.locals.adjustments.is_empty(), "{fields}");
        assert!(r.unknown.contains_key("lrcat_develop_source"));
    }
}

#[test]
fn lr4b_untranslated_extensions_keep_the_prior_empty_recipe() {
    // Before LR-4b these entire keys failed atomically. A newly understood leaf
    // must not change that recipe if another field still prevents translation.
    for (mask, correction) in [
        (
            r#"What="Mask/RangeMask",Future=7,CorrectionRangeMask={Type=2,LumRange="0 0.25 0.5 1"}"#,
            "",
        ),
        (
            r#"What="Mask/RangeMask",CorrectionRangeMask={Type=2,LumRange="0 0.25 0.5 1",Future=7}"#,
            "",
        ),
        (
            r#"What="Mask/RangeMask",CorrectionRangeMask={Type=2,LumRange="0 0.25 0.5 1"}"#,
            "Future=7,",
        ),
        (
            r#"What="Mask/CircularGradient",Left=0.25,Right=0.75,Top=0,Bottom=1,Flipped=false,Future=7"#,
            "",
        ),
    ] {
        let row = format!(
            r#"s={{MaskGroupBasedCorrections={{{{{correction}CorrectionMasks={{{{{mask}}}}}}}}}}}"#
        );
        let r = lua_develop::parse(&row, "15.4").unwrap().0;
        assert!(
            r.settings.locals.adjustments.is_empty(),
            "{mask}, {correction}"
        );
        assert!(r.unknown.contains_key("lrcat_develop_source"));
    }
}
