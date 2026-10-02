use mask_store::{MaskRaster, MaskStore};
#[test]
fn lr5_pins_survive_eviction_replace_and_remove() {
    let root = std::env::temp_dir().join(format!("tessera-lr5-pins-{}", std::process::id()));
    let store = MaskStore::new(&root, 1).unwrap();
    let key = [17; 32];
    for value in [0.25, 0.75] {
        store
            .put_pinned(&key, &MaskRaster::new(2, 1, vec![value; 2]).unwrap())
            .unwrap();
        assert_eq!(store.get(&key).unwrap().data(), &[value; 2]);
    }
    store.remove_pinned(&key).unwrap();
    assert!(store.get(&key).is_none());
    std::fs::remove_dir_all(root).unwrap();
}

#[cfg(unix)]
#[test]
fn lr5b_pin_write_does_not_enumerate_the_directory() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    let store = MaskStore::new(dir.path(), 0).unwrap();
    let pinned = dir.path().join("pinned");
    std::fs::create_dir(&pinned).unwrap();
    std::fs::set_permissions(&pinned, std::fs::Permissions::from_mode(0o300)).unwrap();
    let result = store.put_pinned(&[39; 32], &MaskRaster::new(1, 1, vec![1.]).unwrap());
    std::fs::set_permissions(&pinned, std::fs::Permissions::from_mode(0o700)).unwrap();
    assert!(
        result.is_ok(),
        "pin writes must not require directory read access: {result:?}"
    );
}
