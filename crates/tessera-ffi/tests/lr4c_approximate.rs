//! Invented Adobe forms only. Approximation is renderable + exact source + info,
//! with no user-facing warning.
use import_lrcat::{lua_develop, xmp};
use engine_api::recipe::{LocalAdjustment, MaskKind};
fn check(lua: &str, xml: &str, kind: &str) -> Vec<LocalAdjustment> {
    let source = format!("{{{{LocalExposure2012=1,CorrectionMasks={{{{{lua}}}}}}}}}");
    let (a, aw) = lua_develop::parse(&format!("s={{MaskGroupBasedCorrections={source}}}"), "15.4").unwrap();
    let fragment = format!(r#"<crs:MaskGroupBasedCorrections><rdf:Seq><rdf:li crs:LocalExposure2012="1"><crs:CorrectionMasks><rdf:Seq><rdf:li>{xml}</rdf:li></rdf:Seq></crs:CorrectionMasks></rdf:li></rdf:Seq></crs:MaskGroupBasedCorrections>"#);
    let packet = format!(r#"<rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#"><rdf:Description xmlns:crs="http://ns.adobe.com/camera-raw-settings/1.0/">{fragment}</rdf:Description></rdf:RDF>"#);
    let (b, bw) = xmp::parse(&packet, "15.4").unwrap();
    let mut groups = Vec::new();
    for (r, warnings, raw) in [(a, aw, source), (b, bw, fragment)] {
        assert!(warnings.is_empty(), "{warnings:?}");
        assert_eq!(r.unknown["lrcat_develop_source"]["properties"]["MaskGroupBasedCorrections"], raw);
        let diagnostics = r.unknown["lrcat_develop_diagnostics"].as_array().expect("info diagnostics");
        assert!(diagnostics.iter().any(|d| d["level"] == "info" && d["message"].as_str().unwrap().starts_with("approximate: ")));
        let g = &r.settings.locals.adjustments[0];
        assert_eq!(serde_json::to_value(&g.components[0]).unwrap()["kind"], kind);
        groups.push(g.clone());
    }
    groups
}
#[test]
fn lr4c_luminance_retains_source_without_warnings() {
    check(r#"What="Mask/RangeMask",CorrectionRangeMask={Type=2,LumRange="0.1 0.3 0.6 1"}"#,
        r#"<crs:What>Mask/RangeMask</crs:What><crs:CorrectionRangeMask crs:Type="2" crs:LumRange="0.1 0.3 0.6 1"/>"#, "luminance_range");
}
#[test]
fn lr4c_dab_commands_translate_to_individual_stamps_and_render() {
    let groups = check(r#"What="Mask/Paint",MaskID="synthetic",MaskValue=1,Radius=0.12,Flow=1,CenterWeight=1,Dabs={"d 0.125 0.5","r 0.1","f 0.5","h 1","d 0.875 0.5"}"#,
        r#"<crs:What>Mask/Paint</crs:What><crs:MaskID>synthetic</crs:MaskID><crs:MaskValue>1</crs:MaskValue><crs:Radius>0.12</crs:Radius><crs:Flow>1</crs:Flow><crs:CenterWeight>1</crs:CenterWeight><crs:Dabs><rdf:Seq><rdf:li>d 0.125 0.5</rdf:li><rdf:li>r 0.1</rdf:li><rdf:li>f 0.5</rdf:li><rdf:li>h 1</rdf:li><rdf:li>d 0.875 0.5</rdf:li></rdf:Seq></crs:Dabs>"#, "brush");
    for g in groups {
        let MaskKind::Brush { ref strokes } = g.components[0].kind else { panic!() };
        assert_eq!(strokes.len(), 2);
        assert_eq!(strokes[1].flow, 50.);
        let image = pipeline_cpu::Image::new(4,1,vec![vec![0.2;4];3]).unwrap();
        let alpha = pipeline_cpu::masks::rasterize(&image,&g,Default::default()).unwrap();
        assert_eq!(alpha, vec![1.,0.,0.,0.5]);
        let out = pipeline_cpu::locals_image(&image, &[g], Default::default()).unwrap();
        for (got, expected) in out.planes()[0].iter().zip([0.4,0.2,0.2,0.3]) { assert!((got-expected).abs()<1e-6); }
    }
}
#[test]
fn lr4c_color_models_assume_encoded_rgb_and_render() {
    for key in ["PointModels", "AreaModels"] {
        let groups = check(&format!(r#"What="Mask/RangeMask",CorrectionRangeMask={{Type=1,ColorAmount=0.1,{key}={{"1 1 1 0.2 0.4 0"}}}}"#),
            &format!(r#"<crs:What>Mask/RangeMask</crs:What><crs:CorrectionRangeMask crs:Type="1" crs:ColorAmount="0.1"><crs:{key}><rdf:Seq><rdf:li>1 1 1 0.2 0.4 0</rdf:li></rdf:Seq></crs:{key}></crs:CorrectionRangeMask>"#), "color_range");
        for g in groups {
            let image = pipeline_cpu::Image::new(2,1,vec![vec![1.,0.];3]).unwrap();
            assert_eq!(pipeline_cpu::masks::rasterize(&image,&g,Default::default()).unwrap(), vec![1.,0.]);
        }
    }
}
#[test]
fn lr4c_component_inversion_precedes_range_intersection() {
    let (r, _) = lua_develop::parse(r#"s={MaskGroupBasedCorrections={{CorrectionMasks={{What="Mask/Gradient",FullX=0,FullY=0,ZeroX=1,ZeroY=0,MaskInverted=true,CorrectionRangeMask={LumMin=0.4,LumMax=0.5}}}}}}"#, "15.4").unwrap();
    let g = &r.settings.locals.adjustments[0];
    let image = pipeline_cpu::Image::new(2,1,vec![vec![0.18,0.9];3]).unwrap();
    assert_eq!(pipeline_cpu::masks::rasterize(&image,g,Default::default()).unwrap(),vec![0.25,0.]);
}
#[test]
fn lr4c_documented_metadata_does_not_block_approximation() {
    let (r,w) = lua_develop::parse(r#"s={MaskGroupBasedCorrections={{CorrectionSyncID="synthetic",LocalToningHue=0,LocalToningSaturation=0,CorrectionMasks={{What="Mask/CircularGradient",MaskID="invented",MaskValue=1,Midpoint=50,Roundness=0,Left=0.2,Right=0.8,Top=0.1,Bottom=0.9,Angle=30,Flipped=true}}}}}"#, "15.4").unwrap();
    assert!(w.is_empty(), "{w:?}");
    assert_eq!(r.settings.locals.adjustments.len(),1);
    assert!(r.unknown["lrcat_develop_diagnostics"].is_array());
}
