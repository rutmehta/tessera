use engine_api::recipe::{Grade, Mark, Recipe, Selection};
use export::*;
use pipeline_cpu::{Image, RenderSource};
use std::fs;

#[test]
fn formats_profiles_and_privacy() {
    let image = fixture(32, 24);
    let recipe = Recipe::default();
    let packet = sidecar::XmpPacket::parse(r#"<x:xmpmeta xmlns:x="adobe:ns:meta/"><rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#"><rdf:Description xmlns:dc="http://purl.org/dc/elements/1.1/" xmlns:exif="http://ns.adobe.com/exif/1.0/" dc:rights="Copyright Alice" exif:GPSLatitude="secret"/></rdf:RDF></x:xmpmeta>"#).unwrap();
    for format in [
        Format::Jpeg { quality: 90 },
        Format::Png,
        Format::Tiff { bits: 8 },
        Format::Tiff { bits: 16 },
    ] {
        for color_space in [
            ColorSpace::Srgb,
            ColorSpace::DisplayP3,
            ColorSpace::Rec2020,
            ColorSpace::ProPhoto,
        ] {
            for metadata in [Metadata::All, Metadata::CopyrightOnly, Metadata::None] {
                let dir = tempfile::tempdir().unwrap();
                let settings = ExportSettings {
                    format,
                    color_space,
                    metadata,
                    output_dir: dir.path().into(),
                    ..Default::default()
                };
                let mut source = input(&image);
                source.metadata = Some(&packet);
                let path = export_one(&source, &recipe, &settings).unwrap();
                let (icc, xmp) = match format {
                    Format::Tiff { bits } => {
                        let mut decoder =
                            tiff::decoder::Decoder::new(fs::File::open(&path).unwrap()).unwrap();
                        assert_eq!(decoder.colortype().unwrap(), tiff::ColorType::RGB(bits));
                        assert_eq!(decoder.dimensions().unwrap(), (32, 24));
                        let icc = decoder
                            .get_tag_u8_vec(tiff::tags::Tag::Unknown(34675))
                            .unwrap();
                        let xmp = decoder.get_tag_u8_vec(tiff::tags::Tag::Unknown(700)).ok();
                        assert!(decoder.read_image().is_ok());
                        (icc, xmp)
                    }
                    Format::Png => {
                        let mut decoder = png::Decoder::new(fs::File::open(&path).unwrap())
                            .read_info()
                            .unwrap();
                        let mut buf = vec![0; decoder.output_buffer_size()];
                        decoder.next_frame(&mut buf).unwrap();
                        (
                            decoder.info().icc_profile.as_ref().unwrap().to_vec(),
                            decoder
                                .info()
                                .utf8_text
                                .first()
                                .map(|t| t.get_text().unwrap().into_bytes()),
                        )
                    }
                    Format::Jpeg { .. } => {
                        use image::ImageDecoder;
                        let mut decoder = image::codecs::jpeg::JpegDecoder::new(
                            std::io::BufReader::new(fs::File::open(&path).unwrap()),
                        )
                        .unwrap();
                        (
                            decoder.icc_profile().unwrap().unwrap(),
                            decoder.xmp_metadata().unwrap(),
                        )
                    }
                };
                assert!(lcms2::Profile::new_icc(&icc).is_ok());
                let side_path = sidecar::Sidecar::paths(&path).xmp;
                if matches!(metadata, Metadata::None) {
                    assert!(xmp.is_none());
                    assert!(!side_path.exists());
                } else {
                    let text = String::from_utf8(xmp.unwrap()).unwrap();
                    assert!(text.contains("Copyright Alice"));
                    assert_eq!(text.contains("secret"), matches!(metadata, Metadata::All));
                    assert_eq!(fs::read_to_string(side_path).unwrap(), text);
                }
            }
        }
    }
}

fn fixture(w: u32, h: u32) -> Image {
    Image::new(
        w,
        h,
        (0..3)
            .map(|c| {
                (0..w * h)
                    .map(|i| ((i % w) as f32 / w as f32 + c as f32 * 0.1) * 0.4)
                    .collect()
            })
            .collect(),
    )
    .unwrap()
}
fn input(image: &Image) -> ExportImage<'_> {
    ExportImage {
        source: RenderSource::Rgb(image),
        name: "fixture",
        sequence: 1,
        date: "20260925",
        metadata: None,
    }
}
#[test]
fn jpeg_roundtrip_icc_and_selection_sidecar() {
    let dir = tempfile::tempdir().unwrap();
    let image = fixture(80, 60);
    let mut recipe = Recipe {
        selection: Selection::keep(Some(Grade::Three)),
        ..Default::default()
    };
    recipe.selection.mark = Some(Mark::new("Red"));
    let settings = ExportSettings {
        output_dir: dir.path().into(),
        ..Default::default()
    };
    let path = export_one(&input(&image), &recipe, &settings).unwrap();
    assert_eq!(image::open(&path).unwrap().to_rgb8().dimensions(), (80, 60));
    let bytes = fs::read(&path).unwrap();
    assert!(bytes.windows(12).any(|b| b == b"ICC_PROFILE\0"));
    assert!(
        bytes
            .windows(b"http://ns.adobe.com/xap/1.0/\0".len())
            .any(|b| b == b"http://ns.adobe.com/xap/1.0/\0")
    );
    let packet = sidecar::Sidecar::read_xmp(sidecar::Sidecar::paths(&path).xmp).unwrap();
    assert_eq!(packet.selection().unwrap(), recipe.selection);
}
