use export::{ExportImage, ExportSettings, Format, Metadata, export_one};
use pipeline_cpu::{Image, RenderSource};
use sidecar::XmpPacket;

fn workspace_temp() -> tempfile::TempDir {
    tempfile::tempdir_in(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../tools/orchestrate/wp/M2-45d"
    ))
    .unwrap()
}
fn packet(body: &str) -> String {
    format!(
        r#"<rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#"><rdf:Description rdf:about="">{body}</rdf:Description></rdf:RDF>"#
    )
}
fn segment(jpeg: &mut Vec<u8>, marker: u8, payload: &[u8]) {
    jpeg.extend([255, marker]);
    jpeg.extend(u16::try_from(payload.len() + 2).unwrap().to_be_bytes());
    jpeg.extend(payload);
}
fn export_source(
    source: &[u8],
    sidecar: Option<&XmpPacket>,
    policy: Metadata,
) -> Result<Vec<u8>, String> {
    let dir = workspace_temp();
    let input = dir.path().join("source.bin");
    std::fs::write(&input, source).unwrap();
    let pixels = Image::new(2, 2, vec![vec![0.2; 4]; 3]).unwrap();
    let image = ExportImage {
        source: RenderSource::Rgb(&pixels),
        name: "result",
        sequence: 1,
        date: "",
        metadata: sidecar,
    };
    export_one(
        &image,
        &Default::default(),
        &ExportSettings {
            format: Format::Tiff { bits: 16 },
            metadata: policy,
            remove_person_info: true,
            keywords_as_hierarchy: true,
            output_dir: dir.path().into(),
            metadata_sources: [(1, input)].into(),
            ..Default::default()
        },
    )
    .map(|p| std::fs::read(p).unwrap())
    .map_err(|e| e.to_string())
}
#[test]
fn partial_sidecar_preserves_embedded_properties_in_developed_exports() {
    let embedded = packet(
        r#"<dc:rights xmlns:dc="http://purl.org/dc/elements/1.1/"><rdf:Alt><rdf:li xml:lang="x-default">Embedded Rights</rdf:li><rdf:li xml:lang="fr">Droits</rdf:li></rdf:Alt></dc:rights><dc:description xmlns:dc="http://purl.org/dc/elements/1.1/">Embedded Caption</dc:description><lr:hierarchicalSubject xmlns:lr="http://ns.adobe.com/lightroom/1.0/"><rdf:Bag><rdf:li>Places|Nature</rdf:li></rdf:Bag></lr:hierarchicalSubject><c:CreatorContactInfo xmlns:c="http://iptc.org/std/Iptc4xmpCore/1.0/xmlns/" rdf:parseType="Resource"><c:CiEmailWork>contact@example.test</c:CiEmailWork></c:CreatorContactInfo>"#,
    );
    let mut source = vec![255, 216];
    segment(
        &mut source,
        0xe1,
        &[
            b"http://ns.adobe.com/xap/1.0/\0".as_slice(),
            embedded.as_bytes(),
        ]
        .concat(),
    );
    source.extend([255, 217, 0, 0]);
    let sidecar = XmpPacket::parse(packet(
        r#"<x:Rating xmlns:x="http://ns.adobe.com/xap/1.0/">4</x:Rating>"#,
    ))
    .unwrap();
    for policy in [Metadata::All, Metadata::CopyrightOnly] {
        let bytes = export_source(&source, Some(&sidecar), policy).unwrap();
        let text = String::from_utf8_lossy(&bytes);
        assert!(
            text.contains("Embedded Rights"),
            "embedded rights lost: {policy:?}"
        );
        assert!(text.contains("Droits"));
        for value in ["Embedded Caption", "Places|Nature", "contact@example.test"] {
            assert_eq!(
                text.contains(value),
                matches!(policy, Metadata::All),
                "{value}: {policy:?}"
            );
        }
    }
}
fn ifd(bytes: &mut Vec<u8>, id: u16, value: u32) {
    bytes.extend(1u16.to_le_bytes());
    bytes.extend(id.to_le_bytes());
    bytes.extend(4u16.to_le_bytes());
    bytes.extend(1u32.to_le_bytes());
    bytes.extend(value.to_le_bytes());
    bytes.extend(0u32.to_le_bytes());
}
#[test]
fn misplaced_ifd_pointers_return_error_without_panicking() {
    for parent in [34665, 34853] {
        for misplaced in [34665, 34853, 40965] {
            if parent == 34665 && misplaced == 40965 {
                continue;
            }
            let mut tiff = b"II\x2a\0\x08\0\0\0".to_vec();
            ifd(&mut tiff, parent, 26);
            ifd(&mut tiff, misplaced, 0x00100000);
            let result = std::panic::catch_unwind(|| export_source(&tiff, None, Metadata::All));
            assert!(
                result.is_ok(),
                "malformed pointer must not panic: {parent}/{misplaced}"
            );
            let error = result
                .unwrap()
                .expect_err("malformed pointer must be rejected");
            assert!(error.contains("directory pointer"), "{error}");
        }
    }
}
#[test]
fn extended_xmp_person_metadata_is_rejected_not_silently_ignored() {
    let mut jpeg = vec![255, 216];
    let base = packet(
        r#"<n:HasExtendedXMP xmlns:n="http://ns.adobe.com/xmp/note/">0123456789ABCDEF0123456789ABCDEF</n:HasExtendedXMP>"#,
    );
    segment(
        &mut jpeg,
        0xe1,
        &[
            b"http://ns.adobe.com/xap/1.0/\0".as_slice(),
            base.as_bytes(),
        ]
        .concat(),
    );
    let extended = packet(
        r#"<p:PersonInImage xmlns:p="http://iptc.org/std/Iptc4xmpExt/2008-02-29/"><rdf:Bag><rdf:li>Alice</rdf:li></rdf:Bag></p:PersonInImage>"#,
    );
    let mut extension = b"http://ns.adobe.com/xmp/extension/\0".to_vec();
    extension.extend(b"0123456789ABCDEF0123456789ABCDEF");
    extension.extend((extended.len() as u32).to_be_bytes());
    extension.extend(0u32.to_be_bytes());
    extension.extend(extended.as_bytes());
    segment(&mut jpeg, 0xe1, &extension);
    let iim = b"\x1c\x02\x19\0\x05Alice\x1c\x02\x19\0\x06Nature";
    let mut photoshop = b"Photoshop 3.0\08BIM\x04\x04\0\0".to_vec();
    photoshop.extend((iim.len() as u32).to_be_bytes());
    photoshop.extend(iim);
    if !iim.len().is_multiple_of(2) {
        photoshop.push(0);
    }
    segment(&mut jpeg, 0xed, &photoshop);
    jpeg.extend([255, 217, 0, 0]);
    let error = export_source(&jpeg, None, Metadata::All)
        .expect_err("unsupported Extended XMP must fail closed");
    assert!(error.contains("Extended XMP"), "{error}");
}

#[test]
fn jpeg_metadata_segment_limit_fails_closed() {
    let mut jpeg = vec![255, 216];
    let iim = b"\x1c\x02\x19\0\x05Alice";
    let mut photoshop = b"Photoshop 3.0\08BIM\x04\x04\0\0".to_vec();
    photoshop.extend((iim.len() as u32).to_be_bytes());
    photoshop.extend(iim);
    segment(&mut jpeg, 0xed, &photoshop);
    for _ in 0..4095 {
        segment(&mut jpeg, 0xe0, &[]);
    }
    segment(&mut jpeg, 0xe1, b"http://ns.adobe.com/xmp/extension/\0");
    jpeg.extend([255, 217, 0, 0]);
    let error = export_source(&jpeg, None, Metadata::All)
        .expect_err("segment limit must not silently truncate privacy metadata");
    assert!(error.contains("segment limit"), "{error}");
}
