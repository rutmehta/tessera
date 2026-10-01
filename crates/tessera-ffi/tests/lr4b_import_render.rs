//! Invented Lua and XMP -> actual importer -> CPU local exposure.
use import_lrcat::{lua_develop, xmp};
use pipeline_cpu::{Image, locals_image, masks::MaskOptions};
#[test]
fn lr4b_import_cpu_four_bounds_and_radial() {
    let cases = [
        (
            r#"What="Mask/RangeMask", CorrectionRangeMask={Type=2,LumRange="0 0.25 0.5 1"}"#,
            r#"<crs:What>Mask/RangeMask</crs:What><crs:CorrectionRangeMask crs:Type="2" crs:LumRange="0 0.25 0.5 1"/>"#,
            vec![0.125, 0.25, 0.5, 0.75],
            vec![0.1875, 0.5, 1., 1.125],
        ),
        (
            r#"What="Mask/CircularGradient", Left=0.25,Right=0.75,Top=0,Bottom=1,Flipped=false"#,
            r#"<crs:What>Mask/CircularGradient</crs:What><crs:Left>0.25</crs:Left><crs:Right>0.75</crs:Right><crs:Top>0</crs:Top><crs:Bottom>1</crs:Bottom><crs:Flipped>false</crs:Flipped>"#,
            vec![0.25; 4],
            vec![0.5, 0.25, 0.25, 0.5],
        ),
    ];
    for (lua, xml, input, want) in cases {
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
            assert!(!recipe.unknown.contains_key("lrcat_develop_source"));
            let img = Image::new(4, 1, vec![input.clone(); 3]).unwrap();
            let out = locals_image(
                &img,
                &recipe.settings.locals.adjustments,
                MaskOptions::default(),
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
