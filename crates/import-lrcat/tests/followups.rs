//! B5-29c follow-ups on the LrC 15.5 Lua develop path: ordinary Upright keys,
//! bad develop rows degrade instead of aborting, report entries grouped per
//! message, unconditional pending-source retention, identity-aware extended
//! curves, and explicit bounded omission of oversized cells.
use import_lrcat::{ImportPlan, fixture, import, lua_develop};
use rusqlite::Connection;
use serde_json::json;

const GLOBAL: &str = include_str!("data/lrc155/global.lua");
const STRUCTURES: &str = include_str!("data/lrc155/structures.lua");

/// Write the fixture, replace the first `rows.len()` develop rows (by image id)
/// with the given (text, processVersion) pairs, and import it.
fn import_with(rows: &[(Option<&str>, Option<&str>)]) -> (Vec<i64>, ImportPlan) {
    let dir = tempfile::tempdir().unwrap();
    let f = fixture::write(dir.path()).unwrap();
    let c = Connection::open(&f.catalog).unwrap();
    let ids: Vec<i64> = c
        .prepare("SELECT image FROM Adobe_imageDevelopSettings ORDER BY image LIMIT ?1")
        .unwrap()
        .query_map([rows.len() as i64], |r| r.get(0))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    assert_eq!(ids.len(), rows.len());
    for (id, (text, pv)) in ids.iter().zip(rows) {
        c.execute(
            "UPDATE Adobe_imageDevelopSettings SET text=?1, processVersion=?2 WHERE image=?3",
            rusqlite::params![text, pv, id],
        )
        .unwrap();
    }
    drop(c);
    let plan = import(&f.catalog).unwrap();
    (ids, plan)
}

fn recipe(plan: &ImportPlan, id: i64) -> &engine_api::recipe::Recipe {
    &plan
        .images
        .iter()
        .find(|i| i.catalog_id == id)
        .unwrap()
        .recipe
}

// 1. UprightFourSegmentsCount / UprightTransformCount are ordinary crs
// properties: no "unknown Lua develop key" entry; raw source is retained separately.
#[test]
fn upright_counts_are_ordinary_crs_properties() {
    for key in ["UprightFourSegmentsCount", "UprightTransformCount"] {
        assert!(
            lua_develop::KEY_MAP
                .iter()
                .any(|(l, c)| *l == key && *c == key),
            "{key} missing from KEY_MAP"
        );
    }
    let (recipe, warnings) = lua_develop::parse(
        "s = { Exposure2012 = 1, UprightFourSegmentsCount = 0, UprightTransformCount = 6 }",
        "15.4",
    )
    .unwrap();
    assert_eq!(recipe.settings.tone.exposure, 1.0);
    assert!(
        !warnings
            .iter()
            .any(|w| w.contains("unknown Lua develop key")),
        "{warnings:?}"
    );
    assert!(!recipe.unknown.contains_key("lrcat_develop_lua"));
}

// 2. A bad develop row, or a non-empty row with NULL processVersion, imports
// that image as unedited with a report entry and its source retained.
#[test]
fn bad_develop_rows_degrade_to_unedited_with_source_retained() {
    let bad_lua = "s = { Exposure2012 = f() }";
    let (ids, plan) = import_with(&[
        (Some(bad_lua), Some("15.4")),
        (Some(GLOBAL), None),
        (Some("garbage"), Some("15.4")),
        (Some(GLOBAL), Some("15.4")),
    ]);
    for (id, text) in ids.iter().zip([bad_lua, GLOBAL, "garbage"]) {
        let r = recipe(&plan, *id);
        assert_eq!(r.settings, engine_api::recipe::Recipe::default().settings);
        assert_eq!(r.unknown["lrcat_develop_source"]["text"], json!(text));
        assert!(
            plan.report
                .iter()
                .any(|e| e.starts_with(&format!("image {id}: "))
                    && e.contains("imported as unedited")),
            "{id}: {:#?}",
            plan.report
        );
    }
    assert!(
        plan.report.iter().any(|e| e.contains("no process version")),
        "{:#?}",
        plan.report
    );
    // The good row next to them still imports.
    assert_eq!(recipe(&plan, ids[3]).settings.tone.exposure, 0.35);
}

