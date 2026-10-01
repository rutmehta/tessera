//! LrC 15.5 stores `Adobe_imageDevelopSettings.text` as a Lua table literal
//! (`s = { ... }`) instead of XMP (B5-29b). The rows under `tests/data/lrc155`
//! follow the real layout (tab indentation, trailing commas, Adobe key names)
//! with invented values; nothing is copied from a real catalog.
use import_lrcat::{fixture, import, lua_develop};
use rusqlite::Connection;
use serde_json::json;

const GLOBAL: &str = include_str!("data/lrc155/global.lua");
const STRUCTURES: &str = include_str!("data/lrc155/structures.lua");
const LEGACY: &str = include_str!("data/lrc155/legacy.lua");

fn xmp(attrs: &str, body: &str) -> String {
    format!(
        r#"<x:xmpmeta xmlns:x="adobe:ns:meta/"><rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#"><rdf:Description rdf:about="" xmlns:crs="http://ns.adobe.com/camera-raw-settings/1.0/" {attrs}>{body}</rdf:Description></rdf:RDF></x:xmpmeta>"#
    )
}

/// The recipe minus the retained source text, which differs by format.
fn comparable(mut r: engine_api::recipe::Recipe) -> engine_api::recipe::Recipe {
    r.unknown.remove("sidecar_xmp");
    // Exact Lua literals and exact XMP fragments intentionally differ. The
    // golden and per-key retention tests hash/assert these without stripping.
    r.unknown.remove("lrcat_develop_source");
    r
}

#[test]
fn the_same_edit_as_xmp_and_as_lua_yields_an_identical_recipe() {
    let curve = |pts: &[&str]| {
        format!(
            "<rdf:Seq>{}</rdf:Seq>",
            pts.iter()
                .map(|p| format!("<rdf:li>{p}</rdf:li>"))
                .collect::<String>()
        )
    };
    let identity = curve(&["0, 0", "255, 255"]);
    let as_xmp = xmp(
        r#"crs:AutoLateralCA="1" crs:Blacks2012="-7" crs:CameraProfile="Adobe Standard" crs:CameraProfileDigest="0123456789ABCDEF0123456789ABCDEF" crs:ColorGradeBlending="50" crs:Contrast2012="12" crs:ConvertToGrayscale="False" crs:Exposure2012="0.35" crs:HDREditMode="0" crs:Highlights2012="-40" crs:LensProfileEnable="1" crs:LensProfileSetup="LensDefaults" crs:OverrideLookVignette="False" crs:ProcessVersion="15.4" crs:Saturation="5" crs:Shadows2012="25" crs:SharpenDetail="25" crs:SharpenRadius="1" crs:Sharpness="40" crs:Temperature="5150" crs:Tint="8" crs:ToneCurveName2012="Linear" crs:Version="15.5" crs:Vibrance="10" crs:WhiteBalance="Custom" crs:Whites2012="6""#,
        &format!(
            "<crs:RedEyeInfo><rdf:Seq/></crs:RedEyeInfo><crs:ToneCurvePV2012>{}</crs:ToneCurvePV2012><crs:ToneCurvePV2012Blue>{identity}</crs:ToneCurvePV2012Blue><crs:ToneCurvePV2012Green>{identity}</crs:ToneCurvePV2012Green><crs:ToneCurvePV2012Red>{identity}</crs:ToneCurvePV2012Red>",
            curve(&["0, 0", "64, 58", "192, 200", "255, 255"])
        ),
    );
    let (from_xmp, xmp_warnings) = import_lrcat::xmp::parse(&as_xmp, "15.4").unwrap();
    let (from_lua, lua_warnings) = lua_develop::parse(GLOBAL, "15.4").unwrap();
    assert_eq!(from_lua.settings.tone.exposure, 0.35);
    assert_eq!(from_lua.settings.tone.curves.rgb.0.len(), 4);
    assert_eq!(comparable(from_lua), comparable(from_xmp));
    assert_eq!(lua_warnings, xmp_warnings);
}

