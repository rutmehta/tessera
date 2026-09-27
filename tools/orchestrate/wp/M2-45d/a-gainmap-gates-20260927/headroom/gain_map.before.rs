use engine_api::recipe::{EditMeta, Recipe};
use export::{ColorSpace, ExportImage, ExportSettings, Format, HdrTransfer, export_one};
use pipeline_cpu::{Image, RenderSource};
use std::fs;
#[cfg(target_os = "macos")]
#[path = "support/gain_map_imageio.rs"]
mod native_decode;
const ISO: &[u8] = b"urn:iso:std:iso:ts:21496:-1\0";
fn segments(bytes: &[u8]) -> Vec<(usize, u8, &[u8])> {
    assert_eq!(&bytes[..2], b"\xff\xd8");
    let mut result = Vec::new();
    let mut p = 2;
    while bytes[p + 1] != 0xda {
        let n = u16::from_be_bytes(bytes[p + 2..p + 4].try_into().unwrap()) as usize;
        result.push((p, bytes[p + 1], &bytes[p + 4..p + 2 + n]));
        p += 2 + n;
    }
    result
}
fn images(bytes: &[u8]) -> (&[u8], &[u8]) {
    let parts = segments(bytes);
    let (pos, _, mpf) = parts
        .iter()
        .find(|(_, m, b)| *m == 0xe2 && b.starts_with(b"MPF\0"))
        .unwrap();
    let t = &mpf[4..];
    assert_eq!(&t[..4], b"MM\0\x2a");
    let d = u32::from_be_bytes(t[4..8].try_into().unwrap()) as usize;
    let count = u16::from_be_bytes(t[d..d + 2].try_into().unwrap()) as usize;
    let mut entries = None;
    for e in t[d + 2..d + 2 + count * 12].as_chunks::<12>().0 {
        if e[..2] == [0xb0, 2] {
            assert_eq!(u32::from_be_bytes(e[4..8].try_into().unwrap()), 32);
            let at = u32::from_be_bytes(e[8..12].try_into().unwrap()) as usize;
            entries = Some(&t[at..at + 32]);
        }
    }
    let e = entries.unwrap();
    let base_len = u32::from_be_bytes(e[4..8].try_into().unwrap()) as usize;
    let gain_len = u32::from_be_bytes(e[20..24].try_into().unwrap()) as usize;
    let gain_start = pos + 8 + u32::from_be_bytes(e[24..28].try_into().unwrap()) as usize;
    assert_eq!(gain_start, base_len);
    assert_eq!(gain_start + gain_len, bytes.len());
    (&bytes[..base_len], &bytes[gain_start..])
}
fn rational(b: &[u8]) -> f64 {
    i32::from_be_bytes(b[..4].try_into().unwrap()) as f64
        / u32::from_be_bytes(b[4..8].try_into().unwrap()) as f64
}
fn inverse_srgb(v: u8) -> f64 {
    let v = v as f64 / 255.;
    if v <= 0.04045 {
        v / 12.92
    } else {
        ((v + 0.055) / 1.055).powf(2.4)
    }
}
fn inverse_pq(v: u16) -> f64 {
    let v = (v as f64 / 65535.).powf(32. / 2523.);
    ((v - 3424. / 4096.).max(0.) / (2413. / 128. - 2392. / 128. * v)).powf(16384. / 2610.) * 10000.
        / 203.
}
#[test]
fn iso_gain_map_reconstructs_reference_hdr_and_has_real_mpf_associations() {
    let mut recipe = Recipe::default();
    recipe
        .edit(EditMeta::user("HDR", 0), |s| {
            s.output.hdr = true;
            s.output.hdr_headroom_stops = 2.;
        })
        .unwrap();
    let samples = [
        [0., 0., 0.],
        [0.18, 0.18, 0.18],
        [0.8, 0.7, 0.6],
        [4., 3.8, 3.6],
        [32., 32., 32.],
    ];
    let pixels = Image::new(
        80,
        16,
        (0..3)
            .map(|c| (0..1280).map(|i| samples[(i % 80) / 16][c]).collect())
            .collect(),
    )
    .unwrap();
    let source = ExportImage {
        source: RenderSource::Rgb(&pixels),
        name: "gain",
        sequence: 1,
        date: "",
        metadata: None,
    };
    let dir = tempfile::tempdir().unwrap();
    let path = export_one(
        &source,
        &recipe,
        &ExportSettings {
            gain_map: true,
            format: Format::Jpeg { quality: 100 },
            output_dir: dir.path().into(),
            ..Default::default()
        },
    )
    .unwrap();
    let bytes = fs::read(&path).unwrap();
    let (base, gain) = images(&bytes);
    let primary = segments(base);
    assert!(
        primary
            .iter()
            .any(|(_, m, b)| *m == 0xe2 && b.strip_prefix(ISO) == Some(&[0, 0, 0, 0][..]))
    );
    let aux = segments(gain);
    let data = aux
        .iter()
        .find_map(|(_, m, b)| (*m == 0xe2).then(|| b.strip_prefix(ISO)).flatten())
        .unwrap();
    assert_eq!(data.len(), 61);
    assert_eq!(&data[..5], &[0, 0, 0, 0, 0x40]); // ISO v0, one channel, base colour space; reserved bits clear.
    assert_eq!(rational(&data[5..13]), 0.);
    assert!((rational(&data[13..21]) - 2.).abs() < 0.0001);
    let min = rational(&data[21..29]);
    let max = rational(&data[29..37]);
    let gamma = rational(&data[37..45]);
    assert_eq!((min, max, gamma), (0., 2., 1.));
    let base_offset = rational(&data[45..53]);
    let alt_offset = rational(&data[53..61]);
    assert_eq!((base_offset, alt_offset), (0., 0.));
    let sdr = image::load_from_memory(base).unwrap().to_rgb8();
    let map = image::load_from_memory(gain).unwrap().to_luma8();
    assert_eq!(map.dimensions(), sdr.dimensions());
    let reference = export_one(
        &source,
        &recipe,
        &ExportSettings {
            format: Format::Png,
            hdr: Some(HdrTransfer::Pq),
            color_space: ColorSpace::Rec2020,
            naming: "reference".into(),
            output_dir: dir.path().into(),
            ..Default::default()
        },
    )
    .unwrap();
    let pq = image::open(reference).unwrap().to_rgb16();
    let mut expected_patches = Vec::new();
    for x in [8, 24, 40, 56, 72] {
        let linear = pq.get_pixel(x, 8).0.map(inverse_pq);
        let [r, g, b] = linear;
        let expected = [
            1.660491 * r - 0.587641 * g - 0.072850 * b,
            -0.124550 * r + 1.132900 * g - 0.008349 * b,
            -0.018151 * r - 0.100579 * g + 1.118730 * b,
        ];
        expected_patches.push(expected);
        let logarithm = min + (max - min) * (map.get_pixel(x, 8)[0] as f64 / 255.).powf(1. / gamma);
        for (actual, expected) in sdr
            .get_pixel(x, 8)
            .0
            .into_iter()
            .map(inverse_srgb)
            .zip(expected)
        {
            let reconstructed = (actual + base_offset) * logarithm.exp2() - alt_offset;
            assert!(
                (reconstructed - expected).abs() < 0.04 * expected.max(1.),
                "{x}: {reconstructed} vs {expected}"
            );
        }
    }
    let out = std::process::Command::new("exiftool")
        .args(["-b", "-MPImage2"])
        .arg(&path)
        .output()
        .unwrap();
    assert!(out.status.success());
    assert_eq!(out.stdout, gain);
    #[cfg(target_os = "macos")]
    verify_native_pixels_and_loss_controls(&bytes, &sdr, &map, &expected_patches);
}

