use flate2::{write::GzEncoder, Compression};
use lens::{LensDataPack, LensDataPackSpec};
use sha2::{Digest, Sha256};
use std::io::Read;

const XML: &str = r#"<lensdatabase version="1"><lens><maker>Test</maker><model>Prime</model><calibration><distortion model="poly3" focal="50" k1="0.01"/></calibration></lens></lensdatabase>"#;

fn archive(files: &[(&str, &[u8])]) -> Vec<u8> {
    let mut tar = tar::Builder::new(GzEncoder::new(Vec::new(), Compression::default()));
    for (name, bytes) in files {
        let mut header = tar::Header::new_gnu();
        header.set_size(bytes.len() as u64);
        header.set_mode(0o644);
        header.set_cksum();
        tar.append_data(&mut header, name, *bytes).unwrap();
    }
    tar.into_inner().unwrap().finish().unwrap()
}
fn spec(bytes: &[u8]) -> LensDataPackSpec {
    LensDataPackSpec {
        version: "synthetic-1".into(),
        download_url: "https://example.invalid/pinned.tar.gz".into(),
        sha256: format!("{:x}", Sha256::digest(bytes)),
        archive_root: "pack".into(),
        attribution: "Synthetic test data".into(),
        license: "CC-BY-SA-3.0".into(),
    }
}
#[test]
fn resolves_into_app_lens_cache_and_reuses_offline() {
    let bytes = archive(&[("pack/data/db/test.xml", XML.as_bytes())]);
    let app = tempfile::tempdir().unwrap();
    let pack = LensDataPack::with_spec(app.path(), spec(&bytes)).unwrap();
    assert!(
        !app.path().join("lens").exists(),
        "construction must not download or create files"
    );
    let loaded = pack
        .resolve_with(|url| {
            assert_eq!(url, "https://example.invalid/pinned.tar.gz");
            Ok(bytes.as_slice())
        })
        .unwrap();
    assert_eq!(loaded.spec.version, "synthetic-1");
    assert_eq!(loaded.spec.attribution, "Synthetic test data");
    assert!(loaded.path.starts_with(app.path().join("lens")));
    assert!(loaded.database.find("Test", "Prime").is_some());
    let cached = pack
        .resolve_with(|_| -> lens::Result<&[u8]> { panic!("not used") })
        .unwrap();
    assert_eq!(cached.database.profiles.len(), 1);
}

#[test]
fn integrity_is_checked_before_publish_and_on_every_cache_hit() {
    let bytes = archive(&[("pack/data/db/test.xml", XML.as_bytes())]);
    let app = tempfile::tempdir().unwrap();
    let pack = LensDataPack::with_spec(app.path(), spec(&bytes)).unwrap();
    let wrong = archive(&[(
        "pack/data/db/test.xml",
        XML.replace("Prime", "Wrong").as_bytes(),
    )]);
    assert!(pack.resolve_with(|_| Ok(wrong.as_slice())).is_err());
    assert_eq!(
        std::fs::read_dir(app.path().join("lens")).unwrap().count(),
        0
    );
    let loaded = pack.resolve_with(|_| Ok(bytes.as_slice())).unwrap();
    std::fs::write(&loaded.path, wrong).unwrap();
    assert!(pack
        .resolve_with(|_| -> lens::Result<&[u8]> { panic!("corrupt cache must fail closed") })
        .is_err());
}

#[test]
fn rejects_invalid_manifest_before_io() {
    let bytes = archive(&[]);
    let app = tempfile::tempdir().unwrap();
    for (field, value) in [
        ("sha256", "../escape"),
        ("sha256", &"A".repeat(64)),
        ("version", ""),
        ("attribution", ""),
        ("license", ""),
        ("archive_root", "../pack"),
        ("download_url", "http://example.com/pack"),
    ] {
        let mut json = serde_json::to_value(spec(&bytes)).unwrap();
        json[field] = value.into();
        assert!(
            LensDataPack::with_spec(app.path(), serde_json::from_value(json).unwrap()).is_err(),
            "{field}"
        );
    }
    assert!(!app.path().join("lens").exists());
}

