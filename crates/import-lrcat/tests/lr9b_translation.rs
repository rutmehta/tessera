//! Synthetic LR-9b reason-class regressions. No real catalog content.
use import_lrcat::{diagnostics, lua_develop};
#[test]
fn modern_process_ignores_nondefault_legacy_controls() {
    let (r,w) = lua_develop::parse("s={Brightness=72,Contrast=31,Shadows=8,Exposure=0.75,FillLight=12,HighlightRecovery=14}","15.4").unwrap();
    assert!(w.is_empty(), "{w:?}");
    assert!(r.settings.tone.legacy_pv2010.is_none());
    for key in ["Brightness","Contrast","Shadows","Exposure","FillLight","HighlightRecovery"] {
        assert_eq!(diagnostics::entries(&r)[key][0].status,"ignored");
        assert!(r.unknown["lrcat_develop_source"]["properties"].get(key).is_some());
    }
}
#[test]
fn genuine_legacy_controls_still_translate() {
    for pv in ["5.0","5.7"] {
        let (r,w) = lua_develop::parse("s={Brightness=72,Contrast=31,Shadows=8,Exposure=0.75,FillLight=12,HighlightRecovery=14}",pv).unwrap();
        assert!(w.is_empty(), "{w:?}");
        let legacy = r.settings.tone.legacy_pv2010.unwrap();
        assert_eq!(legacy.brightness,Some(72.));
        assert_eq!(legacy.exposure,Some(0.75));
    }
}

#[test]
fn documented_mask_controls_promote_without_discarding_real_edits() {
    let source = "s={MaskGroupBasedCorrections={{What='Correction',CorrectionReferenceX=0.3,CorrectionReferenceY=0.4,LocalBrightness=0,LocalContrast=0,LocalExposure=0,LocalClarity=0,LocalCurveRefineSaturation=100,LocalGrain=0,LocalCorrectedDepth=0,LocalColorVariance=0,LocalPointColors={},LocalExposure2012=0.5,CorrectionMasks={{What='Mask/CircularGradient',Version=2,MaskID='synthetic',Left=0.2,Top=0.2,Right=0.8,Bottom=0.8,Feather=50}}}}}";
    let (r,w)=lua_develop::parse(source,"15.4").unwrap();
    assert!(w.is_empty(),"{w:?}");
    assert_eq!(r.settings.locals.adjustments[0].params.exposure,0.5);
    assert_eq!(diagnostics::entries(&r)["MaskGroupBasedCorrections"][0].status,"approximate");
    assert!(r.unknown["lrcat_develop_source"]["properties"].get("MaskGroupBasedCorrections").is_some());
}

#[test]
fn retouch_brush_metadata_and_vertical_offset_translate() {
    let (r,w)=lua_develop::parse("s={RetouchAreas={{SpotType='heal',Method='heal',HealVersion=2,SourceState='sourceSetExplicitly',SourceX=0.7,OffsetY=0.1,Feather=0.3,Masks={{What='Mask/Paint',MaskID='synthetic',MaskSyncID='synthetic',CenterWeight=0.7,Radius=0.03,Flow=1,MaskActive=true,MaskInverted=false,MaskBlendMode=0,MaskValue=1,Dabs={'d 0.2 0.4','d 0.3 0.5'}}}}}}","15.4").unwrap();
    assert!(w.is_empty(),"{w:?}");
    assert_eq!(r.settings.locals.retouch.len(),1);
    assert!(matches!(r.settings.locals.retouch[0].kind,engine_api::recipe::mask::RetouchKind::Heal{..}));
}

#[test]
fn active_local_curves_name_the_missing_feature() {
    let (_,w)=lua_develop::parse("s={MaskGroupBasedCorrections={{What='Correction',MainCurve={0,0,128,150,255,255},CorrectionMasks={{What='Mask/Gradient',MaskID='synthetic',FullX=0,FullY=0,ZeroX=1,ZeroY=1}}}}}","15.4").unwrap();
    assert!(w.iter().any(|w|w.contains("local tone curve")),"{w:?}");
    assert!(!w.iter().any(|w|w.contains("mask source retained")));
}
#[test]
fn ai_raster_metadata_does_not_block_regeneration() {
    let (r,w)=lua_develop::parse("s={MaskGroupBasedCorrections={{What='Correction',LocalExposure2012=0.5,CorrectionMasks={{What='Mask/Image',MaskSubType=1,MaskID='synthetic',FullMaskSize='synthetic-size',LocalInputDigest='synthetic-digest',LocalInputDigestVersion=1}}}}}","15.4").unwrap();
    assert!(w.is_empty(),"{w:?}");
    assert_eq!(r.settings.locals.adjustments.len(),1);
}

#[test]
fn remaining_global_effects_have_named_feature_diagnostics() {
    for (key, value, expected) in [("SDRBrightness","20","SDR rendition"),("IncrementalTemperature","4","relative white balance"),("OverrideLookVignette","true","profile vignette"),("RemoveAreas","{{SpotType='contentAware',pm_patch='synthetic'}}","content-aware")] {
        let (_,w)=lua_develop::parse(&format!("s={{HDREditMode=1,{key}={value}}}"),"15.4").unwrap();
        assert!(w.iter().any(|w|w.contains(expected)),"{key}: {w:?}");
        assert!(!w.iter().any(|w|w.contains("unsupported property")||w.contains("unknown Lua develop key")),"{w:?}");
    }
}
#[test]
fn generative_removal_has_one_cloud_note_per_image() {
    let (r,w)=lua_develop::parse("s={EnableDistractionRemoval=true,RemoveAreas={{SpotType='generative',pm_clio_model_version='synthetic',pm_patch='synthetic'},{SpotType='generative',pm_clio_model_version='synthetic',pm_patch='synthetic'}}}","15.4").unwrap();
    assert!(w.is_empty(),"{w:?}");
    let notes: Vec<_>=diagnostics::entries(&r).into_values().flatten().filter(|e|e.reason.contains("requires Adobe cloud; not translatable")).collect();
    assert_eq!(notes.len(),1);
    assert_eq!(notes[0].status,"ignored");
}
