//! Opt-in aggregate audit. Never emit source rows, string values or identifiers.
use import_lrcat::lua_develop::{LuaKey, LuaValue};
use std::collections::{BTreeMap, BTreeSet};
fn walk(v: &LuaValue, prefix: &str, out: &mut BTreeSet<String>) {
    if let LuaValue::Table(t) = v {
        if prefix.ends_with("/Dabs") {
            for item in &t.items {
                if let LuaValue::String(s) = item {
                    let words: Vec<_> = s.split_whitespace().collect();
                    let kind = match words.first().copied() {
                        Some("d") => "stamp",
                        Some("r") => "radius",
                        Some("f") => "flow",
                        Some("h") => "hardness",
                        _ => "unknown_command",
                    };
                    out.insert(format!("{prefix}/command/{kind}"));
                    if words.first() == Some(&"d")
                        && words
                            .iter()
                            .skip(1)
                            .filter_map(|w| w.parse::<f64>().ok())
                            .any(|v| !(0. ..=1.).contains(&v))
                    {
                        out.insert(format!("{prefix}/outside_image"));
                    }
                }
            }
        }
        let get = |name: &str| {
            t.fields
                .iter()
                .find_map(|(k, v)| matches!(k,LuaKey::Str(k) if k == name).then_some(v))
        };
        let scalar = |v: &LuaValue| match v {
            LuaValue::Number(s) | LuaValue::String(s) => Some(s.clone()),
            _ => None,
        };
        let number = |table: &import_lrcat::lua_develop::LuaTable, key: &str| {
            table.fields.iter().find_map(|(k, v)| {
                if matches!(k,LuaKey::Str(k) if k.eq_ignore_ascii_case(key)) {
                    scalar(v).and_then(|v| v.parse::<f64>().ok())
                } else {
                    None
                }
            })
        };
        if let Some(LuaValue::Table(masks)) = get("Masks") {
            for mask in &masks.items {
                if let LuaValue::Table(mask) = mask {
                    for (flat, nested) in [
                        ("centerX", "X"),
                        ("centerY", "Y"),
                        ("radius", "SizeX"),
                        ("radius", "SizeY"),
                        ("sourceY", "OffsetY"),
                    ] {
                        if let (Some(a), Some(b)) = (number(t, flat), number(mask, nested)) {
                            let class = if (a - b).abs() < 1e-6 {
                                "equal"
                            } else if (2. * a - b).abs() < 1e-6 {
                                "double"
                            } else if (a - 2. * b).abs() < 1e-6 {
                                "half"
                            } else {
                                "different"
                            };
                            out.insert(format!("{prefix}/geometry_alias/{flat}_{nested}/{class}"));
                        }
                    }
                }
            }
        }
        if let Some(what) = get("What").and_then(scalar) {
            let class = match what.as_str() {
                "Mask/Image" => match get("MaskSubType").and_then(scalar).as_deref() {
                    Some("1") => {
                        if matches!(get("MaskInverted"), Some(LuaValue::Bool(true))) {
                            "AI_background"
                        } else {
                            "AI_subject"
                        }
                    }
                    Some("2") => "AI_sky",
                    Some("3") => {
                        if get("MaskSubCategoryID")
                            .and_then(scalar)
                            .is_some_and(|s| s != "0")
                        {
                            "AI_person_part"
                        } else {
                            "AI_people"
                        }
                    }
                    Some("0") => "AI_object",
                    _ => "AI_unknown_subtype",
                },
                "Mask/Range" | "Mask/RangeMask" => "range",
                "Mask/Paint" => "brush",
                "Mask/CircularGradient" => "radial",
                "Mask/Gradient" => "gradient",
                "Mask/Aggregate" | "Mask/Group" => "nested_group",
                _ => "other",
            };
            out.insert(format!("{prefix}/selection/{class}"));
            if get("InstanceIDs").is_some() {
                out.insert(format!("{prefix}/instance_parent/{class}"));
            }
        }
        if prefix.ends_with("/CorrectionRangeMask") {
            let class = match get("Type").and_then(scalar).as_deref() {
                Some("1") => "colour_range",
                Some("2") => "luminance_range",
                Some("3") => "depth_range",
                _ => "unknown_range",
            };
            out.insert(format!("{prefix}/selection/{class}"));
        }
        if !t.items.is_empty() && t.fields.is_empty() {
            let numbers: Option<Vec<_>> = t
                .items
                .iter()
                .map(|v| scalar(v).and_then(|s| s.parse::<f64>().ok()))
                .collect();
            let label = if numbers
                .as_ref()
                .is_some_and(|ns| ns.iter().all(|n| *n == 0.))
            {
                "all_zero"
            } else if numbers.is_some() {
                "numeric_sequence"
            } else if t.items.iter().all(|v| {
                scalar(v).is_some_and(|s| s.split(',').all(|n| n.trim().parse::<f64>() == Ok(-1.)))
            }) {
                "all_point_placeholders"
            } else {
                "other_sequence"
            };
            out.insert(format!("{prefix}/sequence/{label}"));
        }
        for item in &t.items {
            walk(item, prefix, out);
        }
        let mut folded = BTreeMap::new();
        for (key, value) in &t.fields {
            if let LuaKey::Str(key) = key {
                let key = key.to_ascii_lowercase();
                if let Some(previous) = folded.insert(key.clone(), value)
                    && key.len() <= 64
                    && key.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
                {
                    let class = if previous == value {
                        "equal"
                    } else {
                        "different"
                    };
                    out.insert(format!("{prefix}/duplicate_casefold/{key}/{class}"));
                }
            }
        }
        for (key, value) in &t.fields {
            if let LuaKey::Str(key) = key {
                // Property names only; refuse arbitrary punctuation/long names.
                if key.len() > 64 || !key.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_') {
                    continue;
                }
                let path = format!("{prefix}/{key}");
                out.insert(path.clone());
                let class = match value {
                    LuaValue::Number(n) => match n.parse::<f64>().ok() {
                        Some(0.) => "zero",
                        Some(1.) => "one",
                        Some(100.) => "hundred",
                        Some(_) => "other_number",
                        None => "invalid_number",
                    },
                    LuaValue::Bool(false) => "false",
                    LuaValue::Bool(true) => "true",
                    LuaValue::Table(t) if t.items.is_empty() && t.fields.is_empty() => "empty",
                    LuaValue::Table(_) => "structure",
                    LuaValue::String(_) => "string",
                    _ => "nil",
                };
                out.insert(format!("{path}/class_{class}"));
                if (key == "What"
                    || key == "SpotType"
                    || key == "spotType"
                    || key == "Method"
                    || key == "SourceState")
                    && let LuaValue::String(s) = value
                {
                    let kind = match s.as_str() {
                        "Mask/Paint" => "brush",
                        "Mask/Gradient" => "gradient",
                        "Mask/CircularGradient" => "radial",
                        "Mask/Image" => "AI",
                        "Mask/Range" => "range",
                        "Mask/Group" | "Mask/Aggregate" => "group",
                        "Mask/RangeMask" => "range",
                        "Mask/Ellipse" => "ellipse",
                        "heal" => "heal",
                        "clone" => "clone",
                        "generative" | "generativeRemove" => "generative",
                        "contentAware" | "contentAwareRemove" | "content-aware" => "content_aware",
                        "gaussian" => "gaussian",
                        "remove" => "remove",
                        "healV2" | "healv2" => "heal_v2",
                        "sourceAutoComputed" => "auto_source",
                        "sourceSetExplicitly" => "explicit_source",
                        _ => "other",
                    };
                    out.insert(format!("{path}/kind_{kind}"));
                }
                walk(value, &path, out);
            } else {
                walk(value, prefix, out);
            }
        }
    }
}
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
    if !path.starts_with("/private/tmp/claude-501/")
        || !path.ends_with("/scratchpad/lrimport/cat.lrcat")
    {
        return Err(());
    }
    let db =
        rusqlite::Connection::open_with_flags(path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
            .map_err(|_| ())?;
    let query = "SELECT processVersion, CAST(text AS TEXT) FROM Adobe_imageDevelopSettings WHERE rowid IN (SELECT max(rowid) FROM Adobe_imageDevelopSettings GROUP BY image) AND image IN (SELECT id_local FROM Adobe_images)";
    let query = if std::env::var_os("TESSERA_LR9B_RETOUCH_ONLY").is_some() {
        format!("{query} AND instr(text, 'RetouchAreas') > 0")
    } else {
        query.into()
    };
    let mut q = db.prepare(&query).map_err(|_| ())?;
    let mut rows = q.query([]).map_err(|_| ())?;
    let mut counts = BTreeMap::<String, u64>::new();
    while let Some(row) = rows.next().map_err(|_| ())? {
        let pv: Option<String> = row.get(0).map_err(|_| ())?;
        let Some(pv) = pv else {
            continue;
        };
        let source: Option<String> = row.get(1).map_err(|_| ())?;
        let Some(source) = source else {
            continue;
        };
        if source.trim().is_empty() {
            continue;
        }
        let Ok((r, w)) = import_lrcat::develop(0, &source, &pv) else {
            *counts.entry("decode_failure".into()).or_default() += 1;
            continue;
        };
        let family = if r.process_version.revision <= 2 {
            "legacy"
        } else {
            "modern"
        };
        *counts
            .entry(format!("process_family/{family}"))
            .or_default() += 1;
        let mut keys = BTreeSet::new();
        for warning in &w {
            if let Some((k, _)) = warning.trim_start_matches("crs:").split_once(':')
                && k.len() <= 64
                && k.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
            {
                keys.insert(k.to_string());
                if k == "MaskGroupBasedCorrections" {
                    let labels: Vec<_> = [
                        ("local tone curve", "local_tone_curve"),
                        ("local point-color", "local_point_color"),
                        ("local color-variance", "local_color_variance"),
                        ("individual AI person-instance", "AI_person_instance"),
                        ("local defringe", "local_defringe"),
                        ("local color overlay", "local_color_overlay"),
                        ("radial mask inversion", "radial_inversion_conflict"),
                        ("AI object selection", "AI_object_geometry"),
                        ("unrecognized AI selection", "AI_selection_subtype"),
                        ("brush stamp", "brush_stamp_encoding"),
                        ("range-mask selection", "range_selection_encoding"),
                        ("unrecognized mask selection kind", "unknown_selection_kind"),
                        ("mask geometry", "geometry_or_structure"),
                    ]
                    .into_iter()
                    .filter_map(|(text, label)| warning.contains(text).then_some(label))
                    .collect();
                    let label = if labels.is_empty() {
                        "unclassified".to_string()
                    } else {
                        labels.join("+")
                    };
                    *counts
                        .entry(format!("mask_reason_classes/{label}"))
                        .or_default() += 1;
                }
                let reason = [
                    "local tone curve",
                    "local point-color",
                    "local color-variance",
                    "individual AI person-instance",
                    "local defringe",
                    "local color overlay",
                    "mask geometry",
                    "number outside CRS range",
                    "invalid boolean",
                    "unknown choice",
                    "unsupported property",
                    "unknown Lua develop key",
                    "duplicate property superseded",
                ]
                .into_iter()
                .find(|r| warning.contains(r))
                .unwrap_or("structured_or_other");
                *counts.entry(format!("reasons/{k}/{reason}")).or_default() += 1;
            }
        }
        for (key, entries) in import_lrcat::diagnostics::entries(&r) {
            if [
                "MaskGroupBasedCorrections",
                "RetouchAreas",
                "RetouchInfo",
                "RemoveAreas",
                "GenerativeRemove",
            ]
            .contains(&key.as_str())
            {
                let class = if entries.iter().any(|e| e.status == "approximate") {
                    "approximate"
                } else {
                    "ignored"
                };
                *counts.entry(format!("outcomes/{key}/{class}")).or_default() += 1;
            }
        }
        for key in &keys {
            *counts
                .entry(format!("warnings/{key}/{family}"))
                .or_default() += 1;
        }
        *counts.entry("warning_occurrences".into()).or_default() += w.len() as u64;
        let Ok(LuaValue::Table(t)) = import_lrcat::lua_develop::read(&source) else {
            continue;
        };
        *counts
            .entry("generic_warning_occurrences".into())
            .or_default() += w
            .iter()
            .filter(|w| {
                [
                    "unsupported property",
                    "unknown Lua develop key",
                    "mask source retained",
                ]
                .iter()
                .any(|s| w.contains(s))
            })
            .count() as u64;
        let mut fields = BTreeSet::new();
        for (k, v) in &t.fields {
            if let LuaKey::Str(k) = k {
                if keys.contains(k) {
                    let class = match v {
                        LuaValue::Number(_) => "number",
                        LuaValue::String(_) => "string",
                        LuaValue::Bool(_) => "boolean",
                        LuaValue::Table(_) => "structure",
                        _ => "nil",
                    };
                    *counts
                        .entry(format!("source_types/{k}/{class}"))
                        .or_default() += 1;
                }
                if matches!(
                    k.as_str(),
                    "MaskGroupBasedCorrections" | "RetouchAreas" | "RetouchInfo" | "RemoveAreas"
                ) {
                    walk(v, &format!("all/{k}"), &mut fields);
                    if keys.contains(k) {
                        walk(v, k, &mut fields);
                    }
                }
            }
        }
        if keys.contains("RetouchAreas") && !fields.contains("RetouchAreas/pm_patch") {
            for (key, value) in &t.fields {
                if matches!(key,LuaKey::Str(key) if key == "RetouchAreas") {
                    walk(value, "non_patch_failure", &mut fields);
                }
            }
        }
        for f in fields {
            *counts.entry(format!("fields/{f}")).or_default() += 1;
        }
    }
    println!("{}", serde_json::to_string_pretty(&counts).map_err(|_| ())?);
    Ok(())
}