fn assert_rejected(bytes: Vec<u8>) {
    let app = tempfile::tempdir().unwrap();
    let pack = LensDataPack::with_spec(app.path(), spec(&bytes)).unwrap();
    assert!(pack.resolve_with(|_| Ok(bytes.as_slice())).is_err());
    assert_eq!(
        std::fs::read_dir(app.path().join("lens")).unwrap().count(),
        0,
        "invalid packs must not be published"
    );
}
#[test]
fn validates_archive_before_atomic_publish() {
    assert_rejected(archive(&[(
        "pack/data/db/broken.xml",
        b"<lensdatabase><lens>",
    )]));
    assert_rejected(archive(&[]));
    assert_rejected(archive(&[("pack/not-db.xml", XML.as_bytes())]));
    assert_rejected(archive(&[
        ("pack/data/db/a.xml", XML.as_bytes()),
        ("pack/data/db/a.xml", XML.as_bytes()),
    ]));
}
#[test]
fn rejects_unsafe_archive_paths_and_links() {
    for (name, kind) in [
        ("../escape.xml", b'0'),
        ("/tmp/escape.xml", b'0'),
        ("pack\\escape.xml", b'0'),
        ("pack/data/db/link.xml", b'2'),
        ("pack/hardlink", b'1'),
    ] {
        let mut builder = tar::Builder::new(GzEncoder::new(Vec::new(), Compression::default()));
        let mut h = tar::Header::new_gnu();
        h.as_mut_bytes()[..name.len()].copy_from_slice(name.as_bytes());
        h.set_entry_type(tar::EntryType::new(kind));
        h.set_size(0);
        h.set_mode(0o644);
        h.set_cksum();
        builder.append(&h, std::io::empty()).unwrap();
        assert_rejected(builder.into_inner().unwrap().finish().unwrap());
    }
}
#[test]
fn bounds_compressed_and_expanded_input() {
    // Small gzip, very large uncompressed member; no XML allocation permitted.
    assert_rejected(archive(&[(
        "pack/data/db/huge.xml",
        &vec![b' '; 2 * 1024 * 1024 + 1],
    )]));
    let app = tempfile::tempdir().unwrap();
    let pack = LensDataPack::with_spec(app.path(), spec(&[])).unwrap();
    let error = pack
        .resolve_with(|_| Ok(std::io::repeat(0).take(16 * 1024 * 1024 + 1)))
        .unwrap_err();
    assert!(error.to_string().contains("compressed"), "{error}");
    assert_eq!(
        std::fs::read_dir(app.path().join("lens")).unwrap().count(),
        0
    );
}

#[test]
fn unsupported_lenses_are_reported_without_losing_supported_neighbors() {
    let unsupported = "<lens><maker>Test</maker><model>Future</model><calibration><tca model='poly3' focal='50'/></calibration></lens>";
    let fisheye = "<lens><maker>Test</maker><model>Fish</model><type>fisheye</type><calibration><distortion model='poly3' focal='50'/></calibration></lens>";
    let xml = XML.replace(
        "</lensdatabase>",
        &format!("{unsupported}{fisheye}</lensdatabase>"),
    );
    let bytes = archive(&[
        ("pack/data/db/test.xml", xml.as_bytes()),
        (
            "pack/data/db/mounts.xml",
            b"<lensdatabase><mount><name>Test</name></mount></lensdatabase>",
        ),
        ("pack/docs/example.xml", b"not XML"),
    ]);
    let app = tempfile::tempdir().unwrap();
    let loaded = LensDataPack::with_spec(app.path(), spec(&bytes))
        .unwrap()
        .resolve_with(|_| Ok(bytes.as_slice()))
        .unwrap();
    assert_eq!(loaded.database.profiles.len(), 1);
    assert_eq!(loaded.skipped.len(), 2);
    assert!(loaded.skipped.iter().all(|s| s.contains("test.xml")));
}
#[test]
fn bounds_xml_depth_and_calibration_expansion() {
    let deep = format!(
        "<lensdatabase>{}{}{}</lensdatabase>",
        "<node>".repeat(40),
        XML,
        "</node>".repeat(40)
    );
    assert_rejected(archive(&[("pack/data/db/deep.xml", deep.as_bytes())]));
    let samples = (1..=100)
        .map(|i| format!("<vignetting model='pa' focal='{i}' aperture='{i}' distance='10'/>"))
        .collect::<String>();
    let xml = format!("<lensdatabase><lens><model>Explosive</model><calibration>{samples}</calibration></lens></lensdatabase>");
    assert_rejected(archive(&[("pack/data/db/grid.xml", xml.as_bytes())]));
    assert_rejected(archive(&[(
        "pack/data/db/dtd.xml",
        b"<!DOCTYPE lensdatabase><lensdatabase/>",
    )]));
}