#[cfg(target_os = "macos")]
fn verify_native_pixels_and_loss_controls(
    bytes: &[u8],
    base: &image::RgbImage,
    map: &image::GrayImage,
    expected: &[[f64; 3]],
) {
    let sdr = native_decode::decode(bytes, false, true);
    let hdr = native_decode::decode(bytes, true, true);
    for (x, expected) in [8usize, 24, 40, 56, 72].into_iter().zip(expected) {
        for channel in 0..3 {
            let actual = f64::from(hdr[8 * 80 + x][channel]);
            assert!(
                (actual - expected[channel]).abs() < 0.04 * expected[channel].max(1.),
                "native HDR patch {x}/{channel}: {actual} vs {}",
                expected[channel]
            );
            let actual_sdr = f64::from(sdr[8 * 80 + x][channel]);
            let expected_sdr = inverse_srgb(base.get_pixel(x as u32, 8)[channel]);
            assert!(
                (actual_sdr - expected_sdr).abs() < 0.04,
                "native SDR patch {x}/{channel}"
            );
        }
    }
    // Report all pixels without claiming the five-center 4% criterion holds
    // at lossy sharp boundaries. Diagnostic baseline: worst normalized 5.0658%.
    let mut max_abs = 0f64;
    let mut max_normalized = 0f64;
    let mut sum = 0f64;
    for (i, pixel) in hdr.iter().enumerate() {
        let x = (i % 80) as u32;
        let y = (i / 80) as u32;
        let gain = (2. * f64::from(map.get_pixel(x, y)[0]) / 255.).exp2();
        for (actual, code) in pixel[..3].iter().zip(base.get_pixel(x, y).0) {
            let reconstructed = inverse_srgb(code) * gain;
            let error = (f64::from(*actual) - reconstructed).abs();
            max_abs = max_abs.max(error);
            max_normalized = max_normalized.max(error / reconstructed.max(1.));
            sum += error;
        }
    }
    eprintln!(
        "gain-map whole-frame native vs independent reconstruction: max_abs={max_abs:.9}, mean_abs={:.9}, max_normalized={max_normalized:.9}; 4% acceptance applies to patch centers only",
        sum / (80. * 16. * 3.)
    );
    assert!(hdr[8 * 80 + 72][0] > 3.8);
    assert!(sdr[8 * 80 + 72][0] < 1.04);
    // Corrupt recognition signatures without moving any bytes: offsets and the
    // SDR codestream remain intact, isolating the two association requirements.
    for marker in [ISO, b"MPF\0".as_slice()] {
        let mut missing = bytes.to_vec();
        let positions: Vec<_> = missing
            .windows(marker.len())
            .enumerate()
            .filter_map(|(i, w)| (w == marker).then_some(i))
            .collect();
        assert!(!positions.is_empty());
        for position in positions {
            missing[position] = b'?';
        }
        let control_sdr = native_decode::decode(&missing, false, false);
        let control_hdr = native_decode::decode(&missing, true, false);
        for (sdr, hdr) in control_sdr.iter().zip(&control_hdr) {
            for channel in 0..3 {
                assert!(
                    (sdr[channel] - hdr[channel]).abs() < 0.0001,
                    "missing association must not reconstruct HDR"
                );
            }
        }
        assert!(control_hdr[8 * 80 + 72][0] < 1.04);
    }
}

