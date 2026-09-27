use export::{ExportImage, ExportSettings, Format, Metadata, export_one};
use pipeline_cpu::{Image, RenderSource};
#[path = "support/dng_tags.rs"]
mod dng_tags;

#[test]
fn developed_float_dng_matches_linear_render() {
    let source = Image::new(32, 24, vec![vec![0.2; 32 * 24]; 3]).unwrap();
    let input = ExportImage {
        source: RenderSource::Rgb(&source),
        name: "developed",
        sequence: 1,
        date: "",
        metadata: None,
    };
    let mut recipe = engine_api::recipe::Recipe::default();
    recipe
        .edit(engine_api::recipe::EditMeta::user("exposure", 0), |s| {
            s.tone.exposure = 1.0;
        })
        .unwrap();
    let expected =
        pipeline_cpu::render_output_linear_scaled(&recipe.settings, &input.source, 1).unwrap();
    let dir = tempfile::tempdir().unwrap();
    let path = export_one(
        &input,
        &recipe,
        &ExportSettings {
            format: Format::Dng,
            metadata: Metadata::None,
            output_dir: dir.path().into(),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(path.extension().unwrap(), "dng");
    let tags = dng_tags::read(&path);
    assert_eq!(
        tags[&50706].2,
        [1, 6, 0, 0],
        "developed exports declare DNG 1.6"
    );
    // Audit the complete minimal DNG 1.6 LinearRaw tag set, not just the
    // version byte. Optional crop/scale/black/exposure tags use spec defaults.
    let expected_tags = [
        (254, 4, 1),
        (256, 4, 1),
        (257, 4, 1),
        (258, 3, 3),
        (259, 3, 1),
        (262, 3, 1),
        (273, 4, 1),
        (274, 3, 1),
        (277, 3, 1),
        (278, 4, 1),
        (279, 4, 1),
        (284, 3, 1),
        (339, 3, 3),
        (50706, 1, 4),
        (50707, 1, 4),
        (50708, 2, 15),
        (50717, 4, 3),
        (50721, 10, 9),
        (50728, 5, 3),
        (50778, 3, 1),
    ];
    assert_eq!(
        tags.keys().copied().collect::<Vec<_>>(),
        expected_tags.map(|t| t.0)
    );
    for (tag, kind, count) in expected_tags {
        assert_eq!((tags[&tag].0, tags[&tag].1), (kind, count), "tag {tag}");
    }
    assert_eq!(tags[&50707].2, [1, 4, 0, 0]);
    assert_eq!(tags[&262].2, 34892u16.to_le_bytes());
    assert_eq!(tags[&258].2, [32u16.to_le_bytes(); 3].concat());
    assert_eq!(tags[&339].2, [3u16.to_le_bytes(); 3].concat());
    assert_eq!(tags[&50717].2, [1u32.to_le_bytes(); 3].concat());
    assert_eq!(tags[&50778].2, 21u16.to_le_bytes());
    let decoded = raw_decode::linear_dng::read(&mut std::fs::File::open(&path).unwrap()).unwrap();
    assert_eq!((decoded.width, decoded.height), (32, 24));
    assert_eq!(decoded.pixels.as_flattened(), expected.as_raw());
    assert!(decoded.xmp.is_empty());
    assert!(!sidecar::Sidecar::paths(&path).xmp.exists());
    let mut raw = libraw_ffi::RawFile::open(&path).unwrap();
    raw.unpack().unwrap();

    let packet = sidecar::XmpPacket::parse(r#"<x:xmpmeta xmlns:x="adobe:ns:meta/"><rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#"><rdf:Description xmlns:dc="http://purl.org/dc/elements/1.1/" xmlns:crs="http://ns.adobe.com/camera-raw-settings/1.0/" dc:rights="Copyright Alice" crs:Exposure2012="1.0"/></rdf:RDF></x:xmpmeta>"#).unwrap();
    let tagged = ExportImage {
        metadata: Some(&packet),
        ..input
    };
    let path = export_one(
        &tagged,
        &recipe,
        &ExportSettings {
            format: Format::Dng,
            naming: "tagged".into(),
            output_dir: dir.path().into(),
            ..Default::default()
        },
    )
    .unwrap();
    let decoded = raw_decode::linear_dng::read(&mut std::fs::File::open(path).unwrap()).unwrap();
    assert!(decoded.xmp.contains("Copyright Alice"));
    assert!(
        !decoded.xmp.contains("Exposure2012"),
        "do not reapply baked edits"
    );

    let mark = export::Watermark::Graphic {
        path: "missing.png".into(),
        scale: 0.2,
        opacity: 1.0,
        anchor: export::Anchor::BottomRight,
        inset: 0.0,
    };
    let error = export_one(
        &tagged,
        &recipe,
        &ExportSettings {
            format: Format::Dng,
            watermark: Some(mark),
            naming: "marked".into(),
            output_dir: dir.path().into(),
            ..Default::default()
        },
    )
    .unwrap_err();
    assert!(error.to_string().contains("DNG watermark"));
}
