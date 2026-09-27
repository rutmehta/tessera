use engine_api::recipe::{EditMeta, Mark, Recipe, settings::WhiteBalanceMode};
use export::{ColorSpace, ExportImage, ExportSettings, Format, HdrTransfer, Metadata, export_one};
use pipeline_cpu::{Image, RenderSource};
use sidecar::XmpPacket;
use std::{fs, io::BufReader};

/// A rendered JPEG already contains its source development. Both XMP copies
/// must reopen as neutral settings even when All metadata was requested.
#[test]
fn baked_jpeg_metadata_does_not_reapply_source_development() {
    let dir = tempfile::tempdir().unwrap();
    let source = Image::new(16, 16, vec![vec![0.2; 256]; 3]).unwrap();
    let mut recipe = Recipe::default();
    recipe
        .edit(EditMeta::user("source develop", 1), |s| {
            s.tone.exposure = 1.0;
            s.white_balance.mode = WhiteBalanceMode::Custom;
            s.white_balance.temperature = 6200.0;
            s.white_balance.tint = 7.0;
        })
        .unwrap();
    recipe.selection.mark = Some(Mark::new("priority-a"));
    let metadata = sidecar::Metadata {
        copyright: "Copyright Example".into(),
        ..Default::default()
    };
    let source_packet =
        XmpPacket::from_recipe(&recipe, &metadata, &sidecar::MarkPreset::lightroom()).unwrap();
    let original_xmp = source_packet.serialize().to_owned();
    assert_eq!(
        source_packet
            .to_recipe()
            .unwrap()
            .recipe
            .settings
            .tone
            .exposure,
        1.0
    );

    let path = export_one(
        &ExportImage {
            source: RenderSource::Rgb(&source),
            name: "baked",
            sequence: 1,
            date: "",
            metadata: Some(&source_packet),
        },
        &recipe,
        &ExportSettings {
            format: Format::Jpeg { quality: 90 },
            metadata: Metadata::All,
            output_dir: dir.path().into(),
            ..Default::default()
        },
    )
    .unwrap();
    // The source packet and recipe are never changed by export.
    assert_eq!(source_packet.serialize(), original_xmp);
    assert_eq!(recipe.settings.tone.exposure, 1.0);
    let neutral_path = export_one(
        &ExportImage {
            source: RenderSource::Rgb(&source),
            name: "neutral",
            sequence: 1,
            date: "",
            metadata: None,
        },
        &Recipe::default(),
        &ExportSettings {
            format: Format::Jpeg { quality: 90 },
            metadata: Metadata::None,
            output_dir: dir.path().join("neutral"),
            ..Default::default()
        },
    )
    .unwrap();
    let baked_pixel = image::open(&path).unwrap().to_rgb8().get_pixel(8, 8).0;
    let neutral_pixel = image::open(&neutral_path)
        .unwrap()
        .to_rgb8()
        .get_pixel(8, 8)
        .0;
    assert!(
        baked_pixel
            .iter()
            .zip(neutral_pixel)
            .any(|(a, b)| a.abs_diff(b) > 8),
        "source development must be baked into JPEG pixels"
    );

    let embedded = XmpPacket::parse(embedded_xmp(&path, Format::Jpeg { quality: 90 })).unwrap();
    let adjacent = sidecar::Sidecar::read_xmp(sidecar::Sidecar::paths(&path).xmp).unwrap();
    for (where_, packet) in [("embedded", embedded), ("adjacent", adjacent)] {
        let reopened = packet.to_recipe().unwrap().recipe;
        assert_eq!(reopened.settings.tone.exposure, 0.0, "{where_}");
        assert_eq!(
            reopened.settings.white_balance.mode,
            WhiteBalanceMode::AsShot,
            "{where_}"
        );
        assert_eq!(
            reopened.settings.white_balance.temperature, 5500.0,
            "{where_}"
        );
        assert_eq!(reopened.settings.white_balance.tint, 0.0, "{where_}");
        assert_eq!(
            packet.selection().unwrap().mark,
            recipe.selection.mark,
            "{where_}"
        );
        assert!(packet.serialize().contains("ts:Mark"), "{where_}");
        assert_eq!(
            packet.metadata().unwrap().copyright,
            metadata.copyright,
            "{where_}"
        );
    }

    let restricted = export_one(
        &ExportImage {
            source: RenderSource::Rgb(&source),
            name: "restricted",
            sequence: 1,
            date: "",
            metadata: Some(&source_packet),
        },
        &recipe,
        &ExportSettings {
            format: Format::Jpeg { quality: 90 },
            metadata: Metadata::CopyrightOnly,
            output_dir: dir.path().join("restricted"),
            ..Default::default()
        },
    )
    .unwrap();
    let restricted_embedded = embedded_xmp(&restricted, Format::Jpeg { quality: 90 });
    let restricted_sidecar =
        sidecar::Sidecar::read_xmp(sidecar::Sidecar::paths(&restricted).xmp).unwrap();
    for xml in [restricted_embedded.as_str(), restricted_sidecar.serialize()] {
        assert!(xml.contains("Copyright Example"));
        assert!(
            !xml.contains("ts:Mark"),
            "copyright-only policy must still filter marks"
        );
        assert!(!xml.contains("crs:Exposure2012"));
    }
}

