//! B5-29c golden: the per-image output for the synthetic catalog is pinned to
//! what B5-29b (10f0a3f7) produced, so the streaming/indexing rewrite cannot
//! change recipes. Lua rows differ from B5-29b only by design (B5-29c item 4:
//! no generated `sidecar_xmp`, and `lrcat_develop_lua` holds unknown keys'
//! source instead of the whole literal), so those two keys are removed from Lua
//! rows before hashing; XMP and empty rows are hashed byte for byte. Image ids
//! derive from the catalog's absolute path, so `recipe.image_id` is cleared.
mod common;

use common::{Row, row_kind};
use engine_api::id::Digest;

const N: i64 = 2_000;
/// Computed by this test at 10f0a3f7 (B5-29b) with the same generator.
const GOLDEN: &str = "32b8574f77399f028e85ef7436f6e77315b0753d7b996792ffd88db7e7c01fa9";

pub fn digest(images: impl IntoIterator<Item = import_lrcat::ImportedImage>) -> String {
    let mut bytes = Vec::new();
    for mut image in images {
        image.recipe.image_id = None;
        if matches!(
            row_kind(image.catalog_id - 1000),
            Row::Lua | Row::LuaUnknown
        ) {
            image.recipe.unknown.remove("sidecar_xmp");
            image.recipe.unknown.remove("lrcat_develop_lua");
        }
        serde_json::to_writer(&mut bytes, &image).unwrap();
        bytes.push(b'\n');
    }
    Digest::derive("B5-29c golden", &bytes).to_string()
}

#[test]
fn synthetic_catalog_output_matches_b5_29b() {
    let dir = tempfile::tempdir().unwrap();
    let catalog = common::write(dir.path(), N);
    let plan = import_lrcat::import(&catalog).unwrap();
    assert_eq!(plan.images.len(), N as usize);
    let got = digest(plan.images);
    eprintln!("golden digest: {got}");
    assert_eq!(got, GOLDEN);
}
