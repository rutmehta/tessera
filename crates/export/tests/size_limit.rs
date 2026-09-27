use engine_api::recipe::Recipe;
use export::{ExportImage, ExportSettings, Format, Metadata, export_one};
use pipeline_cpu::{Image, RenderSource};

#[test]
fn jpeg_budget_includes_profile_and_metadata_and_never_publishes_oversize() {
    let dir = tempfile::tempdir().unwrap();
    let samples = (0..64 * 64)
        .map(|i| ((i * 47 % 251) as f32) / 251.0)
        .collect::<Vec<_>>();
    let source = Image::new(64, 64, vec![samples; 3]).unwrap();
    let packet = sidecar::XmpPacket::parse(r#"<x:xmpmeta xmlns:x="adobe:ns:meta/"><rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#"><rdf:Description xmlns:dc="http://purl.org/dc/elements/1.1/" dc:rights="Copyright test"/></rdf:RDF></x:xmpmeta>"#).unwrap();
    let image = ExportImage {
        source: RenderSource::Rgb(&source),
        name: "budget",
        sequence: 1,
        date: "",
        metadata: Some(&packet),
    };
    let mut settings = ExportSettings {
        format: Format::Jpeg { quality: 100 },
        output_dir: dir.path().into(),
        metadata: Metadata::All,
        ..Default::default()
    };
    let full = export_one(&image, &Recipe::default(), &settings).unwrap();
    let full_size = std::fs::metadata(full).unwrap().len();
    settings.naming = "limited".into();
    settings.max_file_bytes = Some(full_size * 3 / 4);
    let limited = export_one(&image, &Recipe::default(), &settings).unwrap();
    assert!(std::fs::metadata(&limited).unwrap().len() <= full_size * 3 / 4);
    assert_eq!(image::open(&limited).unwrap().width(), 64);
    use image::ImageDecoder;
    let mut decoder = image::codecs::jpeg::JpegDecoder::new(std::io::BufReader::new(
        std::fs::File::open(&limited).unwrap(),
    ))
    .unwrap();
    assert!(lcms2::Profile::new_icc(&decoder.icc_profile().unwrap().unwrap()).is_ok());
    let xmp = String::from_utf8(decoder.xmp_metadata().unwrap().unwrap()).unwrap();
    assert!(xmp.contains("Copyright test"));
    assert_eq!(
        std::fs::read_to_string(sidecar::Sidecar::paths(&limited).xmp).unwrap(),
        xmp
    );
    settings.naming = "impossible".into();
    settings.max_file_bytes = Some(1);
    assert!(export_one(&image, &Recipe::default(), &settings).is_err());
    assert!(!dir.path().join("impossible.jpg").exists());
    assert!(!dir.path().join("impossible.jpg.xmp").exists());
    settings.format = Format::Png;
    assert!(export_one(&image, &Recipe::default(), &settings).is_err());
}