// 3. Per-image warnings are grouped: one entry per message with the image
// count and the first image id; a single occurrence keeps the old form.
#[test]
fn report_entries_are_grouped_per_message() {
    let (ids, plan) = import_with(&[
        (Some(STRUCTURES), Some("15.4")),
        (Some(STRUCTURES), Some("15.4")),
        (Some(STRUCTURES), Some("15.4")),
        (
            Some("s = { Exposure2012 = 1, OnlyOnceKey = 2 }"),
            Some("15.4"),
        ),
    ]);
    let future: Vec<_> = plan
        .report
        .iter()
        .filter(|e| e.contains("SyntheticFutureKey"))
        .collect();
    assert_eq!(future.len(), 1, "{:#?}", plan.report);
    assert!(
        future[0].starts_with(&format!("3 images (first: image {}): ", ids[0])),
        "{}",
        future[0]
    );
    let once: Vec<_> = plan
        .report
        .iter()
        .filter(|e| e.contains("OnlyOnceKey"))
        .collect();
    assert_eq!(once.len(), 1);
    assert!(
        once[0].starts_with(&format!("image {}: ", ids[3])),
        "{}",
        once[0]
    );
    // No message appears twice.
    let unique: std::collections::BTreeSet<_> = plan.report.iter().collect();
    assert_eq!(unique.len(), plan.report.len(), "{:#?}", plan.report);
}

// 4. Lua rows keep neither the whole literal nor a synthesized sidecar_xmp:
// unknown-key lookup coexists with the unconditional pending-source map.
#[test]
fn lua_rows_retain_only_unknown_key_source() {
    let (recipe, _) = lua_develop::parse(STRUCTURES, "15.4").unwrap();
    assert!(!recipe.unknown.contains_key("sidecar_xmp"));
    let kept = recipe.unknown["lrcat_develop_lua"].as_object().unwrap();
    assert_eq!(kept["SyntheticFutureKey"], json!("3"));
    assert!(
        kept["Preset"]
            .as_str()
            .unwrap()
            .contains("Synthetic Preset"),
        "{kept:?}"
    );
    assert!(!kept.contains_key("Exposure2012"));
    assert!(
        !kept
            .values()
            .any(|v| v.as_str().unwrap().contains("Exposure2012 = -0.5")),
        "{kept:?}"
    );
    // Mapped-only rows need no unknown-key lookup; pending mapped source still stays.
    let (known, warnings) = lua_develop::parse(GLOBAL, "15.4").unwrap();
    assert!(
        !known.unknown.contains_key("lrcat_develop_lua"),
        "{warnings:?}"
    );
    assert!(!known.unknown.contains_key("sidecar_xmp"));
    // XMP rows still keep their own source packet.
    let (x, _) = import_lrcat::develop(1, "<x:xmpmeta xmlns:x=\"adobe:ns:meta/\"><rdf:RDF xmlns:rdf=\"http://www.w3.org/1999/02/22-rdf-syntax-ns#\"><rdf:Description xmlns:crs=\"http://ns.adobe.com/camera-raw-settings/1.0/\" crs:Exposure2012=\"1\"/></rdf:RDF></x:xmpmeta>", "15.4").unwrap();
    assert!(x.unknown.contains_key("sidecar_xmp"));
}

