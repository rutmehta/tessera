//! ExifTool is an independent fixture writer and carrier-aware reader. No test
//! parses the output with the production metadata implementation.
use export::{ExportImage, ExportSettings, Format, Metadata, export_one};
use pipeline_cpu::{Image, RenderSource};
use std::{path::Path, process::Command};

fn exiftool(args: &[&str], path: &Path) -> String {
    let out = Command::new("exiftool")
        .args(args)
        .arg(path)
        .output()
        .expect("native metadata interoperability tests require ExifTool");
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout).unwrap()
}
fn fixture(path: &Path) {
    image::RgbImage::from_pixel(16, 16, image::Rgb([80, 100, 120]))
        .save(path)
        .unwrap();
    exiftool(
        &[
            "-overwrite_original",
            "-EXIF:Copyright=Native Rights",
            "-EXIF:Artist=Native Author",
            "-EXIF:Make=Native Camera",
            "-EXIF:ExposureTime=1/125",
            "-EXIF:GPSLatitude=40.5",
            "-EXIF:GPSLatitudeRef=N",
            "-IPTC:CopyrightNotice=IIM Rights",
            "-IPTC:Contact=contact@example.test",
            "-IPTC:City=Private City",
            "-IPTC:Keywords=Alice",
            "-IPTC:Keywords+=Nature",
            "-XMP-iptcExt:PersonInImage=Alice",
            "-XMP-lr:HierarchicalSubject=People|Alice",
        ],
        path,
    );
}
fn formats() -> [Format; 6] {
    [
        Format::Jpeg { quality: 90 },
        Format::Tiff { bits: 16 },
        Format::Png,
        Format::Avif(export::AvifOptions {
            speed: 10,
            ..Default::default()
        }),
        Format::JpegXl { bits: 8 },
        Format::Dng,
    ]
}
fn read(all: &serde_json::Value, tag: &str) -> String {
    let key = tag.trim_start_matches('-').replace("XMP-lr:", "XMP:");
    all.get(&key)
        .filter(|v| !v.is_null())
        .map(|v| v.to_string())
        .unwrap_or_default()
}
#[test]
fn native_policies_and_independent_privacy_flags_in_every_carrier() {
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("source.jpg");
    fixture(&input);
    let original = std::fs::read(&input).unwrap();
    let pixels = Image::new(16, 16, vec![vec![0.2; 256]; 3]).unwrap();
    let image = ExportImage {
        source: RenderSource::Rgb(&pixels),
        name: "policy",
        sequence: 1,
        date: "",
        metadata: None,
    };
    for format in formats() {
        for policy in [
            Metadata::All,
            Metadata::CopyrightOnly,
            Metadata::CopyrightAndContact,
            Metadata::AllExceptCamera,
            Metadata::None,
        ] {
            for (person, location, hierarchy) in [
                (false, false, true),
                (true, false, true),
                (false, true, true),
                (true, true, false),
                (false, false, false),
                (true, true, true),
                (true, false, false),
                (false, true, false),
            ] {
                let out = tempfile::tempdir().unwrap();
                let path = export_one(
                    &image,
                    &Default::default(),
                    &ExportSettings {
                        format,
                        metadata: policy,
                        remove_person_info: person,
                        remove_location: location,
                        keywords_as_hierarchy: hierarchy,
                        output_dir: out.path().into(),
                        metadata_sources: [(1, input.clone())].into(),
                        ..Default::default()
                    },
                )
                .unwrap();
                let all: serde_json::Value = serde_json::from_str::<serde_json::Value>(&exiftool(
                    &["-j", "-G", "-s"],
                    &path,
                ))
                .unwrap()[0]
                    .clone();
                let general = matches!(policy, Metadata::All | Metadata::AllExceptCamera);
                let any = !matches!(policy, Metadata::None);
                assert_eq!(
                    read(&all, "-EXIF:Copyright").contains("Native Rights"),
                    any,
                    "{format:?} {policy:?}\n{all}"
                );
                assert_eq!(
                    read(&all, "-IPTC:CopyrightNotice").contains("IIM Rights"),
                    any,
                    "{format:?} {policy:?}\n{all}"
                );
                assert_eq!(
                    read(&all, "-IPTC:Contact").contains("contact@"),
                    any && !matches!(policy, Metadata::CopyrightOnly),
                    "{all}"
                );
                assert_eq!(
                    read(&all, "-EXIF:Make").contains("Native Camera"),
                    matches!(policy, Metadata::All),
                    "{all}"
                );
                assert_eq!(
                    !read(&all, "-EXIF:ExposureTime").is_empty(),
                    matches!(policy, Metadata::All),
                    "{all}"
                );
                assert_eq!(
                    !read(&all, "-EXIF:GPSLatitude").is_empty(),
                    matches!(policy, Metadata::All) && !location,
                    "{all}"
                );
                assert_eq!(
                    read(&all, "-IPTC:City").contains("Private City"),
                    general && !location,
                    "{all}"
                );
                let keywords = read(&all, "-IPTC:Keywords");
                assert_eq!(keywords.contains("Alice"), general && !person, "{all}");
                assert_eq!(keywords.contains("Nature"), general, "{all}");
                let hierarchy_out = read(&all, "-XMP-lr:HierarchicalSubject");
                assert_eq!(
                    hierarchy_out.contains("Alice"),
                    general && hierarchy && !person,
                    "{all}"
                );
                assert_eq!(
                    hierarchy_out.contains("Nature"),
                    general && hierarchy,
                    "{all}"
                );
            }
        }
    }
    assert_eq!(std::fs::read(input).unwrap(), original);
}

