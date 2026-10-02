//! LR-11 synthetic fixtures; no catalog rows or identifiers.
use import_lrcat::{diagnostics, lua_develop};

const GRADIENT: &str =
    "CorrectionMasks={{What='Mask/Gradient',MaskID='synthetic',FullX=0,FullY=0,ZeroX=1,ZeroY=0}}";
#[test]
fn local_adjustments_are_renderable_recipe_fields() {
    for (source, field) in [
        ("MainCurve={0,0,128,160,255,255}", "curves"),
        (
            "LocalPointColors={'0,0.8,0.5,0.1,0,0,0.5,0,0.25,0.75,1,0,0.25,0.75,1,0,0.25,0.75,1'}",
            "point_colors",
        ),
        (
            "LocalToningHue=120,LocalToningSaturation=40",
            "color_overlay",
        ),
        ("LocalDefringe=50", "defringe"),
    ] {
        let (r, w) = lua_develop::parse(
            &format!("s={{MaskGroupBasedCorrections={{{{{source},{GRADIENT}}}}}}}"),
            "15.4",
        )
        .unwrap();
        assert!(w.is_empty(), "{field}: {w:?}");
        let v = serde_json::to_value(&r).unwrap();
        let path = format!("/settings/locals/adjustments/0/params/{field}");
        assert!(v.pointer(&path).is_some_and(|v| !v.is_null()), "{field}");
        assert!(
            diagnostics::entries(&r)
                .values()
                .flatten()
                .any(|d| d.status == "approximate" && d.field.as_deref() == Some(path.as_str())),
            "{field}"
        );
        assert!(
            r.unknown["lrcat_develop_source"]["properties"]
                .get("MaskGroupBasedCorrections")
                .is_some()
        );
        assert_eq!(r.history.entries.len(), 1);
        assert_eq!(engine_api::recipe::required_schema_version(&r), 4);
    }
}
#[test]
fn radial_conflict_has_named_note() {
    let (_,w)=lua_develop::parse("s={MaskGroupBasedCorrections={{LocalExposure2012=1,CorrectionMasks={{What='Mask/CircularGradient',Left=0.2,Top=0.2,Right=0.8,Bottom=0.8,Flipped=true,MaskInverted=true}}}}}","15.4").unwrap();
    assert!(
        w.iter()
            .any(|w| w.contains("radial mask inversion flags conflict")),
        "{w:?}"
    );
}

#[test]
fn lua_and_xmp_fixtures_map_identically_and_keep_exact_source() {
    for name in [
        "curve",
        "point-color",
        "overlay",
        "defringe",
        "radial-conflict",
    ] {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data/lr11");
        let lua = std::fs::read_to_string(path.join(format!("{name}.lua"))).unwrap();
        let xml = std::fs::read_to_string(path.join(format!("{name}.xmp"))).unwrap();
        let (a, aw) = lua_develop::parse(&lua, "15.4").unwrap();
        let (b, bw) = import_lrcat::xmp::parse(&xml, "15.4").unwrap();
        assert_eq!(a.settings.locals, b.settings.locals, "{name}");
        if name == "radial-conflict" {
            assert!(a.settings.locals.adjustments.is_empty());
            for warnings in [&aw, &bw] {
                assert!(
                    warnings
                        .iter()
                        .any(|w| w.contains("radial mask inversion flags conflict"))
                );
            }
        } else {
            assert!(aw.is_empty(), "{name}: {aw:?}");
            assert!(bw.is_empty(), "{name}: {bw:?}");
            for r in [&a, &b] {
                assert_eq!(r.history.entries.len(), 1);
                assert!(matches!(
                    r.history.entries[0].meta.author,
                    engine_api::recipe::history::Author::Import { .. }
                ));
                assert!(
                    diagnostics::entries(r)
                        .values()
                        .flatten()
                        .any(|d| d.lane == "LR-11"
                            && d.status == "approximate"
                            && d.level == "info")
                );
            }
        }
        let raw_a = a.unknown["lrcat_develop_source"]["properties"]["MaskGroupBasedCorrections"]
            .as_str()
            .unwrap();
        let raw_b = b.unknown["lrcat_develop_source"]["properties"]["MaskGroupBasedCorrections"]
            .as_str()
            .unwrap();
        assert!(lua.contains(raw_a));
        assert!(xml.contains(raw_b));
        for r in [a, b] {
            let round = engine_api::recipe::Recipe::from_json(&r.to_json().unwrap()).unwrap();
            assert_eq!(round.settings, r.settings);
        }
    }
}