#[test]
fn builtin_manifest_pins_verified_upstream_and_attribution() {
    let spec = LensDataPackSpec::lensfun();
    assert_eq!(
        spec.version,
        "0.3.4+101c745e847a5de4a1e569a94368ce2027198598"
    );
    assert_eq!(spec.download_url, "https://codeload.github.com/lensfun/lensfun/tar.gz/101c745e847a5de4a1e569a94368ce2027198598");
    assert_eq!(
        spec.sha256,
        "a11cbe6aeec657839540448b253217c25d20b7a45b6aebfef406f7239933c7a6"
    );
    assert_eq!(spec.license, "CC-BY-SA-3.0");
    assert!(spec.attribution.contains("Lensfun contributors"));
    let app = tempfile::tempdir().unwrap();
    assert!(LensDataPack::new(app.path()).is_ok());
    assert!(!app.path().join("lens").exists());
}
#[test]
#[ignore = "explicit upstream network smoke test; ordinary tests are synthetic and offline"]
fn downloads_pinned_upstream_pack() {
    let app = tempfile::tempdir().unwrap();
    let pack = LensDataPack::new(app.path()).unwrap();
    let loaded = pack.resolve().unwrap();
    assert!(loaded.database.profiles.len() > 100);
    assert!(loaded.path.is_file());
    assert_eq!(
        pack.resolve_with(|_| -> lens::Result<&[u8]> { panic!("cache miss") })
            .unwrap()
            .database
            .profiles
            .len(),
        loaded.database.profiles.len()
    );
    println!(
        "Loaded {} profiles; {} skipped; archive {}",
        loaded.database.profiles.len(),
        loaded.skipped.len(),
        loaded.path.display()
    );
}

#[test]
fn accepts_git_archive_global_comment_header() {
    let bytes = archive(&[("pack/data/db/test.xml", XML.as_bytes())]);
    let mut tar_bytes = Vec::new();
    flate2::read::GzDecoder::new(bytes.as_slice())
        .read_to_end(&mut tar_bytes)
        .unwrap();
    let mut h = tar::Header::new_ustar();
    h.set_path("pax_global_header").unwrap();
    h.set_entry_type(tar::EntryType::new(b'g'));
    h.set_size(18);
    h.set_mode(0o644);
    h.set_cksum();
    let mut raw = h.as_bytes().to_vec();
    raw.extend_from_slice(b"18 comment=pinned\n");
    raw.resize(1024, 0);
    raw.extend(tar_bytes);
    let mut gzip = GzEncoder::new(Vec::new(), Compression::default());
    std::io::Write::write_all(&mut gzip, &raw).unwrap();
    let bytes = gzip.finish().unwrap();
    let app = tempfile::tempdir().unwrap();
    let loaded = LensDataPack::with_spec(app.path(), spec(&bytes))
        .unwrap()
        .resolve_with(|_| Ok(bytes.as_slice()))
        .unwrap();
    assert_eq!(loaded.database.profiles.len(), 1);
}