// 6. Out-of-domain extended curves remain a named, lossless limitation.
#[test]
fn out_of_domain_extended_curves_are_one_named_limitation() {
    let edited = "s = { Exposure2012 = 1,
	ExtendedToneCurveName2012 = \"Custom\",
	ExtendedToneCurvePV2012 = { 0, 0, 128, 150, 300, 350 },
	ExtendedToneCurvePV2012Blue = { 0, 0, 255, 255 },
	ExtendedToneCurvePV2012Green = { 0, 0, 255, 255 },
	ExtendedToneCurvePV2012Red = { 0, 0, 255, 255 } }";
    let identity = "s = { Exposure2012 = 1,
	ExtendedToneCurvePV2012 = { 0, 0, 255, 255 },
	ExtendedToneCurvePV2012Blue = { 0, 0, 255, 255 },
	ExtendedToneCurvePV2012Green = { 0, 0, 255, 255 },
	ExtendedToneCurvePV2012Red = { 0, 0, 255, 255 } }";
    for (text, keys) in [(edited, 2), (identity, 0)] {
        let (recipe, warnings) = lua_develop::parse(text, "15.4").unwrap();
        assert_eq!(recipe.settings.tone.exposure, 1.0);
        let ext: Vec<_> = warnings
            .iter()
            .filter(|w| w.contains("ExtendedToneCurve"))
            .collect();
        assert_eq!(ext.len(), usize::from(text == edited), "{warnings:?}");
        if text == identity {
            continue;
        }
        assert!(ext[0].contains("not supported"), "{}", ext[0]);
        assert!(!ext[0].contains("unknown Lua develop key"), "{}", ext[0]);
        let kept = recipe.unknown["lrcat_develop_lua"].as_object().unwrap();
        assert_eq!(kept.len(), keys, "{kept:?}");
        assert!(kept.keys().all(|k| k.starts_with("ExtendedToneCurve")));
        assert!(recipe.settings.tone.curves.rgb.0.is_empty());
    }
    // Several images: one grouped report entry.
    let (ids, plan) = import_with(&[
        (Some(edited), Some("15.4")),
        (Some(edited), Some("15.4")),
        (Some(identity), Some("15.4")),
    ]);
    let entries: Vec<_> = plan
        .report
        .iter()
        .filter(|e| e.contains("ExtendedToneCurve"))
        .collect();
    assert_eq!(entries.len(), 1, "{:#?}", plan.report);
    assert!(
        entries[0].starts_with(&format!("2 images (first: image {}): ", ids[0])),
        "{}",
        entries[0]
    );
}

// 5. Cells larger than the per-cell bound are not loaded: the row degrades
// with a report entry instead of being held in memory.
#[test]
fn oversized_cells_are_not_loaded() {
    let huge = format!(
        "s = {{ CameraProfile = \"{}\" }}",
        "x".repeat(import_lrcat::MAX_CELL_BYTES)
    );
    let dir = tempfile::tempdir().unwrap();
    let f = fixture::write(dir.path()).unwrap();
    let c = Connection::open(&f.catalog).unwrap();
    c.execute(
        "UPDATE Adobe_imageDevelopSettings SET rowid=9001, text=?1 WHERE image=30",
        [&huge],
    )
    .unwrap();
    let ids = [30];
    let plan = import(&f.catalog).unwrap();
    let r = recipe(&plan, ids[0]);
    let kept = r.unknown["lrcat_develop_source"]["text"].as_str().unwrap();
    assert!(!kept.is_empty() && kept.len() <= 64 * 1024);
    assert!(huge.starts_with(kept));
    assert_eq!(
        r.unknown["lrcat_develop_source"]["shape"],
        "cell-descriptor"
    );
    assert_eq!(r.unknown["lrcat_develop_source"]["truncated"], true);
    assert_eq!(
        r.unknown["lrcat_develop_source"]["cell"]["status"],
        "omitted"
    );
    assert_eq!(
        r.unknown["lrcat_develop_source"]["cell"]["length"],
        huge.len()
    );
    assert_eq!(r.unknown["lrcat_develop_source"]["cell"]["rowid"], 9001);
    assert_eq!(r.settings, engine_api::recipe::Recipe::default().settings);
    assert!(
        plan.report
            .iter()
            .any(|e| e.starts_with(&format!("image {}: ", ids[0]))
                && e.contains("row 9001:")
                && e.contains("limit")
                && e.contains("imported as unedited")),
        "{:#?}",
        plan.report
    );
}

#[test]
fn every_unedited_image_is_individually_reported() {
    let (ids, plan) = import_with(&[
        (Some("garbage"), Some("15.4")),
        (Some("garbage"), Some("15.4")),
        (None, None),
        (None, None),
    ]);
    for id in ids {
        assert!(
            plan.report
                .iter()
                .any(|e| e.starts_with(&format!("image {id}: "))
                    && e.contains("imported as unedited")),
            "{:?}",
            plan.report
        );
    }
}