#[test]
fn malformed_local_payloads_retain_parent_atomically() {
    for source in [
        "MainCurve={0,0,128,150,64,255}",
        "MainCurve={0,0,255,300}",
        "ExtendedMainCurve={0,0,0,255}",
        "LocalPointColors={'0,1,2'}",
        "LocalPointColors={{SrcHue=0,SrcSat=0.5,SrcLum=0.5,UnknownFutureField=1}}",
        "LocalToningHue=120,LocalToningSaturation=101",
        "LocalToningHue=361,LocalToningSaturation=50",
        "LocalDefringe=-1",
        "LocalDefringe=101",
    ] {
        let row = format!("s={{MaskGroupBasedCorrections={{{{{source},{GRADIENT}}}}}}}");
        let (r, w) = lua_develop::parse(&row, "15.4").unwrap();
        assert!(!w.is_empty(), "{source}");
        assert!(r.settings.locals.adjustments.is_empty(), "{source}");
        assert!(
            r.unknown["lrcat_develop_source"]["properties"]
                .get("MaskGroupBasedCorrections")
                .is_some()
        );
        assert!(
            !diagnostics::entries(&r)
                .values()
                .flatten()
                .any(|d| d.lane == "LR-11")
        );
    }
}

#[test]
fn ordinary_and_extended_channel_curves_keep_channel_fallbacks() {
    let (r,w)=lua_develop::parse(&format!("s={{MaskGroupBasedCorrections={{{{MainCurve={{0,0,255,255}},RedCurve={{0,0,255,127.5}},ExtendedBlueCurve={{0,0,510,600}},{GRADIENT}}}}}}}"),"15.4").unwrap();
    assert!(w.is_empty(), "{w:?}");
    let p = &r.settings.locals.adjustments[0].params;
    assert_eq!(p.curves.as_ref().unwrap().red.0[1].y, 0.5);
    let e = p.curves_extended.as_ref().unwrap();
    assert_eq!(e.red, p.curves.as_ref().unwrap().red);
    assert_eq!(e.blue.0[1].x, 2.);
    assert!((e.blue.0[1].y - 600. / 255.).abs() < 1e-6);
}

#[test]
fn indexed_local_point_color_sdk_resources_share_global_decoder() {
    let source = "LocalPointColors={[1]={SrcHue=0,SrcSat=0.5,SrcLum=0.5,HueShift=0.5}}";
    let (r, w) = lua_develop::parse(
        &format!("s={{MaskGroupBasedCorrections={{{{{source},{GRADIENT}}}}}}}"),
        "15.4",
    )
    .unwrap();
    assert!(w.is_empty(), "{w:?}");
    assert_eq!(
        r.settings.locals.adjustments[0]
            .params
            .point_colors
            .as_ref()
            .unwrap()[0]
            .hue_shift,
        30.
    );
}

#[test]
fn empty_local_point_controls_do_not_create_new_recipe_fields_or_lr11_notes() {
    for source in [
        "LocalPointColors={}",
        "LocalPointColors=''",
        "LocalDefringe=0",
        "LocalPointColors={'-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1'}",
    ] {
        let (r, w) = lua_develop::parse(
            &format!("s={{MaskGroupBasedCorrections={{{{{source},{GRADIENT}}}}}}}"),
            "15.4",
        )
        .unwrap();
        assert!(w.is_empty(), "{source}: {w:?}");
        let p = &r.settings.locals.adjustments[0].params;
        assert!(p.point_colors.is_none());
        assert_eq!(p.defringe, 0.);
        assert!(
            !diagnostics::entries(&r)
                .values()
                .flatten()
                .any(|d| d.lane == "LR-11")
        );
    }
}