fn hdr_recipe() -> Recipe {
    let mut recipe = Recipe::default();
    recipe
        .edit(EditMeta::user("HDR", 0), |s| {
            s.output.hdr = true;
            s.output.hdr_headroom_stops = 2.;
        })
        .unwrap();
    recipe
}

#[test]
fn gain_map_rejects_unsupported_settings_and_obeys_budget_and_no_clobber() {
    let pixels = Image::new(80, 16, vec![vec![4.; 1280]; 3]).unwrap();
    let source = ExportImage {
        source: RenderSource::Rgb(&pixels),
        name: "gain",
        sequence: 1,
        date: "",
        metadata: None,
    };
    let root = tempfile::tempdir().unwrap();
    let valid = ExportSettings {
        gain_map: true,
        format: Format::Jpeg { quality: 100 },
        metadata: export::Metadata::None,
        output_dir: root.path().join("valid"),
        ..Default::default()
    };
    let recipe = hdr_recipe();
    let path = export_one(&source, &recipe, &valid).unwrap();
    let bytes = fs::read(&path).unwrap();
    assert!(export_one(&source, &recipe, &valid).is_err());
    assert_eq!(fs::read(&path).unwrap(), bytes, "no overwrite");
    let limited = ExportSettings {
        max_file_bytes: Some(bytes.len() as u64),
        output_dir: root.path().join("limited"),
        ..valid.clone()
    };
    let path = export_one(&source, &recipe, &limited).unwrap();
    assert!(fs::metadata(&path).unwrap().len() <= bytes.len() as u64);
    images(&fs::read(path).unwrap());
    let impossible = ExportSettings {
        max_file_bytes: Some(100),
        output_dir: root.path().join("impossible"),
        ..valid.clone()
    };
    assert!(export_one(&source, &recipe, &impossible).is_err());
    assert!(
        !impossible.output_dir.exists()
            || fs::read_dir(&impossible.output_dir)
                .unwrap()
                .next()
                .is_none()
    );
    for (i, mut bad) in [
        ExportSettings {
            format: Format::Png,
            ..valid.clone()
        },
        ExportSettings {
            color_space: ColorSpace::Rec2020,
            ..valid.clone()
        },
        ExportSettings {
            hdr: Some(HdrTransfer::Pq),
            ..valid.clone()
        },
        ExportSettings {
            watermark: Some(export::Watermark::Graphic {
                path: root.path().join("must-not-load.png"),
                scale: 0.2,
                opacity: 1.,
                anchor: export::Anchor::BottomRight,
                inset: 0.,
            }),
            ..valid.clone()
        },
        ExportSettings {
            max_file_bytes: Some(0),
            ..valid.clone()
        },
    ]
    .into_iter()
    .enumerate()
    {
        bad.output_dir = root.path().join(format!("bad-{i}"));
        assert!(export_one(&source, &recipe, &bad).is_err());
        assert!(!bad.output_dir.exists());
    }
    for (enabled, stops) in [(false, 2.), (true, 0.)] {
        let mut no_headroom = Recipe::default();
        no_headroom
            .edit(EditMeta::user("no headroom", 0), |s| {
                s.output.hdr = enabled;
                s.output.hdr_headroom_stops = stops;
            })
            .unwrap();
        let bad = ExportSettings {
            output_dir: root.path().join("no-headroom"),
            ..valid.clone()
        };
        assert!(export_one(&source, &no_headroom, &bad).is_err());
        assert!(!bad.output_dir.exists());
    }
    let mut masked = recipe.clone();
    masked
        .edit(EditMeta::user("AI mask", 0), |s| {
            s.locals
                .adjustments
                .push(engine_api::recipe::mask::LocalAdjustment {
                    components: vec![engine_api::recipe::mask::MaskComponent::new(
                        engine_api::recipe::mask::MaskKind::Subject { model: None },
                    )],
                    ..Default::default()
                });
        })
        .unwrap();
    let masked_settings = ExportSettings {
        output_dir: root.path().join("masked"),
        ..valid.clone()
    };
    let error = export_one(&source, &masked, &masked_settings).unwrap_err();
    assert!(
        error
            .to_string()
            .contains("HDR export does not support SDR enhancement hooks"),
        "{error}"
    );
    assert!(!masked_settings.output_dir.exists());
    let cancelled = ExportSettings {
        output_dir: root.path().join("cancelled"),
        ..valid
    };
    let token = engine_api::jobs::CancellationToken::new();
    let rendered =
        export::render_one_cancellable(&source, &recipe, &cancelled, &token, None, None).unwrap();
    assert!(!rendered.used_gpu());
    token.cancel();
    assert!(rendered.finish(&token).is_err());
    assert!(!cancelled.output_dir.exists());
}

