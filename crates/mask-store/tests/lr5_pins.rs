use mask_store::{MaskRaster, MaskStore};
#[test]
fn lr5_pins_survive_eviction_replace_and_remove() {
    let root = std::env::temp_dir().join(format!("tessera-lr5-pins-{}", std::process::id()));
    let store = MaskStore::new(&root, 1).unwrap();
    let key = [17; 32];
    for value in [0.25, 0.75] {
        store.put_pinned(&key, &MaskRaster::new(2, 1, vec![value; 2]).unwrap()).unwrap();
        assert_eq!(store.get(&key).unwrap().data(), &[value; 2]);
    }
    store.remove_pinned(&key).unwrap();
    assert!(store.get(&key).is_none());
    std::fs::remove_dir_all(root).unwrap();
}