#[test]
fn baked_hdr_png_metadata_does_not_reapply_source_development() {
    let dir = tempfile::tempdir().unwrap();
    let source = Image::new(16, 16, vec![vec![0.2; 256]; 3]).unwrap();
    let mut recipe = Recipe::default();
    recipe
        .edit(EditMeta::user("source exposure", 1), |s| {
            s.tone.exposure = 1.0
        })
        .unwrap();
    let source_packet = XmpPacket::from_recipe(
        &recipe,
        &sidecar::Metadata::default(),
        &sidecar::MarkPreset::lightroom(),
    )
    .unwrap();
    let path = export_one(
        &ExportImage {
            source: RenderSource::Rgb(&source),
            name: "hdr",
            sequence: 1,
            date: "",
            metadata: Some(&source_packet),
        },
        &recipe,
        &ExportSettings {
            format: Format::Png,
            hdr: Some(HdrTransfer::Pq),
            color_space: ColorSpace::Rec2020,
            metadata: Metadata::All,
            output_dir: dir.path().into(),
            ..Default::default()
        },
    )
    .unwrap();
    let embedded = XmpPacket::parse(embedded_xmp(&path, Format::Png)).unwrap();
    let adjacent = sidecar::Sidecar::read_xmp(sidecar::Sidecar::paths(&path).xmp).unwrap();
    for packet in [embedded, adjacent] {
        assert_eq!(
            packet.to_recipe().unwrap().recipe.settings.tone.exposure,
            0.0
        );
        assert!(!packet.serialize().contains("crs:Exposure2012"));
    }
}

fn embedded_xmp(path: &std::path::Path, format: Format) -> String {
    let bytes = match format {
        Format::Jpeg { .. } => {
            use image::ImageDecoder;
            image::codecs::jpeg::JpegDecoder::new(BufReader::new(fs::File::open(path).unwrap()))
                .unwrap()
                .xmp_metadata()
                .unwrap()
                .unwrap()
        }
        Format::Png => {
            let reader = png::Decoder::new(fs::File::open(path).unwrap())
                .read_info()
                .unwrap();
            reader
                .info()
                .utf8_text
                .iter()
                .find(|t| t.keyword == "XML:com.adobe.xmp")
                .unwrap()
                .get_text()
                .unwrap()
                .into_bytes()
        }
        Format::Dng => raw_decode::linear_dng::read(&mut fs::File::open(path).unwrap())
            .unwrap()
            .xmp
            .into_bytes(),
        Format::Tiff { .. } => tiff::decoder::Decoder::new(fs::File::open(path).unwrap())
            .unwrap()
            .get_tag_u8_vec(tiff::tags::Tag::Unknown(700))
            .unwrap(),
        Format::JpegXl { .. } | Format::Avif(_) => {
            // These encoders write uncompressed XMP in XML / MIME items.
            let data = fs::read(path).unwrap();
            let start = data.windows(10).position(|v| v == b"<x:xmpmeta").unwrap();
            let end = data[start..]
                .windows(12)
                .position(|v| v == b"</x:xmpmeta>")
                .unwrap()
                + start
                + 12;
            data[start..end].to_vec()
        }
    };
    String::from_utf8(bytes).unwrap()
}