#[test]
fn develop_rows_are_ordered_and_orphans_and_null_ids_do_not_block() {
    let dir = tempfile::tempdir().unwrap();
    let f = fixture::write(dir.path()).unwrap();
    let c = Connection::open(&f.catalog).unwrap();
    c.execute_batch(
        "DELETE FROM Adobe_imageDevelopSettings;
        INSERT INTO Adobe_imageDevelopSettings VALUES(31, 's = { Exposure2012 = 2 }', '15.4');
        INSERT INTO Adobe_imageDevelopSettings VALUES(NULL, 'garbage', '15.4');
        INSERT INTO Adobe_imageDevelopSettings VALUES(-1, 'garbage', '15.4');
        INSERT INTO Adobe_imageDevelopSettings VALUES(999999, 'garbage', '15.4');
        INSERT INTO Adobe_imageDevelopSettings VALUES(30, 's = { Exposure2012 = 1 }', '15.4');
        INSERT INTO Adobe_imageDevelopSettings VALUES(30, 's = { Exposure2012 = 3 }', '15.4');",
    )
    .unwrap();
    let plan = import(&f.catalog).unwrap();
    assert_eq!(recipe(&plan, 30).settings.tone.exposure, 3.0);
    assert_eq!(recipe(&plan, 31).settings.tone.exposure, 2.0);
    assert!(
        plan.report
            .iter()
            .any(|e| e.contains("image 30") && e.contains("last-write-wins"))
    );
}

#[test]
fn identity_master_with_edited_channel_still_warns() {
    let (_, notes) = lua_develop::parse("s = { ExtendedToneCurveName2012 = 'Linear', ExtendedToneCurvePV2012 = {0,0,255,255}, ExtendedToneCurvePV2012Red = {0,0,128,150,300,350} }", "15.4").unwrap();
    assert_eq!(
        notes
            .iter()
            .filter(|n| n.contains("ExtendedToneCurve"))
            .count(),
        1
    );
    let (_, notes) = lua_develop::parse("s = { ExtendedToneCurveName2012 = 'Linear', ExtendedToneCurvePV2012 = {0,0,128,128,255,255} }", "15.4").unwrap();
    assert!(notes.iter().all(|n| !n.contains("ExtendedToneCurve")));
}

#[test]
fn pending_sources_are_exact_even_when_inactive_or_identity() {
    let mut cases = vec![
        (
            "MaskGroupBasedCorrections",
            "{ { CorrectionMasks = { { What = 'Mask/Image', Image = 'opaque' } } } }",
        ),
        (
            "LensBlur",
            "{ Active = false, BlurAmount = 30, FocalRange = '0 0 100 100' }",
        ),
        ("RetouchAreas", "{ 'opaque' }"),
        ("RetouchInfo", "{ 'opaque' }"),
        ("PointColors", "{ 'opaque' }"),
        ("ExtendedToneCurveName2012", "'Linear'"),
        ("UprightFuture", "{ Exact = 'yes' }"),
    ];
    cases.extend(
        lua_develop::KEY_MAP
            .iter()
            .filter(|(k, _)| {
                (k.starts_with("Upright") || engine_api::recipe::CrsKey::from_xmp_name(k).is_none())
                    && *k != "ConvertToGrayscale"
                    && !k.starts_with("GrayMixer")
            })
            .map(|(k, _)| (*k, "0")),
    );
    for (key, value) in cases {
        let (r, warnings) =
            lua_develop::parse(&format!("s = {{ {key} = {value} }}"), "15.4").unwrap();
        assert_eq!(
            r.unknown
                .get("lrcat_develop_source")
                .and_then(|s| s["properties"].get(key)),
            Some(&json!(value)),
            "{key}"
        );
        assert!(
            warnings
                .iter()
                .all(|w| !w.contains("retained in original XMP"))
        );
        let (nil, _) = lua_develop::parse(&format!("s = {{ {key} = nil }}"), "15.4").unwrap();
        assert_eq!(
            nil.unknown["lrcat_develop_source"]["properties"][key],
            "nil"
        );
        let fragment = if key == "LensBlur" {
            "<crs:LensBlur crs:Active='False' crs:BlurAmount='30' crs:FocalRange='0 0 100 100'/>"
                .to_string()
        } else {
            format!("<crs:{key}>opaque &amp; exact</crs:{key}>")
        };
        let xmp = format!(
            "<rdf:RDF xmlns:rdf='http://www.w3.org/1999/02/22-rdf-syntax-ns#'><rdf:Description xmlns:crs='http://ns.adobe.com/camera-raw-settings/1.0/'>{fragment}</rdf:Description></rdf:RDF>"
        );
        let (r, _) = import_lrcat::develop(1, &xmp, "15.4").unwrap();
        assert_eq!(
            r.unknown["lrcat_develop_source"]["properties"][key],
            fragment
        );
    }
}

