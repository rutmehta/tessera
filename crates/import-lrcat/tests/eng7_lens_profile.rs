//! ENG-7: Lightroom `LensProfileEnable` maps exactly, and an enabled profile
//! Tessera cannot supply is reported (info level), never estimated.
//! Synthetic rows only; no catalog is opened.
use engine_api::recipe::{Recipe, settings::LensProfileSource};
use import_lrcat::{diagnostics, lua_develop, xmp};

const FIELD: &str = "/settings/lens/profile";

fn lens_notes(r: &Recipe) -> Vec<diagnostics::Entry> {
    diagnostics::entries(r)
        .into_iter()
        .filter(|(key, _)| key.starts_with("LensProfile"))
        .flat_map(|(_, notes)| notes)
        .filter(|n| n.field.as_deref() == Some(FIELD))
        .collect()
}

fn assert_unavailable_note(r: &Recipe, warnings: &[String], what: &str) {
    let notes = lens_notes(r);
    assert!(
        notes.iter().any(|n| n.level == "info"
            && n.status == "approximate"
            && n.reason.contains("unavailable")
            && n.reason.contains("never")),
        "{what}: missing info note: {notes:?}"
    );
    // An info note, not a user-facing warning.
    assert!(
        !warnings.iter().any(|w| w.contains("LensProfileEnable")),
        "{what}: {warnings:?}"
    );
}

fn lua(row: &str) -> (Recipe, Vec<String>) {
    lua_develop::parse(row, "15.4").unwrap()
}

fn packet(attrs: &str) -> (Recipe, Vec<String>) {
    xmp::parse(
        &format!(
            r#"<rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#"><rdf:Description xmlns:crs="http://ns.adobe.com/camera-raw-settings/1.0/" crs:ProcessVersion="15.4" {attrs}/></rdf:RDF>"#
        ),
        "15.4",
    )
    .unwrap()
}

#[test]
fn profile_disabled_maps_to_no_profile_correction_without_note() {
    for (r, _) in [
        lua("s={LensProfileEnable=0}"),
        lua("s={LensProfileEnable=0,LensProfileSetup='Custom',LensProfileName='Adobe (Synthetic Lens)'}"),
        packet(r#"crs:LensProfileEnable="0""#),
        packet(
            r#"crs:LensProfileEnable="0" crs:LensProfileSetup="Custom" crs:LensProfileName="Adobe (Synthetic Lens)""#,
        ),
    ] {
        assert_eq!(r.settings.lens.profile, LensProfileSource::None);
        assert!(lens_notes(&r).is_empty(), "{:?}", lens_notes(&r));
    }
}

#[test]
fn profile_enabled_without_identity_is_auto_and_notes_unavailable_database() {
    for (what, (r, w)) in [
        ("lua", lua("s={LensProfileEnable=1,LensProfileSetup='LensDefaults'}")),
        (
            "xmp",
            packet(r#"crs:LensProfileEnable="1" crs:LensProfileSetup="Auto""#),
        ),
    ] {
        // Auto: the raw's embedded correction if present, else nothing.
        assert_eq!(r.settings.lens.profile, LensProfileSource::Auto, "{what}");
        assert_unavailable_note(&r, &w, what);
    }
}

#[test]
fn named_profile_is_kept_and_noted_unavailable() {
    for (what, (r, w)) in [
        (
            "lua",
            lua("s={LensProfileEnable=1,LensProfileSetup='Custom',LensProfileName='Adobe (Synthetic Lens)',LensProfileFilename='synthetic.lcp',LensProfileDigest='0123'}"),
        ),
        (
            "xmp",
            packet(
                r#"crs:LensProfileEnable="1" crs:LensProfileSetup="Custom" crs:LensProfileName="Adobe (Synthetic Lens)" crs:LensProfileFilename="synthetic.lcp" crs:LensProfileDigest="0123""#,
            ),
        ),
    ] {
        let LensProfileSource::Database { profile } = &r.settings.lens.profile else {
            panic!("{what}: {:?}", r.settings.lens.profile);
        };
        assert_eq!(profile.name.as_str(), "Adobe (Synthetic Lens)", "{what}");
        assert_unavailable_note(&r, &w, what);
    }
}

#[test]
fn absent_profile_keys_keep_the_default_without_note() {
    let (r, _) = lua("s={Exposure2012=0.5}");
    assert_eq!(r.settings.lens.profile, LensProfileSource::Auto);
    assert!(lens_notes(&r).is_empty());
}