#[test]
fn gain_map_keeps_privacy_policy_in_native_and_xmp_carriers() {
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("input.jpg");
    image::RgbImage::from_pixel(80, 16, image::Rgb([255; 3]))
        .save(&input)
        .unwrap();
    let written = std::process::Command::new("exiftool")
        .args([
            "-overwrite_original",
            "-EXIF:Artist=Photographer",
            "-EXIF:Copyright=Rights",
            "-EXIF:GPSLatitude=40.5",
            "-EXIF:GPSLatitudeRef=N",
            "-IPTC:Keywords=Alice",
            "-IPTC:Keywords+=Nature",
            "-XMP-iptcExt:PersonInImage=Alice",
            "-XMP-lr:HierarchicalSubject=People|Alice",
        ])
        .arg(&input)
        .output()
        .unwrap();
    assert!(written.status.success());
    let original = fs::read(&input).unwrap();
    let pixels = Image::new(80, 16, vec![vec![4.; 1280]; 3]).unwrap();
    let source = ExportImage {
        source: RenderSource::Rgb(&pixels),
        name: "gain",
        sequence: 1,
        date: "",
        metadata: None,
    };
    for (name, policy, privacy) in [
        ("all", export::Metadata::All, false),
        ("private", export::Metadata::All, true),
        ("none", export::Metadata::None, true),
    ] {
        let path = export_one(
            &source,
            &hdr_recipe(),
            &ExportSettings {
                gain_map: true,
                format: Format::Jpeg { quality: 100 },
                metadata: policy,
                remove_person_info: privacy,
                remove_location: privacy,
                metadata_sources: [(1, input.clone())].into(),
                output_dir: dir.path().join(name),
                ..Default::default()
            },
        )
        .unwrap();
        let bytes = fs::read(&path).unwrap();
        let (_, aux) = images(&bytes);
        assert!(
            segments(aux)
                .iter()
                .any(|(_, marker, bytes)| *marker == 0xe2 && bytes.starts_with(ISO)),
            "format metadata must survive privacy policy"
        );
        let output = std::process::Command::new("exiftool")
            .args([
                "-j",
                "-G",
                "-s",
                "-EXIF:Artist",
                "-EXIF:Copyright",
                "-EXIF:GPSLatitude",
                "-IPTC:Keywords",
                "-XMP:Subject",
                "-XMP:PersonInImage",
                "-XMP:HierarchicalSubject",
            ])
            .arg(&path)
            .output()
            .unwrap();
        assert!(output.status.success());
        let tags: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
        let tags = &tags[0];
        if name == "none" {
            assert!(tags.get("EXIF:Artist").is_none());
            assert!(tags.get("IPTC:Keywords").is_none());
            assert!(tags.get("XMP:Subject").is_none());
        } else {
            assert_eq!(tags["EXIF:Artist"], "Photographer");
            assert_eq!(tags["EXIF:Copyright"], "Rights");
            let keywords = tags["IPTC:Keywords"].to_string();
            assert!(keywords.contains("Nature"));
            assert_eq!(keywords.contains("Alice"), !privacy);
        }
        if privacy {
            assert!(tags.get("EXIF:GPSLatitude").is_none());
            assert!(tags.get("XMP:PersonInImage").is_none());
            assert!(
                !tags["XMP:HierarchicalSubject"]
                    .to_string()
                    .contains("Alice")
            );
        }
        #[cfg(target_os = "macos")]
        {
            let hdr = native_decode::decode(&bytes, true, true);
            assert!(hdr[8 * 80 + 40][0] > 3.);
        }
    }
    assert_eq!(fs::read(input).unwrap(), original);
}

