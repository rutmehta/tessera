//! Full retained-source baseline for B5-29c, including every Lua/XMP unknown
//! entry. Only the catalog-path-derived recipe image ID is normalized.
//! This deliberately supersedes the B5-29b translation-only digest; see HANDOFF.
mod common;

use engine_api::id::Digest;

const N: i64 = 2_000;
/// LR-3d retains exact RetouchInfo source plus approximate info diagnostics,
/// consolidates import history, and writes these 200 feature rows as schema 4.
const GOLDEN: &str = "7022e432ed77c0c42227de06f331090e8a43d4e06ca4749959f136a26b659763";

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
