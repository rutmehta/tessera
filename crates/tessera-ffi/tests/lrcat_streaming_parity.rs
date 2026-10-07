//! Generated-catalog FFI summary and complete bundle parity with the crate.
#[path = "../../import-lrcat/tests/common/mod.rs"]
mod common;
use tessera_ffi::Engine;

#[test]
fn ffi_streaming_summary_and_bundle_match_crate() {
    let temp = tempfile::tempdir().unwrap();
    let small = common::write(&temp.path().join("comparison"), 200);
    let expected = import_lrcat::import(&small).unwrap();
    let engine = Engine::open(temp.path().join("support2").to_string_lossy().into_owned()).unwrap();
    let import = engine
        .clone()
        .open_lrcat(small.to_string_lossy().into_owned())
        .unwrap();
    let summary = import.summary();
    assert_eq!(
        summary,
        tessera_ffi::inspect_lrcat(small.to_string_lossy().into_owned()).unwrap()
    );
    let crate_summary = import_lrcat::inspect(&small).unwrap();
    assert_eq!(summary.images as usize, crate_summary.images);
    assert_eq!(
        summary.virtual_copies as usize,
        crate_summary.virtual_copies
    );
    assert_eq!(summary.faces as usize, crate_summary.faces);
    assert_eq!(summary.collections as usize, crate_summary.albums);
    assert_eq!(
        summary.edited as usize,
        expected
            .images
            .iter()
            .filter(|i| !i.recipe.history.entries.is_empty())
            .count()
    );
    let expected_bytes = serde_json::to_vec(&expected).unwrap().len()
        + serde_json::to_vec_pretty(&expected.library).unwrap().len()
        + expected
            .images
            .iter()
            .filter(|i| i.master_image.is_none())
            .map(|i| {
                serde_json::to_vec_pretty(&sidecar::RecipeDocument {
                    recipe: i.recipe.clone(),
                    ..Default::default()
                })
                .unwrap()
                .len()
                    + 2048
            })
            .sum::<usize>();
    assert_eq!(summary.estimated_bytes, expected_bytes as u64);
    let history = summary
        .unsupported
        .iter()
        .find(|i| i.category == "History")
        .unwrap();
    assert_eq!(
        history.count as usize,
        expected
            .images
            .iter()
            .map(|i| i.history.len() + i.snapshots.len())
            .sum::<usize>()
    );
    let mut options = import.default_options().unwrap();
    // Not the catalog's own folder: library.json never lands beside the
    // .lrcat (A-LR8 minor, enforced by apply since LR-13b).
    options.library_folder = temp
        .path()
        .join("comparison-library")
        .to_string_lossy()
        .into_owned();
    let report = import.apply(options, None).unwrap();
    let actual: serde_json::Value = serde_json::from_reader(
        std::fs::File::open(std::path::Path::new(&report.bundle_path).join("import-plan.json"))
            .unwrap(),
    )
    .unwrap();
    // Parse both serialized documents: to_value promotes f32 to f64 and
    // does not represent the decimal bytes written by serde_json::to_vec.
    let reference = serde_json::from_slice(&serde_json::to_vec(&expected).unwrap()).unwrap();
    if let Some(difference) = first_difference(&actual, &reference, "$") {
        panic!("{difference}");
    }
}

fn first_difference(a: &serde_json::Value, b: &serde_json::Value, path: &str) -> Option<String> {
    use serde_json::Value;
    if a == b {
        return None;
    }
    match (a, b) {
        (Value::Object(a), Value::Object(b)) if a.len() == b.len() => {
            for (key, value) in b {
                let Some(actual) = a.get(key) else {
                    return Some(format!("{path}: missing {key}"));
                };
                if let Some(difference) = first_difference(actual, value, &format!("{path}.{key}"))
                {
                    return Some(difference);
                }
            }
        }
        (Value::Array(a), Value::Array(b)) if a.len() == b.len() => {
            for (i, (a, b)) in a.iter().zip(b).enumerate() {
                if let Some(difference) = first_difference(a, b, &format!("{path}[{i}]")) {
                    return Some(difference);
                }
            }
        }
        _ => {
            return Some(format!(
                "{path}: {} != {}",
                a.to_string().chars().take(180).collect::<String>(),
                b.to_string().chars().take(180).collect::<String>()
            ));
        }
    }
    Some(format!("{path}: values differ"))
}

