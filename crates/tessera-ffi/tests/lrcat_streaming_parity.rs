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
    let mut options = import.default_options();
    options.library_folder = temp
        .path()
        .join("comparison")
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
