//! LR-11b review findings. Synthetic sources only; no catalog rows or identifiers.
use import_lrcat::{diagnostics, lua_develop};

const GRADIENT: &str =
    "CorrectionMasks={{What='Mask/Gradient',MaskID='synthetic',FullX=0,FullY=0,ZeroX=1,ZeroY=0}}";
const POINT: &str =
    "LocalPointColors={'0,0.8,0.5,0.1,0,0,0.5,0,0.25,0.75,1,0,0.25,0.75,1,0,0.25,0.75,1'}";

fn fixture(name: &str, extension: &str) -> String {
    let data = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data");
    ["lr11", "lr11b"]
        .iter()
        .find_map(|dir| {
            std::fs::read_to_string(data.join(format!("{dir}/{name}.{extension}"))).ok()
        })
        .unwrap()
}

fn both(name: &str) -> [(engine_api::recipe::Recipe, Vec<String>); 2] {
    [
        lua_develop::parse(&fixture(name, "lua"), "15.4").unwrap(),
        import_lrcat::xmp::parse(&fixture(name, "xmp"), "15.4").unwrap(),
    ]
}

fn approximate_fields(r: &engine_api::recipe::Recipe) -> Vec<String> {
    diagnostics::entries(r)
        .values()
        .flatten()
        .filter(|d| d.lane == "LR-11" && d.status == "approximate")
        .filter_map(|d| d.field.clone())
        .collect()
}

fn retained(r: &engine_api::recipe::Recipe) -> bool {
    r.unknown["lrcat_develop_source"]["properties"]
        .get("MaskGroupBasedCorrections")
        .is_some()
}

/// B2: an individual AI instance selection is never widened to the whole
/// object. It stays unsupported: one named warning, exact source, no recipe mask.
#[test]
fn b2_ai_instance_selection_is_unsupported_and_retained_in_both_codecs() {
    let (lua, lua_warnings) = lua_develop::parse(&fixture("instance", "lua"), "15.4").unwrap();
    let (xmp, xmp_warnings) =
        import_lrcat::xmp::parse(&fixture("instance", "xmp"), "15.4").unwrap();
    for (r, w) in [(&lua, &lua_warnings), (&xmp, &xmp_warnings)] {
        assert_eq!(w.len(), 1, "{w:?}");
        assert!(
            w[0].starts_with("crs:MaskGroupBasedCorrections:")
                && w[0].contains("individual AI instance selection"),
            "{w:?}"
        );
        assert!(r.settings.locals.adjustments.is_empty());
        assert!(retained(r));
        assert!(!serde_json::to_string(r).unwrap().contains("instance_hint"));
        assert!(
            !diagnostics::entries(r)
                .values()
                .flatten()
                .any(|d| d.lane == "LR-11" || d.status == "approximate")
        );
        assert_eq!(engine_api::recipe::required_schema_version(r), 3);
    }
}

/// B2: the instance keys are named even when every other part of the group is
/// translatable, and they block the whole parent rather than one component.
#[test]
fn b2_instance_keys_block_the_parent_with_other_translatable_content() {
    for keys in [
        "InstanceIDs={{InstanceID=1}}",
        "InstanceBounds={{Left=0.2,Top=0.2,Right=0.8,Bottom=0.8}}",
    ] {
        let source = format!(
            "s={{MaskGroupBasedCorrections={{{{LocalExposure2012=1,CorrectionMasks={{{{What='Mask/Gradient',MaskID='synthetic',FullX=0,FullY=0,ZeroX=1,ZeroY=0,{keys}}}}}}}}}}}"
        );
        let (r, w) = lua_develop::parse(&source, "15.4").unwrap();
        assert_eq!(w.len(), 1, "{keys}: {w:?}");
        assert!(
            w[0].contains("individual AI instance selection"),
            "{keys}: {w:?}"
        );
        assert!(r.settings.locals.adjustments.is_empty(), "{keys}");
        assert!(retained(&r));
    }
}

/// Restack consequence: without AI-mask promotion an AI selection blocks its
/// group. A decodable local operator in that group is not the reason.
#[test]
fn decodable_local_operators_are_not_blamed_for_an_unsupported_selection() {
    for operator in [
        "MainCurve={0,0,128,150,255,255}",
        "LocalColorVariance={0,0,0},MainCurve={0,0,128,150,255,255}",
        POINT,
        "LocalToningHue=120,LocalToningSaturation=40",
        "LocalDefringe=50",
    ] {
        let source = format!(
            "s={{MaskGroupBasedCorrections={{{{What='Correction',{operator},CorrectionMasks={{{{What='Mask/Image',MaskSubType=1,MaskID='synthetic'}}}}}}}}}}"
        );
        let (r, w) = lua_develop::parse(&source, "15.4").unwrap();
        assert_eq!(w.len(), 1, "{operator}: {w:?}");
        for label in [
            "local tone curve",
            "local point-color",
            "local color overlay",
            "local defringe",
            "color-variance",
        ] {
            assert!(!w[0].contains(label), "{operator}: {w:?}");
        }
        assert!(r.settings.locals.adjustments.is_empty(), "{operator}");
        assert!(retained(&r));
    }
}