#[test]
fn structured_values_map_like_their_xmp_form() {
    // Look (lang-alt group, nested parameters with a curve) and a gradient mask.
    let as_xmp = xmp(
        r#"crs:Exposure2012="-0.5" crs:ProcessVersion="15.4""#,
        r#"<crs:Look><rdf:Description crs:Amount="0.6" crs:Name="Synthetic Look" crs:SupportsAmount="True" crs:UUID="00000000000000000000000000000001"><crs:Group><rdf:Alt><rdf:li xml:lang="x-default">Synthetic Group</rdf:li></rdf:Alt></crs:Group><crs:Parameters><rdf:Description crs:ProcessVersion="15.4" crs:Version="15.5"><crs:ToneCurvePV2012><rdf:Seq><rdf:li>0, 0</rdf:li><rdf:li>255, 255</rdf:li></rdf:Seq></crs:ToneCurvePV2012></rdf:Description></crs:Parameters></rdf:Description></crs:Look><crs:MaskGroupBasedCorrections><rdf:Seq><rdf:li><rdf:Description crs:CorrectionActive="True" crs:CorrectionAmount="1" crs:CorrectionName="Synthetic Gradient" crs:LocalExposure2012="0.5" crs:What="Correction"><crs:CorrectionMasks><rdf:Seq><rdf:li><rdf:Description crs:FullX="0.1" crs:FullY="0.2" crs:MaskActive="True" crs:MaskInverted="False" crs:MaskValue="1" crs:What="Mask/Gradient" crs:ZeroX="0.8" crs:ZeroY="0.9"/></rdf:li></rdf:Seq></crs:CorrectionMasks></rdf:Description></rdf:li></rdf:Seq></crs:MaskGroupBasedCorrections>"#,
    );
    let lua = r#"s = { Exposure2012 = -0.5,
	Look = { Amount = 0.6,
		Group = { ["x-default"] = "Synthetic Group" },
		Name = "Synthetic Look",
		Parameters = { ProcessVersion = "15.4",
			ToneCurvePV2012 = { 0,
				0,
				255,
				255 },
			Version = "15.5" },
		SupportsAmount = true,
		UUID = "00000000000000000000000000000001" },
	MaskGroupBasedCorrections = { { CorrectionActive = true,
			CorrectionAmount = 1,
			CorrectionMasks = { { FullX = 0.1,
					FullY = 0.2,
					MaskActive = true,
					MaskInverted = false,
					MaskValue = 1,
					What = "Mask/Gradient",
					ZeroX = 0.8,
					ZeroY = 0.9 } },
			CorrectionName = "Synthetic Gradient",
			LocalExposure2012 = 0.5,
			What = "Correction" } },
	ProcessVersion = "15.4" }
"#;
    let (from_xmp, xmp_warnings) = import_lrcat::xmp::parse(&as_xmp, "15.4").unwrap();
    let (from_lua, lua_warnings) = lua_develop::parse(lua, "15.4").unwrap();
    let look = from_lua.settings.camera_profile.look.as_ref().unwrap();
    assert_eq!(look.style, "Synthetic Look".into());
    assert_eq!(from_lua.settings.locals.adjustments.len(), 1);
    assert_eq!(from_lua.settings.locals.adjustments[0].params.exposure, 0.5);
    assert_eq!(comparable(from_lua), comparable(from_xmp));
    assert_eq!(lua_warnings, xmp_warnings);
}

#[test]
fn unknown_keys_are_reported_and_the_source_is_retained() {
    let (recipe, warnings) = lua_develop::parse(STRUCTURES, "15.4").unwrap();
    assert_eq!(recipe.settings.tone.exposure, -0.5);
    assert_eq!(
        recipe.settings.camera_profile.profile.name,
        "Synthetic \"Quoted\" Profile".into()
    );
    for key in ["SyntheticFutureKey", "Preset"] {
        assert!(
            warnings
                .iter()
                .any(|w| w.contains(key) && w.contains("unknown Lua develop key")),
            "{key}: {warnings:?}"
        );
    }
    assert_eq!(
        recipe.unknown["lrcat_develop_lua"]["SyntheticFutureKey"],
        json!("3")
    );
    recipe.validate().unwrap();
}

#[test]
fn mapping_table_is_one_to_one_and_covers_every_crs_key() {
    use std::collections::BTreeSet;
    let lua: BTreeSet<_> = lua_develop::KEY_MAP.iter().map(|(l, _)| *l).collect();
    let crs: BTreeSet<_> = lua_develop::KEY_MAP.iter().map(|(_, c)| *c).collect();
    assert_eq!(lua.len(), lua_develop::KEY_MAP.len(), "duplicate Lua key");
    assert_eq!(crs.len(), lua_develop::KEY_MAP.len(), "duplicate crs name");
    for key in engine_api::recipe::CrsKey::ALL {
        assert!(crs.contains(key.xmp_name()), "{key} is not mapped");
    }
}

#[test]
fn lua_escapes_decode() {
    let (recipe, _) = lua_develop::parse(
        r#"s = { CameraProfile = "A\tB\\C\"D\'E\65\x46\u{47}\z
                  H\
I" }"#,
        "15.4",
    )
    .unwrap();
    assert_eq!(
        recipe.settings.camera_profile.profile.name,
        "A\tB\\C\"D'EAFGH\nI".into()
    );
}

