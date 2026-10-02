//! Synthetic LR-9b reason-class regressions. No real catalog content.
use import_lrcat::{diagnostics, lua_develop};
#[test]
fn modern_process_ignores_nondefault_legacy_controls() {
    let (r, w) = lua_develop::parse(
        "s={Brightness=72,Contrast=31,Shadows=8,Exposure=0.75,FillLight=12,HighlightRecovery=14}",
        "15.4",
    )
    .unwrap();
    assert!(w.is_empty(), "{w:?}");
    assert!(r.settings.tone.legacy_pv2010.is_none());
    for key in [
        "Brightness",
        "Contrast",
        "Shadows",
        "Exposure",
        "FillLight",
        "HighlightRecovery",
    ] {
        assert_eq!(diagnostics::entries(&r)[key][0].status, "ignored");
        assert!(
            r.unknown["lrcat_develop_source"]["properties"]
                .get(key)
                .is_some()
        );
    }
}
#[test]
fn genuine_legacy_controls_still_translate() {
    for pv in ["5.0", "5.7"] {
        let (r,w) = lua_develop::parse("s={Brightness=72,Contrast=31,Shadows=8,Exposure=0.75,FillLight=12,HighlightRecovery=14}",pv).unwrap();
        assert!(w.is_empty(), "{w:?}");
        let legacy = r.settings.tone.legacy_pv2010.unwrap();
        assert_eq!(legacy.brightness, Some(72.));
        assert_eq!(legacy.exposure, Some(0.75));
    }
}

#[test]
fn documented_mask_controls_promote_without_discarding_real_edits() {
    let source = "s={MaskGroupBasedCorrections={{What='Correction',CorrectionReferenceX=0.3,CorrectionReferenceY=0.4,LocalBrightness=0,LocalContrast=0,LocalExposure=0,LocalClarity=0,LocalCurveRefineSaturation=100,LocalGrain=0,LocalCorrectedDepth=0,LocalColorVariance=0,LocalPointColors={},LocalExposure2012=0.5,CorrectionMasks={{What='Mask/CircularGradient',Version=2,MaskID='synthetic',Left=0.2,Top=0.2,Right=0.8,Bottom=0.8,Feather=50}}}}}";
    let (r, w) = lua_develop::parse(source, "15.4").unwrap();
    assert!(w.is_empty(), "{w:?}");
    assert_eq!(r.settings.locals.adjustments[0].params.exposure, 0.5);
    assert_eq!(
        diagnostics::entries(&r)["MaskGroupBasedCorrections"][0].status,
        "approximate"
    );
    assert!(
        r.unknown["lrcat_develop_source"]["properties"]
            .get("MaskGroupBasedCorrections")
            .is_some()
    );
}

#[test]
fn retouch_brush_metadata_and_vertical_offset_translate() {
    let (r,w)=lua_develop::parse("s={RetouchAreas={{SpotType='heal',Method='heal',HealVersion=2,SourceState='sourceSetExplicitly',SourceX=0.7,OffsetY=0.1,Feather=0.3,Masks={{What='Mask/Paint',MaskID='synthetic',MaskSyncID='synthetic',CenterWeight=0.7,Radius=0.03,Flow=1,MaskActive=true,MaskInverted=false,MaskBlendMode=0,MaskValue=1,Dabs={'d 0.2 0.4','d 0.3 0.5'}}}}}}","15.4").unwrap();
    assert!(w.is_empty(), "{w:?}");
    assert_eq!(r.settings.locals.retouch.len(), 1);
    assert!(matches!(
        r.settings.locals.retouch[0].kind,
        engine_api::recipe::mask::RetouchKind::Heal { .. }
    ));
}