/// The named operator reasons remain for operators that really fail to decode.
#[test]
fn undecodable_local_operators_keep_their_named_reason() {
    for (operator, label) in [
        ("MainCurve={0,0,128,150,64,255}", "local tone curve"),
        ("ExtendedMainCurve={0,0,0,255}", "local tone curve"),
        ("LocalPointColors={'0,1,2'}", "local point-color"),
        (
            "LocalToningHue=120,LocalToningSaturation=101",
            "local color overlay",
        ),
        ("LocalDefringe=101", "local defringe"),
    ] {
        let source = format!("s={{MaskGroupBasedCorrections={{{{{operator},{GRADIENT}}}}}}}");
        let (r, w) = lua_develop::parse(&source, "15.4").unwrap();
        assert_eq!(w.len(), 1, "{operator}: {w:?}");
        assert!(w[0].contains(label), "{operator}: {w:?}");
        assert!(r.settings.locals.adjustments.is_empty(), "{operator}");
        assert!(retained(&r));
    }
}

/// A radial `Flipped` flag in a group that is retained for another reason is
/// not an inversion conflict; only disagreeing flags are.
#[test]
fn radial_conflict_is_named_only_when_the_flags_disagree() {
    let radial = "What='Mask/CircularGradient',Left=0.2,Top=0.2,Right=0.8,Bottom=0.8";
    let (_, w) = lua_develop::parse(
        &format!(
            "s={{MaskGroupBasedCorrections={{{{LocalExposure2012=1,CorrectionMasks={{{{{radial},Flipped=true}},{{What='Mask/Image',MaskSubType=1,MaskID='synthetic'}}}}}}}}}}"
        ),
        "15.4",
    )
    .unwrap();
    assert_eq!(w.len(), 1, "{w:?}");
    assert!(!w[0].contains("inversion flags conflict"), "{w:?}");
    let (_, w) = lua_develop::parse(
        &format!(
            "s={{MaskGroupBasedCorrections={{{{LocalExposure2012=1,CorrectionMasks={{{{{radial},Flipped=true,MaskInverted=true}}}}}}}}}}"
        ),
        "15.4",
    )
    .unwrap();
    assert_eq!(w.len(), 1, "{w:?}");
    assert!(
        w[0].contains("radial mask inversion flags conflict"),
        "{w:?}"
    );
}

/// B3: the local path follows the global rule. Without HDR output the
/// extended curve is provenance only and the ordinary curve renders.
#[test]
fn b3_sdr_images_ignore_the_extended_local_curve() {
    let [lua, xmp] = both("curve-extended-sdr");
    assert_eq!(lua.0.settings.locals, xmp.0.settings.locals);
    for (r, w) in [lua, xmp] {
        assert!(w.is_empty(), "{w:?}");
        assert!(!r.settings.output.hdr);
        let p = &r.settings.locals.adjustments[0].params;
        assert_eq!(p.curves.as_ref().unwrap().rgb.0[1].y, 0.5);
        assert!(p.curves_extended.is_none());
        let fields = approximate_fields(&r);
        assert!(fields.iter().any(|f| f.ends_with("/params/curves")));
        assert!(
            !fields
                .iter()
                .any(|f| f.ends_with("/params/curves_extended"))
        );
        assert!(retained(&r));
        assert!(
            !serde_json::to_string(&r)
                .unwrap()
                .contains("curves_extended")
        );
    }
}

/// B3: with HDR output the non-identity extended curve is translated, exactly
/// as the global extended curve is.
#[test]
fn b3_hdr_images_translate_the_extended_local_curve() {
    let [lua, xmp] = both("curve-extended-hdr");
    assert_eq!(lua.0.settings.locals, xmp.0.settings.locals);
    for (r, w) in [lua, xmp] {
        assert!(w.is_empty(), "{w:?}");
        assert!(r.settings.output.hdr);
        let p = &r.settings.locals.adjustments[0].params;
        let extended = p.curves_extended.as_ref().unwrap();
        assert_eq!(extended.rgb.0.len(), 3);
        assert_eq!(extended.rgb.0[2].x, 2.);
        assert!(
            approximate_fields(&r)
                .iter()
                .any(|f| f.ends_with("/params/curves_extended"))
        );
    }
}

