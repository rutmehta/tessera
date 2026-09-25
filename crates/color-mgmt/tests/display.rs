use color_mgmt::*;
#[cfg(target_os = "macos")]
#[test]
fn unavailable_display_falls_back_to_srgb() {
    let mut registry = Registry::new();
    let result = registry.display_profile(u32::MAX).unwrap();
    assert_eq!(result.display_id, u32::MAX);
    assert!(std::sync::Arc::ptr_eq(
        &result.profile,
        &registry.builtin(Builtin::Srgb).unwrap()
    ));
}

#[test]
fn display_discovery_returns_valid_deduplicated_profiles() {
    let mut registry = Registry::new();
    #[cfg(target_os = "macos")]
    {
        let displays = registry.display_profiles().unwrap();
        eprintln!("CoreGraphics discovered {} active displays", displays.len());
        let mut ids = std::collections::HashSet::new();
        for display in displays {
            assert!(ids.insert(display.display_id));
            assert!(std::sync::Arc::ptr_eq(
                &display.profile,
                &registry.load_bytes(display.profile.icc_bytes()).unwrap()
            ));
        }
    }
    #[cfg(not(target_os = "macos"))]
    assert!(registry.display_profiles().is_err());
}