#[test]
fn active_local_curves_name_the_missing_feature() {
    let (_,w)=lua_develop::parse("s={MaskGroupBasedCorrections={{What='Correction',MainCurve={0,0,128,150,255,255},CorrectionMasks={{What='Mask/Gradient',MaskID='synthetic',FullX=0,FullY=0,ZeroX=1,ZeroY=1}}}}}","15.4").unwrap();
    assert!(w.iter().any(|w| w.contains("local tone curve")), "{w:?}");
    assert!(!w.iter().any(|w| w.contains("mask source retained")));
}
#[test]
fn ai_raster_metadata_does_not_block_regeneration() {
    let (r,w)=lua_develop::parse("s={MaskGroupBasedCorrections={{What='Correction',LocalExposure2012=0.5,CorrectionMasks={{What='Mask/Image',MaskSubType=1,MaskID='synthetic',FullMaskSize='synthetic-size',LocalInputDigest='synthetic-digest',LocalInputDigestVersion=1}}}}}","15.4").unwrap();
    assert!(w.is_empty(), "{w:?}");
    assert_eq!(r.settings.locals.adjustments.len(), 1);
}

#[test]
fn remaining_global_effects_have_named_feature_diagnostics() {
    for (key, value, expected) in [
        ("SDRBrightness", "20", "SDR rendition"),
        ("IncrementalTemperature", "4", "relative white balance"),
        ("OverrideLookVignette", "true", "profile vignette"),
        (
            "RemoveAreas",
            "{{SpotType='contentAware',pm_patch='synthetic'}}",
            "content-aware",
        ),
    ] {
        let (_, w) =
            lua_develop::parse(&format!("s={{HDREditMode=1,{key}={value}}}"), "15.4").unwrap();
        assert!(w.iter().any(|w| w.contains(expected)), "{key}: {w:?}");
        assert!(
            !w.iter().any(
                |w| w.contains("unsupported property") || w.contains("unknown Lua develop key")
            ),
            "{w:?}"
        );
    }
}
#[test]
fn generative_removal_has_one_cloud_note_per_image() {
    let (r,w)=lua_develop::parse("s={EnableDistractionRemoval=true,RemoveAreas={{SpotType='generative',pm_clio_model_version='synthetic',pm_patch='synthetic'},{SpotType='generative',pm_clio_model_version='synthetic',pm_patch='synthetic'}}}","15.4").unwrap();
    assert!(w.is_empty(), "{w:?}");
    let notes: Vec<_> = diagnostics::entries(&r)
        .into_values()
        .flatten()
        .filter(|e| e.reason.contains("requires Adobe cloud; not translatable"))
        .collect();
    assert_eq!(notes.len(), 1);
    assert_eq!(notes[0].status, "ignored");
}

#[test]
fn saved_overlay_hue_with_zero_saturation_is_inactive() {
    let (r,w)=lua_develop::parse("s={MaskGroupBasedCorrections={{What='Correction',LocalToningHue=80,LocalToningSaturation=0,LocalExposure2012=0.5,CorrectionMasks={{What='Mask/Gradient',MaskID='synthetic',FullX=0,FullY=0,ZeroX=1,ZeroY=1}}}}}","15.4").unwrap();
    assert!(w.is_empty(), "{w:?}");
    assert_eq!(r.settings.locals.adjustments[0].params.color_overlay, None);
    assert_eq!(r.settings.locals.adjustments[0].params.exposure, 0.5);
}

#[test]
fn retouch_offsety_is_the_absolute_source_y_spelling() {
    let (r,w)=lua_develop::parse("s={RetouchAreas={{SpotType='clone',SourceX=0.7,OffsetY=0.6,Masks={{What='Mask/Paint',Radius=0.03,Dabs={'d 0.2 0.4'}}}}}}","15.4").unwrap();
    assert!(w.is_empty(), "{w:?}");
    let engine_api::recipe::mask::RetouchKind::Clone { source_offset } =
        r.settings.locals.retouch[0].kind
    else {
        panic!("clone required")
    };
    assert!((source_offset[0] - 0.5).abs() < 1e-6);
    assert!((source_offset[1] - 0.2).abs() < 1e-6);
}