#[test]
fn gain_map_resize_and_sharpen_preserve_recipe_headroom() {
    let plane = (0..160 * 32)
        .map(|i| if i % 160 < 80 { 0. } else { 1000. })
        .collect::<Vec<_>>();
    let pixels = Image::new(160, 32, vec![plane; 3]).unwrap();
    let source = ExportImage {
        source: RenderSource::Rgb(&pixels),
        name: "edge",
        sequence: 1,
        date: "",
        metadata: None,
    };
    for stops in [1., 2., 4.] {
        let mut recipe = hdr_recipe();
        recipe
            .edit(EditMeta::user("headroom", 0), |s| {
                s.output.hdr_headroom_stops = stops
            })
            .unwrap();
        let dir = tempfile::tempdir().unwrap();
        let path = export_one(
            &source,
            &recipe,
            &ExportSettings {
                gain_map: true,
                format: Format::Jpeg { quality: 100 },
                resize: export::Resize::Fit(80, 16),
                sharpen_for: export::SharpenFor::Screen,
                sharpen_amount: export::SharpenAmount::High,
                metadata: export::Metadata::None,
                output_dir: dir.path().into(),
                ..Default::default()
            },
        )
        .unwrap();
        let bytes = fs::read(path).unwrap();
        let (base, aux) = images(&bytes);
        let base = image::load_from_memory(base).unwrap().to_rgb8();
        let map = image::load_from_memory(aux).unwrap().to_luma8();
        assert_eq!(base.dimensions(), (80, 16));
        assert_eq!(base.dimensions(), map.dimensions());
        let headroom = f64::from(stops).exp2();
        let reconstructed: Vec<_> = base
            .pixels()
            .zip(map.pixels())
            .flat_map(|(rgb, gain)| {
                rgb.0.map(|v| {
                    inverse_srgb(v) * (f64::from(stops) * f64::from(gain[0]) / 255.).exp2()
                })
            })
            .collect();
        assert!(
            reconstructed
                .iter()
                .all(|v| v.is_finite() && *v <= headroom * 1.0001)
        );
        assert!(reconstructed.iter().any(|v| *v < 0.001));
        let peak = reconstructed.iter().copied().fold(0., f64::max);
        assert!(
            (peak - headroom).abs() < 0.04 * headroom,
            "{peak} vs {headroom}"
        );
        #[cfg(target_os = "macos")]
        {
            let hdr = native_decode::decode(&bytes, true, true);
            let peak = hdr
                .iter()
                .flat_map(|v| &v[..3])
                .copied()
                .fold(0f32, f32::max);
            assert!(
                (f64::from(peak) - headroom).abs() < 0.04 * headroom,
                "native filtered peak {peak} vs {headroom}"
            );
        }
    }
}

