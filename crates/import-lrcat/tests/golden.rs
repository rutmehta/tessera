//! Full retained-source baseline for B5-29c, including every Lua/XMP unknown
//! entry. Only the catalog-path-derived recipe image ID is normalized.
//! This deliberately supersedes the B5-29b translation-only digest; see HANDOFF.
mod common;

use engine_api::id::Digest;

const N: i64 = 2_000;
/// LR-6f recomputed on the integrated tree; identical to LR-3e because this
/// original fixture has no active Lens Blur. LR-3e retains exact RetouchInfo source plus shared approximate diagnostics,
/// uses decoder Import XMP/xmp provenance, and writes these 200 rows as schema 4.
const GOLDEN: &str = "87d28d71460e64ad1034fd0a5dc408a20a0452b7d37ccfd6f2a00ada8db3c0d5";

pub fn digest(images: impl IntoIterator<Item = import_lrcat::ImportedImage>) -> String {
    let mut bytes = Vec::new();
    for mut image in images {
        image.recipe.image_id = None;
        serde_json::to_writer(&mut bytes, &image).unwrap();
        bytes.push(b'\n');
    }
    Digest::derive("B5-29c golden", &bytes).to_string()
}

#[test]
fn synthetic_catalog_output_including_retained_source() {
    let dir = tempfile::tempdir().unwrap();
    let catalog = common::write(dir.path(), N);
    let plan = import_lrcat::import(&catalog).unwrap();
    assert_eq!(plan.images.len(), N as usize);
    let got = digest(plan.images);
    eprintln!("golden digest: {got}");
    assert_eq!(got, GOLDEN);
}

/// The streaming API gives the same images and plan as `import`, and
/// `PlanJson` writes exactly `serde_json::to_vec_pretty(&plan)`.
#[test]
fn streaming_matches_import_and_plan_json_is_byte_identical() {
    let dir = tempfile::tempdir().unwrap();
    let synthetic = common::write(dir.path(), 300);
    let fixture = import_lrcat::fixture::write(&dir.path().join("fx")).unwrap();
    let empty = common::write(&dir.path().join("empty"), 0);
    for catalog in [synthetic, fixture.catalog, empty] {
        let plan = import_lrcat::import(&catalog).unwrap();
        let mut out = Vec::new();
        let json = std::cell::RefCell::new(None);
        let mut streamed = Vec::new();
        let header = import_lrcat::import_each(
            &catalog,
            |plan| {
                assert!(plan.images.is_empty());
                *json.borrow_mut() = Some(import_lrcat::PlanJson::begin(&mut out, plan)?);
                Ok(())
            },
            |image| {
                json.borrow_mut().as_mut().unwrap().image(&image)?;
                streamed.push(image);
                Ok(())
            },
        )
        .unwrap();
        json.into_inner().unwrap().finish(&header).unwrap();
        assert_eq!(header.report, plan.report);
        assert_eq!(
            serde_json::to_vec(&streamed).unwrap(),
            serde_json::to_vec(&plan.images).unwrap()
        );
        assert!(
            out == serde_json::to_vec_pretty(&plan).unwrap(),
            "PlanJson bytes differ"
        );
    }
}

/// Active LR-6 rows supplement the unchanged original fixture. The byte digest
/// still serializes every ImportedImage field, normalizing only its image ID.
#[test]
fn lr6f_active_blur_and_inactive_depth_catalog_golden() {
    let dir = tempfile::tempdir().unwrap();
    let catalog = common::write(dir.path(), N);
    let db = rusqlite::Connection::open(&catalog).unwrap();
    for (id, source) in [
        (
            1004,
            "s = { LensBlur = { Active=true, BlurAmount=37, FocalRange='10 20 60 80' } }",
        ),
        (
            1005,
            "s = { LensBlur = { Active=true, BlurAmount=37 }, DepthMapInfo={ BaseRawDepthTable='synthetic-depth' } }",
        ),
        (1006, "s = { DepthMapInfo={ DepthSource=1 } }"),
        (
            1007,
            "s = { LensBlur={ Active=false }, DepthMapInfo={ DepthSource=1 } }",
        ),
    ] {
        db.execute(
            "UPDATE Adobe_imageDevelopSettings SET text=?1 WHERE image=?2",
            rusqlite::params![source, id],
        )
        .unwrap();
    }
    drop(db);
    let mut images = import_lrcat::import(&catalog).unwrap().images;
    let baseline: serde_json::Value =
        serde_json::from_str(include_str!("data/lr6f-main-inactive.json")).unwrap();
    for image in &mut images {
        image.recipe.image_id = None;
        if let Some(expected) = baseline.get(image.catalog_id.to_string()) {
            // Compare exact main serialization bytes, including source/history.
            assert_eq!(
                serde_json::to_vec(&*image).unwrap(),
                expected.as_str().unwrap().as_bytes()
            );
        }
    }
    let got = digest(images);
    eprintln!("LR-6f active golden digest: {got}");
    assert_eq!(
        got,
        "141018bf3d1b53071354c60090993dc0799d7f7fa35c0c8685cbbb55349ef6e0"
    );
}

/// A neighboring bundle must not change an ordinary import's serialized bytes.
#[test]
fn ordinary_import_golden_is_identical_with_smart_preview_bundle_present() {
    let dir = tempfile::tempdir().unwrap();
    let catalog = common::write(dir.path(), N);
    let before = import_lrcat::import(&catalog).unwrap();
    let index = import_lrcat::smart_previews::SmartPreviewIndex::new(&catalog);
    std::fs::create_dir_all(index.root()).unwrap();
    let after = import_lrcat::import(&catalog).unwrap();
    assert_eq!(serde_json::to_vec(&before).unwrap(), serde_json::to_vec(&after).unwrap());
    assert_eq!(digest(before.images), GOLDEN);
    assert_eq!(digest(after.images), GOLDEN);
}
