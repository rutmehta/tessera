//! Opt-in aggregate audit. Never emit source rows, string values or identifiers.
use import_lrcat::lua_develop::{LuaKey, LuaValue};
use std::collections::{BTreeMap, BTreeSet};
fn walk(v: &LuaValue, prefix: &str, out: &mut BTreeSet<String>) {
    if let LuaValue::Table(t) = v {
        for item in &t.items { walk(item, prefix, out); }
        for (key, value) in &t.fields {
            if let LuaKey::Str(key) = key {
                // Property names only; refuse arbitrary punctuation/long names.
                if key.len() > 64 || !key.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_') { continue; }
                let path = format!("{prefix}/{key}");
                out.insert(path.clone());
                if key == "What" || key == "SpotType" || key == "spotType" {
                    if let LuaValue::String(s) = value {
                        let kind = match s.as_str() {
                            "Mask/Paint" => "brush", "Mask/Gradient" => "gradient", "Mask/CircularGradient" => "radial", "Mask/Image" => "AI", "Mask/Range" => "range", "Mask/Group" => "group", "heal" => "heal", "clone" => "clone", "generative" => "generative", _ => "other",
                        };
                        out.insert(format!("{path}/kind_{kind}"));
                    }
                }
                walk(value, &path, out);
            } else { walk(value, prefix, out); }
        }
    }
}
#[test]
#[ignore]
fn aggregate_only() {
    std::panic::set_hook(Box::new(|_| {}));
    let result = std::panic::catch_unwind(run);
    assert!(matches!(result, Ok(Ok(()))), "aggregate audit failed; details suppressed");
}
fn run() -> Result<(), ()> {
    let path = std::env::var("TESSERA_LRCAT_PROFILE").map_err(|_| ())?;
    if !path.starts_with("/private/tmp/claude-501/") || !path.ends_with("/scratchpad/lrimport/cat.lrcat") { return Err(()); }
    let db = rusqlite::Connection::open_with_flags(path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY).map_err(|_| ())?;
    let mut q = db.prepare("SELECT processVersion, CAST(text AS TEXT) FROM Adobe_imageDevelopSettings WHERE rowid IN (SELECT max(rowid) FROM Adobe_imageDevelopSettings GROUP BY image) AND image IN (SELECT id_local FROM Adobe_images)").map_err(|_| ())?;
    let mut rows = q.query([]).map_err(|_| ())?;
    let mut counts = BTreeMap::<String,u64>::new();
    while let Some(row) = rows.next().map_err(|_| ())? {
        let pv: Option<String> = row.get(0).map_err(|_| ())?;
        let Some(pv) = pv else { continue; };
        let source: Option<String> = row.get(1).map_err(|_| ())?;
        let Some(source) = source else { continue; };
        if source.trim().is_empty() { continue; }
        let Ok((r,w)) = import_lrcat::develop(0, &source, &pv) else { *counts.entry("decode_failure".into()).or_default() += 1; continue; };
        let family = if r.process_version.revision <= 2 { "legacy" } else { "modern" };
        let mut keys = BTreeSet::new();
        for warning in &w {
            if let Some((k,_)) = warning.trim_start_matches("crs:").split_once(':') {
                if k.len() <= 64 && k.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_') { keys.insert(k.to_string()); }
            }
        }
        for key in &keys { *counts.entry(format!("warnings/{key}/{family}")).or_default() += 1; }
        *counts.entry("warning_occurrences".into()).or_default() += w.len() as u64;
        let Ok(LuaValue::Table(t)) = import_lrcat::lua_develop::read(&source) else { continue; };
        let mut fields = BTreeSet::new();
        for (k,v) in &t.fields {
            if let LuaKey::Str(k) = k {
                if keys.contains(k) && matches!(k.as_str(), "MaskGroupBasedCorrections" | "RetouchAreas" | "RetouchInfo" | "RemoveAreas") { walk(v,k,&mut fields); }
            }
        }
        for f in fields { *counts.entry(format!("fields/{f}")).or_default() += 1; }
    }
    println!("{}", serde_json::to_string_pretty(&counts).map_err(|_| ())?);
    Ok(())
}
