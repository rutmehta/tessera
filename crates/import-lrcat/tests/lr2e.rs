use import_lrcat::{diagnostics, lua_develop, xmp};

#[test]
fn lr2e_combined_import_has_one_entry_and_each_lane_runs_once() {
    for combined in [false, true] {
        let geo = if combined {
            ",PerspectiveUpright=1,UprightTransform_1='1,0,0,0,1,0,0.2,0,1'"
        } else {
            ""
        };
        let lua = format!("s={{ConvertToGrayscale=true{geo}}}");
        let geo = if combined {
            " crs:PerspectiveUpright=\"1\" crs:UprightTransform_1=\"1,0,0,0,1,0,0.2,0,1\""
        } else {
            ""
        };
        let xml = format!(
            r#"<rdf:Description xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#" xmlns:crs="http://ns.adobe.com/camera-raw-settings/1.0/" crs:ConvertToGrayscale="True"{geo}/>"#
        );
        for (mut r, _) in [
            lua_develop::parse(&lua, "15.4").unwrap(),
            xmp::parse(&xml, "15.4").unwrap(),
        ] {
            assert_eq!(r.history.entries.len(), 1);
            assert!(matches!(
                r.history.entries[0].meta.author,
                engine_api::recipe::Author::Import { .. }
            ));
            assert!(r.settings.color.monochrome.as_ref().unwrap().enabled);
            assert_eq!(r.settings.geometry.upright.homography.is_some(), combined);
            let entries = diagnostics::entries(&r);
            assert_eq!(entries["ConvertToGrayscale"].len(), 1);
            if combined {
                assert_eq!(entries["UprightTransform_1"].len(), 1);
            }
            r.validate().unwrap();
            let settings = r.settings.clone();
            assert!(r.undo().unwrap());
            assert_eq!(r.settings, r.history.base);
            assert!(r.redo().unwrap());
            assert_eq!(r.settings, settings);
        }
    }
}

#[test]
fn lr2e_legacy_rows_ignore_stale_modern_controls_even_without_legacy_sliders() {
    for legacy in ["", "Brightness=50,"] {
        let lua = format!(
            "s={{{legacy}Exposure2012=2,Clarity2012=70,Texture=30,Dehaze=20,ParametricShadows=60,ParametricDarks=40,ParametricLights=30,ParametricHighlights=20,ToneCurvePV2012={{0,0,128,160,255,255}},HDREditMode=1,ExtendedToneCurvePV2012={{0,0,255,300}}}}"
        );
        let xml = format!(
            r#"<rdf:Description xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#" xmlns:crs="http://ns.adobe.com/camera-raw-settings/1.0/" {} crs:Exposure2012="2" crs:Clarity2012="70" crs:Texture="30" crs:Dehaze="20" crs:ParametricShadows="60"/>"#,
            if legacy.is_empty() {
                ""
            } else {
                "crs:Brightness=\"50\""
            }
        );
        for (r, _) in [
            lua_develop::parse(&lua, "5.7").unwrap(),
            xmp::parse(&xml, "5.7").unwrap(),
        ] {
            let s = &r.settings.tone;
            assert!(s.legacy_pv2010.is_some());
            assert_eq!([s.exposure, s.clarity, s.texture, s.dehaze], [0.; 4]);
            assert_eq!(s.curves, Default::default());
            assert!(s.curves_extended.is_none());
            for key in [
                "Exposure2012",
                "Clarity2012",
                "Texture",
                "Dehaze",
                "ParametricShadows",
            ] {
                let source = r.unknown["lrcat_develop_source"]["properties"][key]
                    .as_str()
                    .expect("stale source retained");
                assert!(!source.is_empty());
                assert!(
                    diagnostics::entries(&r)[key]
                        .iter()
                        .any(|d| d.status == "ignored" && d.lane == "LR-2")
                );
            }
            r.validate().unwrap();
        }
    }
}
