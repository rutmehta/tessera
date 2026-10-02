//! Opt-in, aggregate-only read-only catalog measurement. Never emit source data.
use std::collections::BTreeMap;
#[test]
#[ignore]
fn aggregate_only() {
    std::panic::set_hook(Box::new(|_| {}));
    let result = std::panic::catch_unwind(run);
    assert!(
        matches!(result, Ok(Ok(()))),
        "aggregate audit failed; details suppressed"
    );
}
fn run() -> Result<(), ()> {
    let path = std::env::var("TESSERA_LRCAT_PROFILE").map_err(|_| ())?;
    let path = std::path::Path::new(&path).canonicalize().map_err(|_| ())?;
    if !path.starts_with("/private/tmp") || path.extension().is_none_or(|s| s != "lrcat") {
        return Err(());
    }
    let db = rusqlite::Connection::open_with_flags(
        format!("file:{}?immutable=1&mode=ro", path.to_str().ok_or(())?),
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_URI,
    )
    .map_err(|_| ())?;
    let mut q = db.prepare("SELECT processVersion, CAST(text AS TEXT) FROM Adobe_imageDevelopSettings WHERE rowid IN (SELECT max(rowid) FROM Adobe_imageDevelopSettings GROUP BY image) AND image IN (SELECT id_local FROM Adobe_images)").map_err(|_| ())?;
    let mut rows = q.query([]).map_err(|_| ())?;
    let mut counts = BTreeMap::<String, u64>::new();
    while let Some(row) = rows.next().map_err(|_| ())? {
        let pv: Option<String> = row.get(0).map_err(|_| ())?;
        let source: Option<String> = row.get(1).map_err(|_| ())?;
        let (Some(pv), Some(source)) = (pv, source) else {
            continue;
        };
        if source.trim().is_empty() {
            continue;
        }
        let Ok((r, w)) = import_lrcat::develop(0, &source, &pv) else {
            *counts.entry("decode_failures".into()).or_default() += 1;
            continue;
        };
        *counts.entry("images_decoded".into()).or_default() += 1;
        *counts.entry("warning_occurrences".into()).or_default() += w.len() as u64;
        if import_lrcat::diagnostics::entries(&r)
            .values()
            .flatten()
            .any(|e| e.status == "cloud")
        {
            *counts.entry("cloud_images".into()).or_default() += 1;
        }
        for warning in w {
            let key = warning
                .trim_start_matches("crs:")
                .split_once(':')
                .map(|(k, _)| k)
                .unwrap_or("");
            // Only schema-defined keys can reach output. No catalog-derived identifiers.
            let safe = engine_api::recipe::crs::CrsKey::from_xmp(
                engine_api::recipe::crs::CRS_NAMESPACE,
                key,
            )
            .is_some();
            let safe = safe
                || import_lrcat::lua_develop::KEY_MAP
                    .iter()
                    .any(|(known, _)| *known == key)
                || include_str!("../../../docs/coordination/LR-TRANSLATION-MATRIX.md")
                    .lines()
                    .filter_map(|line| {
                        line.strip_prefix("| `")
                            .and_then(|rest| rest.split_once("` |"))
                    })
                    .any(|(known, _)| known == key);
            let key = if safe { key } else { "other" };
            *counts.entry(format!("warnings/{key}")).or_default() += 1;
        }
    }
    println!("{}", serde_json::to_string(&counts).map_err(|_| ())?);
    Ok(())
}