#[test]
fn numeric_and_string_unknown_keys_cannot_collide() {
    let (r, _) = lua_develop::parse("s = { [1] = 'number', ['[1]'] = 'string', ['(positional entries)'] = 'named', 'positional' }", "15.4").unwrap();
    assert_eq!(r.unknown["lrcat_develop_lua"]["[1]"], "'string'");
    assert_eq!(
        r.unknown["lrcat_develop_lua"]["(positional entries)"],
        "'named'"
    );
    assert_eq!(
        r.unknown["lrcat_develop_lua_entries"][0]["key"]["number"],
        "1"
    );
    assert_eq!(
        r.unknown["lrcat_develop_lua_entries"][0]["value"],
        "'number'"
    );
    assert!(
        r.unknown["lrcat_develop_lua_positional"]
            .as_str()
            .unwrap()
            .contains("'positional'")
    );
}

#[test]
fn xmp_extended_identity_is_silent_and_edits_use_named_limitation() {
    for (points, count) in [
        (
            "<rdf:li>0, 0</rdf:li><rdf:li>128, 128</rdf:li><rdf:li>255, 255</rdf:li>",
            0,
        ),
        (
            "<rdf:li>0, 0</rdf:li><rdf:li>128, 150</rdf:li><rdf:li>300, 350</rdf:li>",
            1,
        ),
    ] {
        let fragment = format!(
            "<crs:ExtendedToneCurvePV2012><rdf:Seq>{points}</rdf:Seq></crs:ExtendedToneCurvePV2012>"
        );
        let packet = format!(
            "<rdf:RDF xmlns:rdf='http://www.w3.org/1999/02/22-rdf-syntax-ns#'><rdf:Description xmlns:crs='http://ns.adobe.com/camera-raw-settings/1.0/'>{fragment}</rdf:Description></rdf:RDF>"
        );
        let (r, notes) = import_lrcat::develop(1, &packet, "15.4").unwrap();
        if count > 0 {
            assert_eq!(
                r.unknown["lrcat_develop_source"]["properties"]["ExtendedToneCurvePV2012"],
                fragment
            );
        } else {
            assert!(!r.unknown.contains_key("lrcat_develop_source"));
        }
        assert_eq!(notes.len(), count, "{notes:?}");
        if count > 0 {
            assert_eq!(notes[0], lua_develop::EXTENDED_TONE_CURVE_NOTE);
        }
    }
}

#[test]
fn oversized_history_is_omitted_explicitly_and_null_stays_null() {
    let dir = tempfile::tempdir().unwrap();
    let f = fixture::write(dir.path()).unwrap();
    let c = Connection::open(&f.catalog).unwrap();
    let bytes = vec![0xff_u8; import_lrcat::MAX_CELL_BYTES + 1];
    c.execute(
        "UPDATE Adobe_libraryImageDevelopHistoryStep SET text=?1 WHERE image=30",
        [&bytes],
    )
    .unwrap();
    c.execute("INSERT INTO Adobe_libraryImageDevelopHistoryStep(id_local,image,text) VALUES(9001,30,NULL)", []).unwrap();
    let plan = import(&f.catalog).unwrap();
    let history = &recipe(&plan, 30).unknown["lrcat_history"];
    let cells: Vec<_> = history
        .as_array()
        .unwrap()
        .iter()
        .map(|r| &r["text"])
        .collect();
    let cell = cells.iter().find(|c| c.is_object()).unwrap();
    assert_eq!(cell["status"], "omitted");
    assert_eq!(cell["length"], bytes.len());
    assert_eq!(cell["prefix"].as_array().unwrap().len(), 64 * 1024);
    assert!(cells.iter().any(|c| c.is_null()));
    assert!(plan.report.iter().any(|r| r.contains("image 30:")
        && r.contains("omitted")
        && r.contains("recover from source catalog")));
}

