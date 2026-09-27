use engine_api::recipe::Recipe;
use export::{ExportImage, ExportSettings, Format, Metadata, export_one};
use pipeline_cpu::{Image, RenderSource};
use sidecar::XmpPacket;
use std::{fs, io::BufReader};

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
                assert_eq!(
                    text.contains("baked-edit"),
                    matches!(policy, Metadata::All) && !matches!(format, Format::Dng)
                );
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