#[test]
fn ai_group_rejection_reports_the_effect_that_blocks_promotion() {
    let (_,w)=lua_develop::parse("s={MaskGroupBasedCorrections={{What='Correction',MainCurve={0,0,128,150,255,255},CorrectionMasks={{What='Mask/Image',MaskSubType=1,MaskID='synthetic'}}}}}","15.4").unwrap();
    assert_eq!(w.len(), 1, "{w:?}");
    assert!(w[0].contains("local tone curve"), "{w:?}");
    assert!(!w[0].contains("unsupported mask kind"));
}
#[test]
fn zero_local_color_variance_is_inactive() {
    let (r,w)=lua_develop::parse("s={MaskGroupBasedCorrections={{What='Correction',LocalColorVariance={0,0,0},LocalExposure2012=0.5,CorrectionMasks={{What='Mask/Gradient',MaskID='synthetic',FullX=0,FullY=0,ZeroX=1,ZeroY=1}}}}}","15.4").unwrap();
    assert!(w.is_empty(), "{w:?}");
    assert_eq!(r.settings.locals.adjustments[0].params.exposure, 0.5);
}

#[test]
fn mixed_cloud_and_patch_removal_keeps_both_dispositions() {
    let (r,w)=lua_develop::parse("s={RemoveAreas={{SpotType='generative',pm_clio_model_version='synthetic',pm_patch='synthetic'},{SpotType='contentAware',pm_patch='synthetic'}}}","15.4").unwrap();
    assert_eq!(w.len(), 1, "{w:?}");
    assert!(w[0].contains("content-aware"));
    let notes: Vec<_> = diagnostics::entries(&r)
        .into_values()
        .flatten()
        .filter(|e| e.reason.contains("requires Adobe cloud; not translatable"))
        .collect();
    assert_eq!(notes.len(), 1);
}

#[test]
fn cloud_switches_are_info_notes_once_per_image() {
    let (r,w)=lua_develop::parse("s={GenerativeRemove=true,GenerativeFill=true,EnableDistractionRemoval=true,FilterList={{What='synthetic-filter'}}}","15.4").unwrap();
    assert!(
        !w.iter().any(|w| w.contains("GenerativeRemove:")
            || w.contains("GenerativeFill:")
            || w.contains("EnableDistractionRemoval:")),
        "{w:?}"
    );
    let notes: Vec<_> = diagnostics::entries(&r)
        .into_values()
        .flatten()
        .filter(|e| e.reason.contains("requires Adobe cloud; not translatable"))
        .collect();
    assert_eq!(notes.len(), 1);
}
#[test]
fn legacy_fixture_byte_change_is_only_the_inactive_fill_light_note() {
    let (mut r, _) = lua_develop::parse(include_str!("data/lrc155/legacy.lua"), "15.4").unwrap();
    let notes = diagnostics::entries(&r);
    assert_eq!(notes.len(), 1);
    assert_eq!(notes["FillLight"][0].status, "ignored");
    r.unknown.remove(diagnostics::KEY);
    let bytes = r.to_json().unwrap();
    assert_eq!(bytes.len(), 11633);
    assert_eq!(
        engine_api::id::Digest::derive("LR-1 byte compatibility", &bytes).to_string(),
        "fe85a1a43d4268ab1aa6f24ba8add80325b3cd274d5b54e181f66bfbe6e438ce"
    );
}

#[test]
fn equivalent_retouch_aliases_are_accepted_but_conflicts_fail_closed() {
    for (alias, accepted) in [("0.5", true), ("0.8", false)] {
        let (r,w)=lua_develop::parse(&format!("s={{RetouchAreas={{{{SpotType='clone',spotType='clone',Opacity=0.5,opacity={alias},CenterX=0.2,CenterY=0.4,Radius=0.03,SourceX=0.7,SourceY=0.6}}}}}}"),"15.4").unwrap();
        assert_eq!(!r.settings.locals.retouch.is_empty(), accepted);
        assert_eq!(w.is_empty(), accepted);
        assert!(
            r.unknown["lrcat_develop_source"]["properties"]
                .get("RetouchAreas")
                .is_some()
        );
    }
}