#[test]
fn ffi_retains_develop_sources_and_publishes_oversized_cells() {
    let temp = tempfile::tempdir().unwrap();
    let catalog = common::write(&temp.path().join("sources"), 5);
    let c = rusqlite::Connection::open(&catalog).unwrap();
    let large = "x".repeat(import_lrcat::MAX_CELL_BYTES + 1);
    c.execute(
        "UPDATE Adobe_imageDevelopSettings SET text=?1 WHERE image=1001",
        [&large],
    )
    .unwrap();
    c.execute(
        "UPDATE Adobe_imageDevelopSettings SET text='not lua' WHERE image IN (1002,1003)",
        [],
    )
    .unwrap();
    c.execute("UPDATE Adobe_imageDevelopSettings SET text='s = { UprightVersion = 151388160, RetouchAreas = {}, ExtendedToneCurvePV2012 = {0,0,255,255} }' WHERE image=1004", []).unwrap();
    drop(c);
    let inspect = tessera_ffi::inspect_lrcat(catalog.to_string_lossy().into_owned()).unwrap();
    let engine = Engine::open(temp.path().join("support").to_string_lossy().into_owned()).unwrap();
    let import = engine
        .clone()
        .open_lrcat(catalog.to_string_lossy().into_owned())
        .unwrap();
    for summary in [inspect, import.summary()] {
        for id in 1001..=1003 {
            let issue = summary
                .unsupported
                .iter()
                .find(|i| {
                    i.reason.starts_with(&format!("image {id}:"))
                        && i.reason.contains("imported as unedited")
                })
                .unwrap();
            assert_eq!(issue.count, 1);
        }
    }
    // Publication must use the pinned source cells even after the catalog changes.
    rusqlite::Connection::open(&catalog)
        .unwrap()
        .execute("UPDATE Adobe_imageDevelopSettings SET text='changed'", [])
        .unwrap();
    let mut options = import.default_options().unwrap();
    options.library_folder = temp.path().join("library").to_string_lossy().into_owned();
    for r in &mut options.relocations {
        r.to = temp.path().join("absent").to_string_lossy().into_owned();
    }
    let report = import.apply(options.clone(), None).unwrap();
    let bundle = std::path::Path::new(&report.bundle_path);
    let plan: serde_json::Value =
        serde_json::from_slice(&std::fs::read(bundle.join("import-plan.json")).unwrap()).unwrap();
    let source = |id| {
        plan["images"]
            .as_array()
            .unwrap()
            .iter()
            .find(|i| i["catalog_id"] == id)
            .unwrap()["recipe"]["lrcat_develop_source"]
            .clone()
    };
    let oversized = source(1001);
    assert_eq!(oversized["truncated"], false);
    assert_eq!(oversized["cell"]["status"], "externalized");
    let retained = bundle.join(oversized["cell"]["path"].as_str().unwrap());
    assert_eq!(std::fs::read(&retained).unwrap(), large.as_bytes());
    assert_eq!(source(1002)["text"], "not lua");
    assert_eq!(source(1003)["text"], "not lua");
    assert_eq!(source(1004)["shape"], "lua-values");
    assert_eq!(source(1004)["properties"]["UprightVersion"], "151388160");
    assert_eq!(source(1004)["properties"]["RetouchAreas"], "{}");
    assert_eq!(
        source(1004)["properties"]["ExtendedToneCurvePV2012"],
        "{0,0,255,255}"
    );
    // A pre-existing plan must not prevent repairing missing side files.
    std::fs::remove_file(&retained).unwrap();
    import.apply(options.clone(), None).unwrap();
    drop(import);
    assert_eq!(std::fs::read(&retained).unwrap(), large.as_bytes());

    // A later session can introduce another cell while reusing the same bundle.
    let later_large = "y".repeat(import_lrcat::MAX_CELL_BYTES + 1);
    rusqlite::Connection::open(&catalog)
        .unwrap()
        .execute(
            "UPDATE Adobe_imageDevelopSettings SET text=?1 WHERE image=1002",
            [&later_large],
        )
        .unwrap();
    let later = engine
        .clone()
        .open_lrcat(catalog.to_string_lossy().into_owned())
        .unwrap();
    later.apply(options, None).unwrap();
    assert!(
        std::fs::read_dir(bundle.join("large"))
            .unwrap()
            .any(|entry| {
                std::fs::read(entry.unwrap().path()).unwrap() == later_large.as_bytes()
            })
    );
    assert_eq!(std::fs::read(&retained).unwrap(), large.as_bytes());

    // Reusing a row's filename with different bytes must preserve the old bundle.
    rusqlite::Connection::open(&catalog)
        .unwrap()
        .execute(
            "UPDATE Adobe_imageDevelopSettings SET text=?1 WHERE image=1001",
            [&later_large],
        )
        .unwrap();
    let conflicting = engine
        .open_lrcat(catalog.to_string_lossy().into_owned())
        .unwrap();
    let mut options = conflicting.default_options().unwrap();
    options.library_folder = temp.path().join("library").to_string_lossy().into_owned();
    assert!(
        conflicting
            .apply(options, None)
            .unwrap_err()
            .to_string()
            .contains("retained source cell conflicts")
    );
    assert_eq!(std::fs::read(retained).unwrap(), large.as_bytes());
}