#[test]
fn explicit_sidecar_keywords_override_native_keywords_in_every_carrier() {
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("source.jpg");
    fixture(&input);
    let original = std::fs::read(&input).unwrap();
    let pixels = Image::new(16, 16, vec![vec![0.2; 256]; 3]).unwrap();
    // An absent property inherits source metadata; an empty property is an edit.
    // Reintroducing native keywords after the sidecar merge breaks these cases.
    for (body, expected_flat, expected_native, expected_paths) in [
        (
            "<xmp:Rating>4</xmp:Rating>",
            vec!["Alice", "Nature"],
            vec!["Alice", "Nature"],
            vec!["Nature", "People|Alice"],
        ),
        (
            "<dc:subject><rdf:Bag/></dc:subject><lr:hierarchicalSubject><rdf:Bag/></lr:hierarchicalSubject>",
            vec![],
            vec![],
            vec![],
        ),
        (
            "<dc:subject><rdf:Bag><rdf:li>Nature</rdf:li><rdf:li>New</rdf:li></rdf:Bag></dc:subject><lr:hierarchicalSubject><rdf:Bag><rdf:li>Places|Nature</rdf:li><rdf:li>New</rdf:li></rdf:Bag></lr:hierarchicalSubject>",
            vec!["Nature", "New"],
            vec!["Nature"],
            vec!["New", "Places|Nature"],
        ),
        // Overrides are per expanded property name: clearing only the flat
        // property must not erase an independently retained source hierarchy.
        (
            "<dc:subject/>",
            vec![],
            vec![],
            vec!["Nature", "People|Alice"],
        ),
        (
            "<lr:hierarchicalSubject/>",
            vec!["Alice", "Nature"],
            vec!["Alice", "Nature"],
            vec![],
        ),
    ] {
        let packet = sidecar::XmpPacket::parse(format!(
            r#"<rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#"><rdf:Description xmlns:dc="http://purl.org/dc/elements/1.1/" xmlns:lr="http://ns.adobe.com/lightroom/1.0/" xmlns:xmp="http://ns.adobe.com/xap/1.0/">{body}</rdf:Description></rdf:RDF>"#,
        ))
        .unwrap();
        let image = ExportImage {
            source: RenderSource::Rgb(&pixels),
            name: "keywords",
            sequence: 1,
            date: "",
            metadata: Some(&packet),
        };
        for format in formats() {
            let out = tempfile::tempdir().unwrap();
            let path = export_one(
                &image,
                &Default::default(),
                &ExportSettings {
                    format,
                    metadata: Metadata::All,
                    remove_person_info: false,
                    keywords_as_hierarchy: true,
                    output_dir: out.path().into(),
                    metadata_sources: [(1, input.clone())].into(),
                    ..Default::default()
                },
            )
            .unwrap();
            let all: serde_json::Value =
                serde_json::from_str(&exiftool(&["-j", "-G", "-s"], &path)).unwrap();
            for (tag, expected) in [
                ("XMP:Subject", &expected_flat),
                ("IPTC:Keywords", &expected_native),
                ("XMP:HierarchicalSubject", &expected_paths),
            ] {
                let mut actual = match &all[0][tag] {
                    serde_json::Value::Null => vec![],
                    // ExifTool represents an explicitly empty XMP property as
                    // an empty string, while an absent property is null.
                    serde_json::Value::String(v) if v.is_empty() => vec![],
                    serde_json::Value::String(v) => vec![v.as_str()],
                    serde_json::Value::Array(v) => v.iter().map(|v| v.as_str().unwrap()).collect(),
                    other => panic!("unexpected keywords: {other}"),
                };
                actual.sort_unstable();
                assert_eq!(&actual, expected, "{format:?} {tag}: {body}");
            }
            assert_eq!(all[0]["IPTC:CopyrightNotice"], "IIM Rights");
            assert_eq!(all[0]["IPTC:Contact"], "contact@example.test");
        }
    }
    assert_eq!(std::fs::read(input).unwrap(), original);
}