#[test]
fn xmp_policies_and_privacy_read_back_in_every_export_format() {
    let source = Image::new(16, 16, vec![vec![0.2; 256]; 3]).unwrap();
    let packet = XmpPacket::parse(r#"<x:xmpmeta xmlns:x="adobe:ns:meta/"><rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#"><rdf:Description xmlns:dc="http://purl.org/dc/elements/1.1/" xmlns:exif="http://ns.adobe.com/exif/1.0/" xmlns:iptc="http://iptc.org/std/Iptc4xmpCore/1.0/xmlns/" xmlns:m="http://www.metadataworkinggroup.com/schemas/regions/" xmlns:crs="http://ns.adobe.com/camera-raw-settings/1.0/" dc:rights="Copyright Example" exif:GPSLatitude="GPS-private" exif:ExposureTime="camera-value" crs:Exposure2012="baked-edit"><iptc:CreatorContactInfo rdf:parseType="Resource" iptc:CiEmailWork="contact@example.test"/><m:Regions rdf:parseType="Resource" m:Name="Face-private"/><dc:subject><rdf:Bag><rdf:li>Face-private</rdf:li><rdf:li>Nature</rdf:li></rdf:Bag></dc:subject></rdf:Description></rdf:RDF></x:xmpmeta>"#).unwrap();
    for format in [
        Format::Jpeg { quality: 85 },
        Format::Png,
        Format::Tiff { bits: 16 },
        Format::Dng,
        Format::Avif(export::AvifOptions {
            speed: 10,
            ..Default::default()
        }),
        Format::JpegXl { bits: 8 },
    ] {
        for policy in [
            Metadata::All,
            Metadata::CopyrightOnly,
            Metadata::CopyrightAndContact,
            Metadata::AllExceptCamera,
        ] {
            for remove in [false, true] {
                let dir = tempfile::tempdir().unwrap();
                let path = export_one(
                    &ExportImage {
                        source: RenderSource::Rgb(&source),
                        name: "policy",
                        sequence: 1,
                        date: "",
                        metadata: Some(&packet),
                    },
                    &Recipe::default(),
                    &ExportSettings {
                        format,
                        metadata: policy,
                        remove_person_info: remove,
                        remove_location: remove,
                        keywords_as_hierarchy: true,
                        output_dir: dir.path().into(),
                        ..Default::default()
                    },
                )
                .unwrap();
                let text = embedded_xmp(&path, format);
                assert!(text.contains("Copyright Example"), "{format:?} {policy:?}");
                let general = matches!(policy, Metadata::All | Metadata::AllExceptCamera);
                assert_eq!(
                    text.contains("contact@example.test"),
                    !matches!(policy, Metadata::CopyrightOnly),
                    "{format:?} {policy:?}"
                );
                assert_eq!(
                    text.contains("camera-value"),
                    matches!(policy, Metadata::All),
                    "{format:?} {policy:?}"
                );
                assert_eq!(
                    text.contains("GPS-private"),
                    matches!(policy, Metadata::All) && !remove,
                    "{format:?} {policy:?}"
                );
                assert_eq!(
                    text.contains("Face-private"),
                    general && !remove,
                    "{format:?} {policy:?}"
                );
                assert!(!text.contains("baked-edit"), "{format:?} {policy:?}");
                let out = XmpPacket::parse(text).unwrap();
                assert_eq!(
                    out.metadata()
                        .unwrap()
                        .hierarchical_keywords
                        .contains(&"Nature".into()),
                    general
                );
                let side = sidecar::Sidecar::read_xmp(sidecar::Sidecar::paths(&path).xmp).unwrap();
                assert_eq!(side.metadata().unwrap(), out.metadata().unwrap());
            }
        }
    }
}