#[test]
fn equivalent_ellipse_and_flat_circle_retouch_forms_translate() {
    for (size, accepted) in [("0.03", true), ("0.06", false)] {
        let (r,w)=lua_develop::parse(&format!("s={{RetouchAreas={{{{SpotType='clone',spotType='clone',Opacity=0.5,opacity=0.5,centerX=0.2,centerY=0.4,radius=0.03,sourceX=0.7,sourceY=0.6,Masks={{{{What='Mask/Ellipse',MaskID='synthetic',MaskSyncID='synthetic',MaskActive=true,MaskInverted=false,MaskBlendMode=0,MaskValue=1,X=0.2,Y=0.4,SizeX={size},SizeY=0.03,Alpha=0,CenterValue=1,PerimeterValue=0}}}}}}}}}}"),"15.4").unwrap();
        assert_eq!(w.is_empty(), accepted, "{w:?}");
        assert_eq!(r.settings.locals.retouch.len(), usize::from(accepted));
    }
}

#[test]
fn saved_preset_and_algorithm_metadata_are_not_missing_edits() {
    let source = "s={Preset='synthetic preset',AutoWhiteVersion=2,CropConstrainAspectRatio=true,Exposure2012=0.5}";
    let (r, w) = lua_develop::parse(source, "15.4").unwrap();
    assert!(w.is_empty(), "{w:?}");
    assert_eq!(r.settings.tone.exposure, 0.5);
    for key in ["Preset", "AutoWhiteVersion", "CropConstrainAspectRatio"] {
        assert!(
            r.unknown["lrcat_develop_source"]["properties"]
                .get(key)
                .is_some()
        );
        assert!(!diagnostics::entries(&r).contains_key(key));
    }
}

#[test]
fn modern_retouch_list_supersedes_the_legacy_alias_once() {
    let (r,w)=lua_develop::parse("s={RetouchAreas={{SpotType='clone',CenterX=0.2,CenterY=0.4,Radius=0.03,SourceX=0.7,SourceY=0.6,Feather=0.3}},RetouchInfo={'spotType=clone,centerX=0.2,centerY=0.4,radius=0.03,sourceX=0.7,sourceY=0.6'}}","15.4").unwrap();
    assert!(w.is_empty(), "{w:?}");
    assert_eq!(r.settings.locals.retouch.len(), 1);
    assert_eq!(
        diagnostics::entries(&r)["RetouchAreas"][0].status,
        "approximate"
    );
    assert_eq!(diagnostics::entries(&r)["RetouchInfo"][0].status, "ignored");
}

#[test]
fn independent_remove_areas_append_to_retouch_with_unique_ids() {
    let (r,w)=lua_develop::parse("s={RetouchAreas={{SpotType='clone',CenterX=0.2,CenterY=0.4,Radius=0.03,SourceX=0.7,SourceY=0.6}},RemoveAreas={{SpotType='heal',CenterX=0.3,CenterY=0.4,Radius=0.03,SourceX=0.8,SourceY=0.6}}}","15.4").unwrap();
    assert!(w.is_empty(), "{w:?}");
    assert_eq!(r.settings.locals.retouch.len(), 2);
    assert_ne!(
        r.settings.locals.retouch[0].id,
        r.settings.locals.retouch[1].id
    );
}

#[test]
fn conflicting_radial_inversion_is_a_named_mask_limitation() {
    let (_,w)=lua_develop::parse("s={MaskGroupBasedCorrections={{What='Correction',CorrectionMasks={{What='Mask/CircularGradient',MaskID='synthetic',Left=0.2,Top=0.2,Right=0.8,Bottom=0.8,Flipped=false,MaskInverted=false}}}}}","15.4").unwrap();
    assert!(
        w.iter().any(|w| w.contains("radial mask inversion")),
        "{w:?}"
    );
}

#[test]
fn neutral_color_variance_is_not_named_as_a_curve_blocker() {
    let (_,w)=lua_develop::parse("s={MaskGroupBasedCorrections={{What='Correction',LocalColorVariance={0,0,0},MainCurve={0,0,128,150,255,255},CorrectionMasks={{What='Mask/Image',MaskSubType=1,MaskID='synthetic'}}}}}","15.4").unwrap();
    assert!(w[0].contains("local tone curve"));
    assert!(!w[0].contains("color-variance"), "{w:?}");
}

