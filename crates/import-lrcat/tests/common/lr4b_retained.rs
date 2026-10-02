//! Invented unsupported cases, pinned against a88440a4 without changing it.
pub fn digests() -> Vec<(usize, String)> {
    let mut out:Vec<_> = [
        r#"What="Mask/Paint", Radius=0.05, Flow=0.5, CenterWeight=0.75, MaskValue=1, Dabs={"d 0.125 0.5","r 0.1","f 0.25","h 0.5","d 0.875 0.5"}"#,
        r#"What="Mask/RangeMask",CorrectionRangeMask={Type=1,ColorAmount=0.5,PointModels={"0.2 0.3 0.4 0.5 0.5 0"}}"#,
        r#"What="Mask/RangeMask",CorrectionRangeMask={Type=1,AreaModels={"opaque"}}"#,
        r#"What="Mask/RangeMask",CorrectionRangeMask={Type=9,LumMin=0,LumMax=1}"#,
        r#"What="Mask/RangeMask",CorrectionRangeMask={LumRange="opaque"}"#,
        r#"What="Mask/Image",MaskSyncID="invented""#,
        r#"What="Mask/CircularGradient",Flipped=true,MaskInverted=true"#,
    ].iter().map(|mask| {
        let lua=format!(r#"s={{ProcessVersion="15.4",MaskGroupBasedCorrections={{{{CorrectionMasks={{{{{mask}}}}}}}}}}}"#);
        let r=import_lrcat::lua_develop::parse(&lua,"15.4").unwrap().0;
        let bytes=serde_json::to_vec(&r).unwrap();
        (bytes.len(),engine_api::id::Digest::derive("LR-4b retained recipe",&bytes).to_string())
    }).collect();
    for mask in [
        r#"<crs:What>Mask/Paint</crs:What><crs:Radius>0.05</crs:Radius><crs:Dabs><rdf:Seq><rdf:li>d 0.125 0.5</rdf:li></rdf:Seq></crs:Dabs>"#,
        r#"<crs:What>Mask/RangeMask</crs:What><crs:CorrectionRangeMask crs:Type="1" crs:ColorAmount="0.5"><crs:PointModels><rdf:Seq><rdf:li>0.2 0.3 0.4 0.5 0.5 0</rdf:li></rdf:Seq></crs:PointModels></crs:CorrectionRangeMask>"#,
        r#"<crs:What>Mask/RangeMask</crs:What><crs:CorrectionRangeMask crs:LumRange="opaque"/>"#,
        r#"<crs:What>Mask/Image</crs:What><crs:MaskSyncID>invented</crs:MaskSyncID>"#,
    ] {
        let xml = format!(
            r#"<rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#" xmlns:crs="http://ns.adobe.com/camera-raw-settings/1.0/"><rdf:Description crs:ProcessVersion="15.4"><crs:MaskGroupBasedCorrections><rdf:Seq><rdf:li rdf:parseType="Resource"><crs:CorrectionMasks><rdf:Seq><rdf:li rdf:parseType="Resource">{mask}</rdf:li></rdf:Seq></crs:CorrectionMasks></rdf:li></rdf:Seq></crs:MaskGroupBasedCorrections></rdf:Description></rdf:RDF>"#
        );
        let r = import_lrcat::xmp::parse(&xml, "15.4").unwrap().0;
        let bytes = serde_json::to_vec(&r).unwrap();
        out.push((
            bytes.len(),
            engine_api::id::Digest::derive("LR-4b retained recipe", &bytes).to_string(),
        ));
    }
    out
}
