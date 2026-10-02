//! Synthetic LR-9c regressions.
use import_lrcat::{diagnostics, lua_develop};
#[test]
fn cloud_effects_are_visible_and_filter_list_survives() {
    for key in [
        "GenerativeRemove",
        "GenerativeFill",
        "EnableDistractionRemoval",
    ] {
        let (r, w) = lua_develop::parse(
            &format!("s={{{key}=true,FilterList={{{{What='synthetic'}}}}}}"),
            "15.4",
        )
        .unwrap();
        assert!(
            w.iter()
                .any(|w| w.contains(key) && w.contains("requires Adobe cloud; not translatable")),
            "{w:?}"
        );
        assert!(w.iter().any(|w| w.contains("FilterList")), "{w:?}");
        assert!(
            diagnostics::entries(&r)
                .values()
                .flatten()
                .any(|e| e.status == "cloud")
        );
    }
}
#[test]
fn mixed_heal_and_generative_keeps_both_dispositions() {
    let (r,w)=lua_develop::parse("s={RetouchAreas={{SpotType='heal',CenterX=0.2,CenterY=0.3,Radius=0.02,SourceX=0.6,SourceY=0.7},{SpotType='generative'}}}","15.4").unwrap();
    assert_eq!(r.settings.locals.retouch.len(), 1);
    let notes = diagnostics::entries(&r);
    assert!(
        notes["RetouchAreas"]
            .iter()
            .any(|e| e.status == "approximate")
    );
    assert!(notes.values().flatten().any(|e| e.status == "cloud"));
    assert!(w.iter().any(|w| w.contains("requires Adobe cloud")));
}
#[test]
fn legacy_defaults_leave_no_diagnostics() {
    let (r, w) = lua_develop::parse(
        "s={FillLight=0,HighlightRecovery=0,Recovery=0,Blacks=5}",
        "15.4",
    )
    .unwrap();
    assert!(w.is_empty(), "{w:?}");
    assert!(diagnostics::entries(&r).is_empty());
}
#[test]
fn embedded_profile_selected_is_not_silent() {
    let (_, w) =
        lua_develop::parse("s={LensProfileIsEmbedded=true,LensProfileEnable=1}", "15.4").unwrap();
    assert!(
        w.iter().any(|w| w.contains("LensProfileIsEmbedded")),
        "{w:?}"
    );
}
#[test]
fn version_on_undocumented_mask_kind_is_retained() {
    let (r,w)=lua_develop::parse("s={MaskGroupBasedCorrections={{What='Correction',LocalExposure2012=1,CorrectionMasks={{What='Mask/Gradient',MaskID='synthetic',Version=2,FullX=0,FullY=0,ZeroX=1,ZeroY=1}}}}}","15.4").unwrap();
    assert!(!w.is_empty());
    assert!(!diagnostics::entries(&r).contains_key("MaskGroupBasedCorrections"));
    assert!(
        r.unknown["lrcat_develop_source"]["properties"]["MaskGroupBasedCorrections"].is_string()
    );
}
#[test]
fn conflicting_source_y_aliases_warn() {
    let (r,w)=lua_develop::parse("s={RetouchAreas={{SpotType='heal',SourceX=0.7,SourceY=0.6,OffsetY=0.8,Masks={{What='Mask/Paint',Radius=0.03,Dabs={'d 0.2 0.4'}}}}}}","15.4").unwrap();
    assert!(!w.is_empty());
    assert!(r.settings.locals.retouch.is_empty());
}
#[test]
fn circle_centerweight_controls_feather() {
    let (r,w)=lua_develop::parse("s={RetouchAreas={{SpotType='heal',SourceX=0.7,SourceY=0.6,Masks={{What='Mask/Circle',Radius=0.03,CenterX=0.2,CenterY=0.4,CenterWeight=0.7}}}}}","15.4").unwrap();
    assert!(w.is_empty(), "{w:?}");
    let engine_api::recipe::mask::RetouchTarget::Area { components } =
        &r.settings.locals.retouch[0].target
    else {
        panic!()
    };
    let engine_api::recipe::MaskKind::Brush { strokes } = &components[0].kind else {
        panic!()
    };
    assert!((strokes[0].feather - 30.).abs() < 1e-5);
}
#[test]
fn unrelated_out_of_range_control_does_not_claim_auto_tone() {
    let (_, w) = lua_develop::parse("s={GrainAmount=1000}", "15.4").unwrap();
    assert!(
        w.iter()
            .any(|w| w.contains("GrainAmount") && w.contains("range")),
        "{w:?}"
    );
    assert!(!w.iter().any(|w| w.contains("auto-tone")), "{w:?}");
}
#[test]
fn curve_name_without_valid_points_is_not_suppressed() {
    for points in [
        "",
        ",ToneCurvePV2012={0,0,0,255}",
        ",ToneCurvePV2012={0,0,256,255}",
    ] {
        let (_, w) = lua_develop::parse(
            &format!("s={{ToneCurveName2012='synthetic'{points}}}"),
            "15.4",
        )
        .unwrap();
        assert!(w.iter().any(|w| w.contains("ToneCurveName2012")), "{w:?}");
    }
}
#[test]
fn embedded_profile_flag_is_silent_only_when_not_selected() {
    for silent in [
        "LensProfileIsEmbedded=true",
        "LensProfileIsEmbedded=true,LensProfileEnable=0",
        "LensProfileIsEmbedded=false,LensProfileEnable=1",
    ] {
        let (_, w) = lua_develop::parse(&format!("s={{{silent}}}"), "15.4").unwrap();
        assert!(
            !w.iter().any(|w| w.contains("LensProfileIsEmbedded")),
            "{silent}: {w:?}"
        );
    }
}
#[test]
fn non_default_legacy_tone_values_are_still_reported() {
    for active in [
        "FillLight=30",
        "HighlightRecovery=20",
        "Recovery=20",
        "Blacks=9",
    ] {
        let (r, w) = lua_develop::parse(&format!("s={{{active}}}"), "15.4").unwrap();
        assert!(
            !w.is_empty() || !diagnostics::entries(&r).is_empty(),
            "{active} was silenced"
        );
    }
}
#[test]
fn source_y_aliases_conflict_names_the_key_and_agreement_translates() {
    let spot = |offset: &str| {
        format!(
            "s={{RetouchAreas={{{{SpotType='heal',SourceX=0.7,SourceY=0.6,OffsetY={offset},Masks={{{{What='Mask/Paint',Radius=0.03,Dabs={{'d 0.2 0.4'}}}}}}}}}}}}"
        )
    };
    let (r, w) = lua_develop::parse(&spot("0.8"), "15.4").unwrap();
    assert!(w.iter().any(|w| w.contains("RetouchAreas")), "{w:?}");
    assert!(r.settings.locals.retouch.is_empty());
    assert!(r.unknown["lrcat_develop_source"]["properties"]["RetouchAreas"].is_string());
    let (r, w) = lua_develop::parse(&spot("0.6"), "15.4").unwrap();
    assert!(w.is_empty(), "{w:?}");
    assert_eq!(r.settings.locals.retouch.len(), 1);
}
#[test]
fn generative_only_retouch_areas_are_cloud_not_ignored() {
    let (r, w) = lua_develop::parse("s={RetouchAreas={{SpotType='generative'}}}", "15.4").unwrap();
    assert!(r.settings.locals.retouch.is_empty());
    assert!(
        w.iter()
            .any(|w| w.contains("requires Adobe cloud; not translatable")),
        "{w:?}"
    );
    let notes = diagnostics::entries(&r);
    assert!(notes.values().flatten().any(|e| e.status == "cloud"));
    assert!(!notes.values().flatten().any(|e| e.status == "ignored"));
}