#[test]
fn malformed_literals_are_decode_errors_naming_the_image() {
    for bad in [
        "s = { Exposure2012 = f() }",
        "s = { Exposure2012 = other }",
        "s = { Exposure2012 = 1 + 2 }",
        "s = { Exposure2012 = 1 } t = 2",
        "s = { Exposure2012 = 1 }; s = {}",
        "t = { Exposure2012 = 1 }",
        "s = { Exposure2012 = \"unterminated }",
        "s = { Exposure2012 = \"bad \\q escape\" }",
        "s = { Exposure2012 = 1, Exposure2012 = 2 }",
        "s = { Exposure2012 = 0x10 }",
        "s = { Exposure2012 = 1e999 }",
        "s = { Exposure2012 = 1 -- comment\n }",
        "s = { Exposure2012 = [[long]] }",
        "s = { Exposure2012 = \"\\255\" }",
        "s = { Exposure2012 = 1",
        "s = ",
        "return { }",
        "garbage",
    ] {
        let err = import_lrcat::develop(42, bad, "15.4")
            .unwrap_err()
            .to_string();
        assert!(err.contains("image 42"), "{bad:?}: {err}");
    }
}

#[test]
fn format_is_detected_by_the_first_token() {
    let (lua, _) = import_lrcat::develop(1, "\n  s = { Exposure2012 = 1 }", "15.4").unwrap();
    assert_eq!(lua.settings.tone.exposure, 1.0);
    let (x, _) = import_lrcat::develop(1, &xmp(r#"crs:Exposure2012="1""#, ""), "15.4").unwrap();
    assert_eq!(x.settings.tone.exposure, 1.0);
}

#[test]
fn deep_nesting_is_rejected_without_overflowing_the_stack() {
    for depth in [lua_develop::MAX_DEPTH + 1, 1_000_000] {
        let text = format!("s = {}{}", "{".repeat(depth), "}".repeat(depth));
        let err = lua_develop::read(&text).unwrap_err().to_string();
        assert!(err.contains("nesting"), "{err}");
    }
    let ok = format!(
        "s = {}{}",
        "{".repeat(lua_develop::MAX_DEPTH),
        "}".repeat(lua_develop::MAX_DEPTH)
    );
    lua_develop::read(&ok).unwrap();
}

#[test]
fn oversized_input_is_rejected_before_parsing() {
    let long = format!(
        "s = {{ CameraProfile = \"{}\" }}",
        "x".repeat(lua_develop::MAX_INPUT_BYTES)
    );
    let err = lua_develop::read(&long).unwrap_err().to_string();
    assert!(err.contains("limit"), "{err}");
    // A long but in-limit string is fine.
    let fits = format!("s = {{ CameraProfile = \"{}\" }}", "x".repeat(1 << 20));
    lua_develop::read(&fits).unwrap();
    // So is a wide array; the value count is bounded too.
    let wide = format!("s = {{ {} }}", "0,".repeat(lua_develop::MAX_VALUES + 1));
    let err = lua_develop::read(&wide).unwrap_err().to_string();
    assert!(err.contains("limit"), "{err}");
}

#[test]
fn catalog_with_lua_develop_rows_imports() {
    let dir = tempfile::tempdir().unwrap();
    let f = fixture::write(dir.path()).unwrap();
    let c = Connection::open(&f.catalog).unwrap();
    let ids: Vec<i64> = c
        .prepare("SELECT image FROM Adobe_imageDevelopSettings ORDER BY image LIMIT 3")
        .unwrap()
        .query_map([], |r| r.get(0))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    assert_eq!(ids.len(), 3);
    for (id, (text, pv)) in
        ids.iter()
            .zip([(GLOBAL, "15.4"), (STRUCTURES, "15.4"), (LEGACY, "11.0")])
    {
        c.execute(
            "UPDATE Adobe_imageDevelopSettings SET text=?1, processVersion=?2 WHERE image=?3",
            rusqlite::params![text, pv, id],
        )
        .unwrap();
    }
    drop(c);
    let plan = import(&f.catalog).unwrap();
    let recipe = |id: i64| {
        &plan
            .images
            .iter()
            .find(|i| i.catalog_id == id)
            .unwrap()
            .recipe
    };
    assert_eq!(recipe(ids[0]).settings.tone.exposure, 0.35);
    assert_eq!(recipe(ids[1]).settings.tone.exposure, -0.5);
    assert_eq!(recipe(ids[2]).settings.tone.exposure, 1.25);
    assert_eq!(
        recipe(ids[2]).process_version,
        engine_api::recipe::ProcessVersion::adobe(5)
    );
    assert!(
        plan.report
            .iter()
            .any(|r| r.starts_with(&format!("image {}: ", ids[1]))
                && r.contains("SyntheticFutureKey")),
        "{:?}",
        plan.report
    );
}
