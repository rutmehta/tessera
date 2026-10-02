//! LR-11b review findings. Synthetic sources only; no catalog rows or identifiers.
use import_lrcat::{diagnostics, lua_develop};

const GRADIENT: &str =
    "CorrectionMasks={{What='Mask/Gradient',MaskID='synthetic',FullX=0,FullY=0,ZeroX=1,ZeroY=0}}";
const POINT: &str =
    "LocalPointColors={'0,0.8,0.5,0.1,0,0,0.5,0,0.25,0.75,1,0,0.25,0.75,1,0,0.25,0.75,1'}";

fn fixture(name: &str, extension: &str) -> String {
    std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join(format!("tests/data/lr11/{name}.{extension}")),
    )
    .unwrap()
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
