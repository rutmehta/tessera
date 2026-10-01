//! Invented Lua and XMP -> actual importer -> CPU local exposure.
use import_lrcat::{lua_develop, xmp};
use pipeline_cpu::{Image, locals_image, masks::MaskOptions};
#[test]
fn lr4b_import_cpu_four_bounds_radial_and_subtypes() {
    let cases = [
        (
            r#"What="Mask/RangeMask", CorrectionRangeMask={Type=2,LumRange="0 0.25 0.5 1"}"#,
            r#"<crs:What>Mask/RangeMask</crs:What><crs:CorrectionRangeMask crs:Type="2" crs:LumRange="0 0.25 0.5 1"/>"#,
            vec![0.014349875, 0.05087609, 0.21404114, 0.52252155],
            vec![0.021524812, 0.10175218, 0.428_082_3, 0.783_782_3],
            None,
        ),
        (
            r#"What="Mask/CircularGradient", Left=0.25,Right=0.75,Top=0,Bottom=1,Flipped=false"#,
            r#"<crs:What>Mask/CircularGradient</crs:What><crs:Left>0.25</crs:Left><crs:Right>0.75</crs:Right><crs:Top>0</crs:Top><crs:Bottom>1</crs:Bottom><crs:Flipped>false</crs:Flipped>"#,
            vec![0.25; 4],
            vec![0.5, 0.25, 0.25, 0.5],
            None,
        ),
        (
            r#"What="Mask/RangeMask",CorrectionRangeMask={Type=2,LumMin=0.25,LumMax=0.5}"#,
            r#"<crs:What>Mask/RangeMask</crs:What><crs:CorrectionRangeMask crs:Type="2" crs:LumMin="0.25" crs:LumMax="0.5"/>"#,
            // Interior samples avoid the pre-existing f32 luma rounding at a
            // discontinuous hard bound; four-bound endpoints are tested above.
            vec![0.014349875, 0.116016135, 0.16068268, 0.52252155],
            vec![0.014349875, 0.23203227, 0.32136536, 0.52252155],
            None,
        ),
        (
            r#"What="Mask/RangeMask",CorrectionRangeMask={Type=3,DepthMin=0.25,DepthMax=0.75}"#,
            r#"<crs:What>Mask/RangeMask</crs:What><crs:CorrectionRangeMask crs:Type="3" crs:DepthMin="0.25" crs:DepthMax="0.75"/>"#,
            vec![0.25; 4],
            vec![0.25, 0.5, 0.5, 0.25],
            Some([0., 0.25, 0.75, 1.]),
        ),
    ];
    for (lua, xml, input, want, depth) in cases {
        let lua = format!(
            r#"s={{ProcessVersion="15.4",MaskGroupBasedCorrections={{{{LocalExposure2012=1,CorrectionMasks={{{{{lua}}}}}}}}}}}"#
        );
        let xml = format!(
            r#"<rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#" xmlns:crs="http://ns.adobe.com/camera-raw-settings/1.0/"><rdf:Description><crs:MaskGroupBasedCorrections><rdf:Seq><rdf:li rdf:parseType="Resource"><crs:LocalExposure2012>1</crs:LocalExposure2012><crs:CorrectionMasks><rdf:Seq><rdf:li rdf:parseType="Resource">{xml}</rdf:li></rdf:Seq></crs:CorrectionMasks></rdf:li></rdf:Seq></crs:MaskGroupBasedCorrections></rdf:Description></rdf:RDF>"#
        );
        for recipe in [
            lua_develop::parse(&lua, "15.4").unwrap().0,
            xmp::parse(&xml, "15.4").unwrap().0,
        ] {
            assert_eq!(recipe.settings.locals.adjustments.len(), 1);
            assert!(recipe.unknown.contains_key("lrcat_develop_source"));
            assert!(recipe.unknown["lrcat_develop_diagnostics"].is_array());
            let img = Image::new(4, 1, vec![input.clone(); 3]).unwrap();
            let out = locals_image(
                &img,
                &recipe.settings.locals.adjustments,
                MaskOptions {
                    depth: depth.as_ref().map(|d| d.as_slice()),
                    ..Default::default()
                },
            )
            .unwrap();
            let mut max = 0f32;
            for channel in out.planes() {
                for (got, want) in channel.iter().zip(&want) {
                    max = max.max((got - want).abs());
                }
            }
            assert!(max <= 1e-6, "maximum channel error: {max}");
            eprintln!("LR-4b Lua/XMP CPU maximum channel error {max}; tolerance 1e-6");
        }
    }
}