#[test]
fn long_xmp_and_gain_map_metadata_source_do_not_duplicate_format_segments() {
    let dir = tempfile::tempdir().unwrap();
    let pixels = Image::new(80, 16, vec![vec![4.; 1280]; 3]).unwrap();
    let description = "descriptive metadata ".repeat(1500);
    let packet = sidecar::XmpPacket::parse(format!(r#"<x:xmpmeta xmlns:x="adobe:ns:meta/"><rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#"><rdf:Description xmlns:dc="http://purl.org/dc/elements/1.1/"><dc:description><rdf:Alt><rdf:li xml:lang="x-default">{description}</rdf:li></rdf:Alt></dc:description></rdf:Description></rdf:RDF></x:xmpmeta>"#)).unwrap();
    let mut source = ExportImage {
        source: RenderSource::Rgb(&pixels),
        name: "gain",
        sequence: 1,
        date: "",
        metadata: Some(&packet),
    };
    let settings = ExportSettings {
        gain_map: true,
        format: Format::Jpeg { quality: 100 },
        output_dir: dir.path().join("first"),
        ..Default::default()
    };
    let first = export_one(&source, &hdr_recipe(), &settings).unwrap();
    source.metadata = None;
    let second = export_one(
        &source,
        &hdr_recipe(),
        &ExportSettings {
            output_dir: dir.path().join("second"),
            metadata_sources: [(1, first.clone())].into(),
            ..settings
        },
    )
    .unwrap();
    for path in [first, second] {
        let bytes = fs::read(&path).unwrap();
        let (base, aux) = images(&bytes);
        assert_eq!(bytes.windows(ISO.len()).filter(|b| *b == ISO).count(), 2);
        assert_eq!(
            segments(base)
                .iter()
                .filter(|(_, marker, b)| *marker == 0xe2 && b.starts_with(b"MPF\0"))
                .count(),
            1
        );
        let extracted = std::process::Command::new("exiftool")
            .args(["-b", "-MPImage2"])
            .arg(&path)
            .output()
            .unwrap();
        assert!(extracted.status.success());
        assert_eq!(extracted.stdout, aux);
        let xmp = std::process::Command::new("exiftool")
            .args(["-b", "-XMP-dc:Description"])
            .arg(&path)
            .output()
            .unwrap();
        assert!(xmp.status.success());
        assert_eq!(String::from_utf8(xmp.stdout).unwrap(), description);
        #[cfg(target_os = "macos")]
        {
            let hdr = native_decode::decode(&bytes, true, true);
            assert!(hdr[8 * 80 + 40][0] > 3.);
        }
    }
}
