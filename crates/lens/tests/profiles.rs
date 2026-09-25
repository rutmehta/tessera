use lens::*;
#[test]
fn lensfun_load_sample_match_save() {
    let xml = r#"<lensdatabase><lens><maker>Acme</maker><model>Zoom 24-70mm</model><calibration><distortion model="poly3" focal="24" k1="0.1"/><distortion model="poly3" focal="70" k1="0.3"/></calibration></lens></lensdatabase>"#;
    let db = ProfileDatabase::from_lensfun(xml).unwrap();
    let p = db.find("ACME", "zoom 24 70 mm").unwrap();
    assert!((p.sample(47.0, 4.0, 10.0).unwrap().distortion.k1 - 0.2).abs() < 1e-10);
    let path = std::env::temp_dir().join(format!("tessera-lens-{}.json", std::process::id()));
    save_user_profile(&path, p).unwrap();
    assert_eq!(load_user_profile(&path).unwrap(), *p);
    std::fs::remove_file(path).unwrap();
    assert!(ProfileDatabase::from_lensfun("<broken").is_err());
}
