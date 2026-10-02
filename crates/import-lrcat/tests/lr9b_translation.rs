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