#[test]
fn metadata_sources_roundtrip_all_six_formats_and_big_endian_tiff() {
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("source.jpg");
    fixture(&input);
    let pixels = Image::new(16, 16, vec![vec![0.2; 256]; 3]).unwrap();
    let image = ExportImage {
        source: RenderSource::Rgb(&pixels),
        name: "source",
        sequence: 1,
        date: "",
        metadata: None,
    };
    for format in formats() {
        let out = tempfile::tempdir().unwrap();
        let source = export_one(
            &image,
            &Default::default(),
            &ExportSettings {
                format,
                output_dir: out.path().into(),
                metadata_sources: [(1, input.clone())].into(),
                ..Default::default()
            },
        )
        .unwrap();
        match format {
            Format::Jpeg { .. } | Format::Png | Format::Tiff { .. } => {
                let decoded = image::open(&source).unwrap();
                assert_eq!((decoded.width(), decoded.height()), (16, 16));
            }
            Format::Dng => {
                let decoded =
                    raw_decode::linear_dng::read(&mut std::fs::File::open(&source).unwrap())
                        .unwrap();
                assert_eq!((decoded.width, decoded.height), (16, 16));
            }
            Format::JpegXl { .. } => {
                let bytes = std::fs::read(&source).unwrap();
                let decoder = jxl_oxide::JxlImage::read_with_defaults(bytes.as_slice()).unwrap();
                assert_eq!((decoder.width(), decoder.height()), (16, 16));
                decoder.render_frame(0).unwrap();
            }
            Format::Avif(_) => {
                #[cfg(target_os = "macos")]
                {
                    let decoded =
                        color_mgmt::decode_to_tiff(&std::fs::read(&source).unwrap()).unwrap();
                    let mut decoder =
                        tiff::decoder::Decoder::new(std::io::Cursor::new(decoded)).unwrap();
                    assert_eq!(decoder.dimensions().unwrap(), (16, 16));
                    decoder.read_image().unwrap();
                }
            }
        }
        let target = tempfile::tempdir().unwrap();
        let path = export_one(
            &image,
            &Default::default(),
            &ExportSettings {
                format: Format::Jpeg { quality: 90 },
                output_dir: target.path().into(),
                metadata_sources: [(1, source)].into(),
                ..Default::default()
            },
        )
        .unwrap();
        let all = exiftool(&["-G", "-s"], &path);
        for value in [
            "Native Rights",
            "IIM Rights",
            "Native Camera",
            "Private City",
            "Alice",
            "Nature",
            "contact@example.test",
        ] {
            assert!(all.contains(value), "{format:?} missing {value}: {all}");
        }
    }
    let tiff = dir.path().join("big.tif");
    // Independently assembled big-endian EXIF directory and rational payload.
    let mut bytes = b"MM\0\x2a\0\0\0\x08".to_vec();
    bytes.extend(3u16.to_be_bytes());
    for (tag, kind, count, offset) in [
        (33432u16, 2u16, 11u32, 68u32),
        (33723, 7, 12, 88),
        (34665, 4, 1, 50),
    ] {
        bytes.extend(tag.to_be_bytes());
        bytes.extend(kind.to_be_bytes());
        bytes.extend(count.to_be_bytes());
        bytes.extend(offset.to_be_bytes());
    }
    bytes.extend([0; 4]);
    bytes.extend(1u16.to_be_bytes());
    bytes.extend(33434u16.to_be_bytes());
    bytes.extend(5u16.to_be_bytes());
    bytes.extend(1u32.to_be_bytes());
    bytes.extend(80u32.to_be_bytes());
    bytes.extend([0; 4]);
    bytes.extend(b"Big Endian\0\0");
    bytes.extend(1u32.to_be_bytes());
    bytes.extend(125u32.to_be_bytes());
    bytes.extend(b"\x1c\x02\x74\0\x07Big IIM");
    std::fs::write(&tiff, bytes).unwrap();
    let out = tempfile::tempdir().unwrap();
    let path = export_one(
        &image,
        &Default::default(),
        &ExportSettings {
            output_dir: out.path().into(),
            metadata_sources: [(1, tiff)].into(),
            ..Default::default()
        },
    )
    .unwrap();
    assert!(exiftool(&["-s3", "-EXIF:Copyright"], &path).contains("Big Endian"));
    assert!(exiftool(&["-s3", "-EXIF:ExposureTime"], &path).contains("1/125"));
    assert!(exiftool(&["-s3", "-IPTC:CopyrightNotice"], &path).contains("Big IIM"));
}

