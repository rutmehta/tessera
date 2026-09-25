use lens::{CalibrationSample, Profile, ProfileDatabase};

#[test]
fn lcp_retains_camera_identity_separately_from_lens_maker() {
    let xml = r#"<rdf:RDF xmlns:rdf="rdf" xmlns:crs="crs">
      <rdf:Description crs:Make="Canon" crs:Model="EOS R5"
        crs:LensMake="Sigma" crs:LensPrettyName="35mm F1.4 DG">
        <rdf:Description crs:FocalLength="35">
          <crs:PerspectiveModel crs:RadialDistortParam1="0.01"/>
        </rdf:Description>
      </rdf:Description></rdf:RDF>"#;
    let db = ProfileDatabase::from_lcp(xml).unwrap();
    let value = serde_json::to_value(&db.profiles[0]).unwrap();
    assert_eq!(value["maker"], "Sigma");
    assert_eq!(value["camera"]["maker"], "Canon");
    assert_eq!(value["camera"]["model"], "EOS R5");
}

#[test]
fn old_user_profile_without_camera_still_loads() {
    let p: Profile = serde_json::from_value(serde_json::json!({
        "maker": "Sigma", "model": "35mm F1.4 DG",
        "samples": [CalibrationSample::default()]
    }))
    .unwrap();
    p.validate().unwrap();
}

#[test]
fn lens_only_lookup_does_not_choose_a_camera_specific_profile() {
    let db = ProfileDatabase {
        profiles: vec![Profile {
            maker: "Sigma".into(),
            model: "35mm F1.4 DG".into(),
            camera: Some(lens::CameraIdentity {
                maker: "Canon".into(),
                model: "EOS R5".into(),
            }),
            samples: vec![CalibrationSample::default()],
        }],
    };
    assert!(db.find("Sigma", "35mm F1.4 DG").is_none());
}

#[test]
fn ambiguous_lens_makers_require_explicit_selection() {
    let profile = |maker: &str| Profile {
        maker: maker.into(),
        model: "35mm F1.4".into(),
        samples: vec![CalibrationSample::default()],
        ..Default::default()
    };
    let db = ProfileDatabase {
        profiles: vec![profile("Sigma"), profile("Canon")],
    };
    assert!(db.find("", "35mm F1.4").is_none());
    assert_eq!(db.find("Sigma", "35mm F1.4").unwrap().maker, "Sigma");
}