#[test]
fn retouch_dab_state_commands_preserve_radius_flow_and_hardness() {
    let (r,w)=lua_develop::parse("s={RetouchAreas={{SpotType='heal',SourceX=0.7,OffsetY=0.6,Masks={{What='Mask/Paint',Radius=0.01,CenterWeight=0.7,Flow=1,Dabs={'r 0.02','d 0.2 0.4','r 0.04','f 0.5','h 0.2','d 0.3 0.5'}}}}}}","15.4").unwrap();
    assert!(w.is_empty(), "{w:?}");
    let engine_api::recipe::mask::RetouchTarget::Area { components } =
        &r.settings.locals.retouch[0].target
    else {
        panic!("area required")
    };
    let engine_api::recipe::MaskKind::Brush { strokes } = &components[0].kind else {
        panic!("brush required")
    };
    assert_eq!(strokes.len(), 2);
    assert_eq!(strokes[0].radius, 0.02);
    assert!((strokes[0].feather - 30.).abs() < 1e-4);
    assert_eq!(strokes[1].radius, 0.04);
    assert_eq!(strokes[1].flow, 50.);
    assert_eq!(strokes[1].feather, 80.);
    assert!(strokes.iter().all(|s| s.points.len() == 1));
}

#[test]
fn unsupported_modern_retouch_does_not_apply_a_stale_legacy_alias() {
    let (r,w)=lua_develop::parse("s={RetouchAreas={{SpotType='contentAware',pm_patch='synthetic'}},RetouchInfo={'spotType=clone,centerX=0.2,centerY=0.4,radius=0.03,sourceX=0.7,sourceY=0.6'}}","15.4").unwrap();
    assert!(r.settings.locals.retouch.is_empty());
    assert_eq!(w.len(), 1, "{w:?}");
    assert!(w[0].contains("content-aware"), "{w:?}");
    assert_eq!(diagnostics::entries(&r)["RetouchInfo"][0].status, "ignored");
}

#[test]
fn malformed_modern_retouch_also_blocks_stale_legacy_fallback() {
    let (r,w)=lua_develop::parse("s={RetouchAreas={{SpotType='clone',Opacity=0.5,opacity=0.8}},RetouchInfo={'spotType=clone,centerX=0.2,centerY=0.4,radius=0.03,sourceX=0.7,sourceY=0.6'}}","15.4").unwrap();
    assert!(r.settings.locals.retouch.is_empty());
    assert!(w.iter().any(|w| w.contains("RetouchAreas")));
    assert_eq!(diagnostics::entries(&r)["RetouchInfo"][0].status, "ignored");
}

#[test]
fn retouch_brushes_may_cross_the_image_boundary_without_clamping() {
    let (r,w)=lua_develop::parse("s={RetouchAreas={{SpotType='clone',SourceX=0.3,SourceY=0.4,Masks={{What='Mask/Paint',Radius=0.03,Dabs={'d -0.05 0.4','d 0.1 0.4'}}}}}}","15.4").unwrap();
    assert!(w.is_empty(), "{w:?}");
    let engine_api::recipe::mask::RetouchTarget::Area { components } =
        &r.settings.locals.retouch[0].target
    else {
        panic!("area required")
    };
    let engine_api::recipe::MaskKind::Brush { strokes } = &components[0].kind else {
        panic!("brush required")
    };
    assert_eq!(strokes[0].points[0][0], -0.05);
    r.validate().unwrap();
}

#[test]
fn ai_object_instance_metadata_names_instance_selection() {
    let (_,w)=lua_develop::parse("s={MaskGroupBasedCorrections={{What='Correction',CorrectionMasks={{What='Mask/Image',MaskSubType=0,ReferencePoint='0.5 0.5',InstanceIDs={{InstanceID=1}},InstanceBounds={{Left=0.2,Top=0.2,Right=0.8,Bottom=0.8}}}}}}}","15.4").unwrap();
    assert!(
        w.iter()
            .any(|w| w.contains("individual AI instance selection")),
        "{w:?}"
    );
}