#[test]
fn external_sidecar_cannot_hide_embedded_person_identity_from_privacy_filter() {
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("source.jpg");
    fixture(&input);
    let pixels = Image::new(16, 16, vec![vec![0.2; 256]; 3]).unwrap();
    let packet=sidecar::XmpPacket::parse(r#"<x:xmpmeta xmlns:x="adobe:ns:meta/"><rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#"><rdf:Description xmlns:dc="http://purl.org/dc/elements/1.1/"><dc:subject><rdf:Bag><rdf:li>Alice</rdf:li><rdf:li>Landscape</rdf:li></rdf:Bag></dc:subject></rdf:Description></rdf:RDF></x:xmpmeta>"#).unwrap();
    let path = export_one(
        &ExportImage {
            source: RenderSource::Rgb(&pixels),
            name: "private",
            sequence: 1,
            date: "",
            metadata: Some(&packet),
        },
        &Default::default(),
        &ExportSettings {
            output_dir: dir.path().into(),
            metadata_sources: [(1, input)].into(),
            remove_person_info: true,
            ..Default::default()
        },
    )
    .unwrap();
    let all = exiftool(&["-G", "-s"], &path);
    assert!(!all.contains("Alice"), "{all}");
    assert!(all.contains("Landscape"));
    assert!(all.contains("Nature"));
}

#[test]
fn malformed_native_metadata_never_publishes_an_output() {
    let pixels = Image::new(4, 4, vec![vec![0.2; 16]; 3]).unwrap();
    let image = ExportImage {
        source: RenderSource::Rgb(&pixels),
        name: "bad",
        sequence: 1,
        date: "",
        metadata: None,
    };
    let entry = |id: u16, kind: u16, count: u32, value: u32| {
        [
            id.to_le_bytes().as_slice(),
            &kind.to_le_bytes(),
            &count.to_le_bytes(),
            &value.to_le_bytes(),
        ]
        .concat()
    };
    let tiff = |entries: Vec<Vec<u8>>| {
        [
            b"II\x2a\0\x08\0\0\0".as_slice(),
            &(entries.len() as u16).to_le_bytes(),
            &entries.concat(),
            &[0; 4],
        ]
        .concat()
    };
    for bytes in [
        tiff(vec![entry(34665, 4, 1, 8)]), // directory cycle
        tiff(vec![entry(33432, 2, 2, 0), entry(33432, 2, 2, 0)]),
        tiff(vec![entry(33432, 2, u32::MAX, 26)]),
        tiff(vec![entry(33432, 2, 100, 26)]),
        tiff(vec![entry(34665, 3, 1, 8)]),
        b"II\x2b\0\x08\0\0\0".to_vec(),
        vec![255, 216, 255, 225, 0, 1],
    ] {
        let dir = tempfile::tempdir().unwrap();
        let input = dir.path().join("source.raw");
        std::fs::write(&input, bytes).unwrap();
        let out = dir.path().join("out");
        let result = export_one(
            &image,
            &Default::default(),
            &ExportSettings {
                output_dir: out.clone(),
                metadata_sources: [(1, input)].into(),
                ..Default::default()
            },
        );
        assert!(result.is_err(), "{result:?}");
        assert!(!out.exists());
    }
}

#[test]
fn heif_uses_metadata_associated_with_primary_image_only() {
    fn bx(kind: &[u8; 4], data: &[u8]) -> Vec<u8> {
        [(data.len() as u32 + 8).to_be_bytes().as_slice(), kind, data].concat()
    }
    fn full(kind: &[u8; 4], version: u8, data: &[u8]) -> Vec<u8> {
        bx(kind, &[&[version, 0, 0, 0], data].concat())
    }
    fn exif(rights: &[u8]) -> Vec<u8> {
        let mut b = b"\0\0\0\0II\x2a\0\x08\0\0\0\x01\0\x98\x82\x02\0".to_vec();
        b.extend((rights.len() as u32).to_le_bytes());
        b.extend(26u32.to_le_bytes());
        b.extend([0; 4]);
        b.extend(rights);
        b
    }
    let wrong = exif(b"Other image rights\0");
    let right = exif(b"Primary rights\0");
    let mut info = 2u16.to_be_bytes().to_vec();
    for id in [2u16, 3] {
        info.extend(full(
            b"infe",
            2,
            &[&id.to_be_bytes(), b"\0\0Exif\0".as_slice()].concat(),
        ));
    }
    let mut iloc = vec![0x44, 0, 0, 2];
    for (id, offset, size) in [
        (2u16, 0u32, wrong.len()),
        (3, wrong.len() as u32, right.len()),
    ] {
        iloc.extend(id.to_be_bytes());
        iloc.extend([0, 1, 0, 0, 0, 1]);
        iloc.extend(offset.to_be_bytes());
        iloc.extend((size as u32).to_be_bytes());
    }
    let refs = [
        bx(b"cdsc", &[0, 2, 0, 1, 0, 4]),
        bx(b"cdsc", &[0, 3, 0, 1, 0, 1]),
    ]
    .concat();
    let meta = [
        full(b"pitm", 0, &[0, 1]),
        full(b"iinf", 0, &info),
        full(b"iloc", 1, &iloc),
        full(b"iref", 0, &refs),
        bx(b"idat", &[wrong, right].concat()),
    ]
    .concat();
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("source.avif");
    std::fs::write(
        &input,
        [
            bx(b"ftyp", b"avif\0\0\0\0avifmif1"),
            full(b"meta", 0, &meta),
        ]
        .concat(),
    )
    .unwrap();
    let pixels = Image::new(4, 4, vec![vec![0.2; 16]; 3]).unwrap();
    let result = export_one(
        &ExportImage {
            source: RenderSource::Rgb(&pixels),
            name: "primary",
            sequence: 1,
            date: "",
            metadata: None,
        },
        &Default::default(),
        &ExportSettings {
            output_dir: dir.path().into(),
            metadata_sources: [(1, input)].into(),
            ..Default::default()
        },
    );
    let path = result.unwrap();
    let all = exiftool(&["-s3", "-EXIF:Copyright"], &path);
    assert_eq!(all.trim(), "Primary rights");
}

#[test]
fn latin1_iptc_person_keywords_are_removed_without_losing_other_keywords() {
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("source.jpg");
    fixture(&input);
    exiftool(
        &[
            "-overwrite_original",
            "-charset",
            "IPTC=Latin",
            "-IPTC:CodedCharacterSet=",
            "-IPTC:Keywords=Renée",
            "-IPTC:Keywords+=Nature",
            "-XMP-iptcExt:PersonInImage=Renée",
            "-XMP-lr:HierarchicalSubject=People|Renée",
        ],
        &input,
    );
    assert!(
        std::fs::read(&input)
            .unwrap()
            .windows(5)
            .any(|b| b == b"Ren\xe9e")
    );
    let pixels = Image::new(4, 4, vec![vec![0.2; 16]; 3]).unwrap();
    for person in [false, true] {
        let out = tempfile::tempdir().unwrap();
        let path = export_one(
            &ExportImage {
                source: RenderSource::Rgb(&pixels),
                name: "person",
                sequence: 1,
                date: "",
                metadata: None,
            },
            &Default::default(),
            &ExportSettings {
                output_dir: out.path().into(),
                metadata_sources: [(1, input.clone())].into(),
                remove_person_info: person,
                ..Default::default()
            },
        )
        .unwrap();
        let keywords = exiftool(&["-s3", "-IPTC:Keywords"], &path);
        assert_eq!(keywords.contains("Renée"), !person, "{keywords}");
        assert!(keywords.contains("Nature"));
        let hierarchy = exiftool(&["-s3", "-XMP-lr:HierarchicalSubject"], &path);
        assert_eq!(hierarchy.contains("Renée"), !person, "{hierarchy}");
        assert!(!hierarchy.contains('\u{fffd}'));
    }
}

#[test]
fn native_metadata_survives_hdr_carriers_and_jpeg_budget_counts_it() {
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("source.jpg");
    fixture(&input);
    let pixels = Image::new(16, 16, vec![vec![0.2; 256]; 3]).unwrap();
    let image = ExportImage {
        source: RenderSource::Rgb(&pixels),
        name: "hdr",
        sequence: 1,
        date: "",
        metadata: None,
    };
    for transfer in [export::HdrTransfer::Pq, export::HdrTransfer::Hlg] {
        for format in [
            Format::Png,
            Format::Avif(export::AvifOptions {
                bits: 12,
                speed: 10,
                ..Default::default()
            }),
        ] {
            let out = tempfile::tempdir().unwrap();
            let path = export_one(
                &image,
                &Default::default(),
                &ExportSettings {
                    format,
                    hdr: Some(transfer),
                    color_space: export::ColorSpace::Rec2020,
                    output_dir: out.path().into(),
                    metadata_sources: [(1, input.clone())].into(),
                    remove_location: true,
                    ..Default::default()
                },
            )
            .unwrap();
            let all = exiftool(&["-G", "-s"], &path);
            assert!(all.contains("Native Rights"));
            assert!(all.contains("IIM Rights"));
            assert!(!all.contains("Private City"));
            #[cfg(target_os = "macos")]
            {
                let decoded = color_mgmt::decode_to_tiff(&std::fs::read(&path).unwrap()).unwrap();
                let mut decoder =
                    tiff::decoder::Decoder::new(std::io::Cursor::new(decoded)).unwrap();
                assert_eq!(decoder.dimensions().unwrap(), (16, 16));
                decoder.read_image().unwrap();
            }
        }
    }
    exiftool(
        &[
            "-overwrite_original",
            &format!("-EXIF:Copyright={}", "x".repeat(8000)),
        ],
        &input,
    );
    let out = dir.path().join("limited");
    let result = export_one(
        &image,
        &Default::default(),
        &ExportSettings {
            output_dir: out.clone(),
            metadata_sources: [(1, input)].into(),
            max_file_bytes: Some(4000),
            ..Default::default()
        },
    );
    assert!(result.is_err());
    assert!(!out.join("hdr-1.jpg").exists());
}

#[test]
fn independently_written_png_and_tiff_metadata_is_extracted() {
    let pixels = Image::new(4, 4, vec![vec![0.2; 16]; 3]).unwrap();
    for extension in ["png", "tif"] {
        let dir = tempfile::tempdir().unwrap();
        let input = dir.path().join(format!("source.{extension}"));
        fixture(&input);
        let path = export_one(
            &ExportImage {
                source: RenderSource::Rgb(&pixels),
                name: "imported",
                sequence: 1,
                date: "",
                metadata: None,
            },
            &Default::default(),
            &ExportSettings {
                output_dir: dir.path().into(),
                metadata_sources: [(1, input)].into(),
                ..Default::default()
            },
        )
        .unwrap();
        let all = exiftool(&["-G", "-s"], &path);
        for value in [
            "Native Rights",
            "IIM Rights",
            "Native Camera",
            "Private City",
            "Alice",
            "Nature",
            "contact@example.test",
        ] {
            assert!(all.contains(value), "{extension}: missing {value}\n{all}");
        }
    }
}

#[test]
fn compressed_native_profiles_share_an_aggregate_metadata_budget() {
    use std::io::Write;
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("oversized.png");
    let mut iim = vec![0x1c, 2, 116, 0x80, 4];
    iim.extend(120_000u32.to_be_bytes());
    iim.resize(iim.len() + 120_000, b'x');
    let mut irb = b"8BIM\x04\x04\0\0".to_vec();
    irb.extend((iim.len() as u32).to_be_bytes());
    irb.extend(iim);
    if !irb.len().is_multiple_of(2) {
        irb.push(0);
    }
    let mut text = format!("\nIPTC profile\n{}\n", irb.len());
    for b in &irb {
        use std::fmt::Write;
        write!(text, "{b:02x}").unwrap();
    }
    let mut z = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
    z.write_all(text.as_bytes()).unwrap();
    let compressed = z.finish().unwrap();
    let mut chunk = b"Raw profile type iptc\0\0".to_vec();
    chunk.extend(compressed);
    let mut encoder = png::Encoder::new(std::fs::File::create(&input).unwrap(), 1, 1);
    encoder.set_color(png::ColorType::Rgb);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder.write_header().unwrap();
    for _ in 0..24 {
        writer
            .write_chunk(png::chunk::ChunkType(*b"zTXt"), &chunk)
            .unwrap();
    }
    writer.write_image_data(&[90; 3]).unwrap();
    writer.finish().unwrap();
    let pixels = Image::new(4, 4, vec![vec![0.2; 16]; 3]).unwrap();
    let out = dir.path().join("out");
    let result = export_one(
        &ExportImage {
            source: RenderSource::Rgb(&pixels),
            name: "budget",
            sequence: 1,
            date: "",
            metadata: None,
        },
        &Default::default(),
        &ExportSettings {
            format: Format::Png,
            output_dir: out.clone(),
            metadata_sources: [(1, input)].into(),
            ..Default::default()
        },
    );
    assert!(
        result.is_err(),
        "expanded native metadata must not bypass the aggregate budget"
    );
    assert!(!out.exists());
}

#[test]
fn review_regressions_reject_misplaced_pointers_and_extended_xmp() {
    let dir = tempfile::tempdir().unwrap();
    let pixels = Image::new(4, 4, vec![vec![0.2; 16]; 3]).unwrap();
    let source = ExportImage {
        source: RenderSource::Rgb(&pixels),
        name: "bad",
        sequence: 1,
        date: "",
        metadata: None,
    };
    // Root ExifIFD pointer -> an ExifIFD containing an invalid GPS pointer.
    let mut tiff=b"II\x2a\0\x08\0\0\0\x01\0\x69\x87\x04\0\x01\0\0\0\x1a\0\0\0\0\0\0\0\x01\0\x25\x88\x04\0\x01\0\0\0\0\0\x10\0\0\0\0\0".to_vec();
    let extension = b"http://ns.adobe.com/xmp/extension/\0unassembled person identities";
    let jpeg = [
        b"\xff\xd8\xff\xe1".as_slice(),
        &((extension.len() + 2) as u16).to_be_bytes(),
        extension,
        b"\xff\xd9\0\0",
    ]
    .concat();
    for bytes in [&mut tiff, &mut jpeg.clone()] {
        let path = dir.path().join("input");
        std::fs::write(&path, bytes).unwrap();
        for format in [
            Format::Tiff { bits: 16 },
            Format::Dng,
            Format::Jpeg { quality: 90 },
        ] {
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                export_one(
                    &source,
                    &Default::default(),
                    &ExportSettings {
                        format,
                        output_dir: dir.path().join("out"),
                        metadata_sources: [(1, path.clone())].into(),
                        remove_person_info: true,
                        ..Default::default()
                    },
                )
            }));
            assert!(result.is_ok(), "malformed metadata must not panic");
            assert!(
                result.unwrap().is_err(),
                "unsupported metadata must fail closed"
            );
        }
    }
}

