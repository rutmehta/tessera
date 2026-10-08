//! Exact serialized-byte fingerprints taken before LR-7 production changes.
#[test]
fn unrelated_recipe_bytes_remain_identical() {
    let rows = [
        "s = { Exposure2012 = 1.25, FutureKey = { Exact = 'yes' } }",
        "s = { EnableDistractionRemoval = true, UprightVersion = 'opaque', UprightTransform_4 = '1,0,0,0,1,0,0,0,1' }",
        include_str!("data/lrc155/global.lua"),
        include_str!("data/lrc155/structures.lua"),
    ];
    // ENG-7b: every row re-pinned because Remove CA defaults to off (was
    // (11420, 0x35b9bf7bfe270b28), (11199, 0x01729e71086f5d4d),
    // (15716, 0x76a536a0a0d32c5b), (20442, 0x0c5aebc9285d9719)); only the
    // remove_chromatic_aberration value and its history patch differ.
    for (row, expected) in rows.into_iter().zip([
        (11422, 0x5298de2b98d75640),
        (11201, 0x2650e87b9393d40f),
        // ENG-7: LensProfileEnable=1 adds one info note (lens profile unavailable).
        (15850, 0x80d6eda6bd3c1a60),
        // LR-3f: structures.lua now translates its explicit-source heal.
        (20309, 0x8bb392fd1c144607),
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