#[test]
fn duplicate_reports_follow_image_id_order_not_sqlite_row_order() {
    let dir = tempfile::tempdir().unwrap();
    let f = fixture::write(dir.path()).unwrap();
    let c = Connection::open(&f.catalog).unwrap();
    c.execute_batch(
        "DELETE FROM Adobe_imageDevelopSettings;
        INSERT INTO Adobe_imageDevelopSettings VALUES(31,'garbage','15.4');
        INSERT INTO Adobe_imageDevelopSettings VALUES(31,'s = { Exposure2012 = 2 }','15.4');
        INSERT INTO Adobe_imageDevelopSettings VALUES(30,'garbage','15.4');",
    )
    .unwrap();
    let plan = import(&f.catalog).unwrap();
    let ids: Vec<i64> = plan
        .report
        .iter()
        .filter_map(|s| {
            s.strip_prefix("image ")
                .and_then(|s| s.split(':').next())
                .and_then(|s| s.parse().ok())
        })
        .collect();
    assert!(ids.windows(2).all(|ids| ids[0] <= ids[1]), "{ids:?}");
}

#[test]
fn xmp_source_fragments_preserve_attribute_spelling() {
    let fragment = "camera:UprightVersion = '1 &amp; 2'";
    let packet = format!(
        "<rdf:RDF xmlns:rdf='http://www.w3.org/1999/02/22-rdf-syntax-ns#'><rdf:Description xmlns:camera='http://ns.adobe.com/camera-raw-settings/1.0/' {fragment}/></rdf:RDF>"
    );
    let (recipe, _) = import_lrcat::develop(1, &packet, "15.4").unwrap();
    assert_eq!(
        recipe.unknown["lrcat_develop_source"]["properties"]["UprightVersion"],
        fragment
    );
}

#[test]
fn retained_source_shapes_and_unedited_reasons_are_distinct() {
    let (r, _) = lua_develop::parse("s = { shape = 'original', LensBlur = nil }", "15.4").unwrap();
    assert_eq!(r.unknown["lrcat_develop_source"]["shape"], "lua-values");
    assert_eq!(
        r.unknown["lrcat_develop_source"]["properties"]["shape"],
        "'original'"
    );
    assert_eq!(
        r.unknown["lrcat_develop_source"]["properties"]["LensBlur"],
        "nil"
    );
    let xmp = "<rdf:RDF xmlns:rdf='http://www.w3.org/1999/02/22-rdf-syntax-ns#'><rdf:Description xmlns:crs='http://ns.adobe.com/camera-raw-settings/1.0/' crs:Future='opaque'/></rdf:RDF>";
    let (r, _) = import_lrcat::develop(1, xmp, "15.4").unwrap();
    assert_eq!(r.unknown["lrcat_develop_source"]["shape"], "xmp-fragments");
    assert_eq!(
        r.unknown["lrcat_develop_source"]["properties"]["Future"],
        "crs:Future='opaque'"
    );
    let (ids, plan) = import_with(&[(None, None), (Some("garbage"), Some("15.4"))]);
    assert!(
        plan.report
            .iter()
            .any(|n| n.starts_with(&format!("image {}:", ids[0])) && n.contains("never developed"))
    );
    assert!(
        plan.report
            .iter()
            .any(|n| n.starts_with(&format!("image {}:", ids[1]))
                && n.contains("edits failed to import"))
    );
    assert_eq!(
        recipe(&plan, ids[1]).unknown["lrcat_develop_source"]["shape"],
        "raw-text"
    );
}