#[test]
fn sidecar_overrides_properties_without_discarding_embedded_descriptive_metadata() {
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("input.jpg");
    fixture(&input);
    exiftool(
        &[
            "-overwrite_original",
            "-XMP-dc:Rights=Embedded Rights",
            "-XMP-dc:Description=Embedded Caption",
            "-XMP-iptcCore:CreatorWorkEmail=embedded@example.test",
        ],
        &input,
    );
    let packet=sidecar::XmpPacket::parse(r#"<x:xmpmeta xmlns:x="adobe:ns:meta/" xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#" xmlns:dc="http://purl.org/dc/elements/1.1/"><rdf:RDF><rdf:Description><dc:description>Sidecar Caption</dc:description></rdf:Description></rdf:RDF></x:xmpmeta>"#).unwrap();
    let pixels = Image::new(16, 16, vec![vec![0.2; 256]; 3]).unwrap();
    let source = ExportImage {
        source: RenderSource::Rgb(&pixels),
        name: "merged",
        sequence: 1,
        date: "",
        metadata: Some(&packet),
    };
    for (i, policy) in [
        Metadata::All,
        Metadata::CopyrightOnly,
        Metadata::CopyrightAndContact,
    ]
    .into_iter()
    .enumerate()
    {
        let path = export_one(
            &source,
            &Default::default(),
            &ExportSettings {
                metadata: policy,
                output_dir: dir.path().join(i.to_string()),
                metadata_sources: [(1, input.clone())].into(),
                keywords_as_hierarchy: true,
                ..Default::default()
            },
        )
        .unwrap();
        let all = exiftool(&["-G", "-s", "-XMP:All"], &path);
        assert!(all.contains("Embedded Rights"), "{policy:?}: {all}");
        if matches!(policy, Metadata::All) {
            assert!(all.contains("Sidecar Caption"), "{all}");
            assert!(all.contains("People|Alice"), "{all}");
            assert!(!all.contains("Embedded Caption"));
        }
        if !matches!(policy, Metadata::CopyrightOnly) {
            assert!(all.contains("embedded@example.test"), "{all}");
        }
    }
}
