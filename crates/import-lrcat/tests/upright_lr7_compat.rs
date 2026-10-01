//! Exact serialized-byte fingerprints taken before LR-7 production changes.
#[test]
fn unrelated_recipe_bytes_remain_identical() {
    let rows = [
        "s = { Exposure2012 = 1.25, FutureKey = { Exact = 'yes' } }",
        "s = { EnableDistractionRemoval = true, UprightVersion = 'opaque', UprightTransform_4 = '1,0,0,0,1,0,0,0,1' }",
        include_str!("data/lrc155/global.lua"),
        include_str!("data/lrc155/structures.lua"),
    ];
    for (row, expected) in rows.into_iter().zip([
        (11420, 0x35b9bf7bfe270b28),
        (11199, 0x01729e71086f5d4d),
        (15228, 0x14af73e31df40bc7),
        (18101, 0xbeb156c96528413e),
    ]) {
        let (recipe, _) = import_lrcat::lua_develop::parse(row, "15.4").unwrap();
        let bytes = recipe.to_json().unwrap();
        let hash = bytes.iter().fold(0xcbf29ce484222325u64, |h, b| {
            (h ^ u64::from(*b)).wrapping_mul(0x100000001b3)
        });
        assert_eq!((bytes.len(), hash), expected);
        eprintln!("LR-7 compatibility: {} bytes {hash:016x}", bytes.len());
    }
}
