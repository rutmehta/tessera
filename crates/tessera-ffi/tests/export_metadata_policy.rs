#[test]
fn metadata_policy_settings_round_trip() {
    for policy in [
        "all",
        "copyright",
        "copyright_and_contact",
        "all_except_camera",
        "none",
    ] {
        let input = format!(
            r#"{{"metadata":"{policy}","remove_person_info":true,"remove_location":true,"keywords_as_hierarchy":false}}"#
        );
        let normalized = tessera_ffi::normalize_export_settings(input).unwrap();
        let output: serde_json::Value = serde_json::from_str(&normalized).unwrap();
        assert_eq!(output["metadata"], policy);
        assert_eq!(output["remove_person_info"], true);
        assert_eq!(output["remove_location"], true);
        assert_eq!(output["keywords_as_hierarchy"], false);
    }
}