/// B3: the remaining clauses of the global rule. An identity extended curve
/// never replaces the ordinary curve, a legacy process has no HDR curve, and a
/// malformed extended curve is still reported on an SDR image.
#[test]
fn b3_identity_legacy_and_malformed_extended_local_curves() {
    let source = |top: &str, extended: &str| {
        format!(
            "s={{{top}MaskGroupBasedCorrections={{{{MainCurve={{0,0,255,127.5}},ExtendedMainCurve={{{extended}}},{GRADIENT}}}}}}}"
        )
    };
    let (r, w) =
        lua_develop::parse(&source("HDREditMode=1,", "0,0,255,255,510,510"), "15.4").unwrap();
    assert!(w.is_empty(), "{w:?}");
    let p = &r.settings.locals.adjustments[0].params;
    assert!(p.curves.is_some() && p.curves_extended.is_none());

    let (r, _) =
        lua_develop::parse(&source("HDREditMode=1,", "0,0,255,255,510,600"), "5.7").unwrap();
    assert!(
        r.settings
            .locals
            .adjustments
            .iter()
            .all(|g| g.params.curves_extended.is_none())
    );
    assert!(
        !approximate_fields(&r)
            .iter()
            .any(|f| f.ends_with("/params/curves_extended"))
    );

    let (r, w) = lua_develop::parse(&source("", "0,0,0,255"), "15.4").unwrap();
    assert_eq!(w.len(), 1, "{w:?}");
    assert!(w[0].contains("local tone curve"), "{w:?}");
    assert!(r.settings.locals.adjustments.is_empty());
}

/// S9: local defringe accepts Adobe's signed -100..=100 range in both codecs.
#[test]
fn s9_local_defringe_accepts_the_signed_adobe_range() {
    for value in [-100., -50., 100.] {
        let lua =
            format!("s={{MaskGroupBasedCorrections={{{{LocalDefringe={value},{GRADIENT}}}}}}}");
        let xmp = format!(
            "<x:xmpmeta xmlns:x=\"adobe:ns:meta/\"><rdf:RDF xmlns:rdf=\"http://www.w3.org/1999/02/22-rdf-syntax-ns#\"><rdf:Description rdf:about=\"\" xmlns:crs=\"http://ns.adobe.com/camera-raw-settings/1.0/\" crs:ProcessVersion=\"15.4\"><crs:MaskGroupBasedCorrections><rdf:Seq><rdf:li rdf:parseType=\"Resource\"><crs:LocalDefringe>{value}</crs:LocalDefringe><crs:CorrectionMasks><rdf:Seq><rdf:li rdf:parseType=\"Resource\"><crs:What>Mask/Gradient</crs:What><crs:MaskID>synthetic</crs:MaskID><crs:FullX>0</crs:FullX><crs:FullY>0</crs:FullY><crs:ZeroX>1</crs:ZeroX><crs:ZeroY>0</crs:ZeroY></rdf:li></rdf:Seq></crs:CorrectionMasks></rdf:li></rdf:Seq></crs:MaskGroupBasedCorrections></rdf:Description></rdf:RDF></x:xmpmeta>"
        );
        let a = lua_develop::parse(&lua, "15.4").unwrap();
        let b = import_lrcat::xmp::parse(&xmp, "15.4").unwrap();
        assert_eq!(a.0.settings.locals, b.0.settings.locals, "{value}");
        for (r, w) in [a, b] {
            assert!(w.is_empty(), "{value}: {w:?}");
            assert_eq!(r.settings.locals.adjustments[0].params.defringe, value);
            assert!(retained(&r));
            assert_eq!(engine_api::recipe::required_schema_version(&r), 4);
            let notes: Vec<_> = diagnostics::entries(&r)
                .values()
                .flatten()
                .filter(|d| {
                    d.lane == "LR-11"
                        && d.status == "approximate"
                        && d.field.as_deref()
                            == Some("/settings/locals/adjustments/0/params/defringe")
                })
                .map(|d| d.reason.clone())
                .collect();
            assert_eq!(notes.len(), 1, "{value}: {notes:?}");
            // A negative value protects the area from global defringe; the
            // note has to say that this protection is not rendered.
            assert_eq!(notes[0].contains("not rendered"), value < 0., "{notes:?}");
            let round = engine_api::recipe::Recipe::from_json(&r.to_json().unwrap()).unwrap();
            assert_eq!(round.settings, r.settings);
        }
    }
}

/// S9: values outside -100..=100 are still retained with the named reason.
#[test]
fn s9_local_defringe_outside_the_adobe_range_is_retained() {
    for value in ["-100.5", "-101", "100.5", "101"] {
        let source =
            format!("s={{MaskGroupBasedCorrections={{{{LocalDefringe={value},{GRADIENT}}}}}}}");
        let (r, w) = lua_develop::parse(&source, "15.4").unwrap();
        assert_eq!(w.len(), 1, "{value}: {w:?}");
        assert!(w[0].contains("local defringe"), "{value}: {w:?}");
        assert!(r.settings.locals.adjustments.is_empty(), "{value}");
        assert!(retained(&r));
    }
}
