use engine_api::recipe::Recipe;
use export::*;
use pipeline_cpu::{Image, RenderSource};
use std::fs;

#[test]
fn invalid_formats_existing_files_and_encoder_errors_are_atomic() {
    let image = Image::new(32, 24, vec![vec![0.2; 768]; 3]).unwrap();
    let mut source = ExportImage {
        source: RenderSource::Rgb(&image),
        name: "photo",
        sequence: 1,
        date: "",
        metadata: None,
    };
    let recipe = Recipe::default();
    for format in [
        Format::Tiff { bits: 12 },
        Format::Jpeg { quality: 0 },
        Format::Jpeg { quality: 101 },
    ] {
        let dir = tempfile::tempdir().unwrap();
        let settings = ExportSettings {
            format,
            output_dir: dir.path().into(),
            ..Default::default()
        };
        assert!(export_one(&source, &recipe, &settings).is_err());
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 0);
    }
    let dir = tempfile::tempdir().unwrap();
    let settings = ExportSettings {
        output_dir: dir.path().into(),
        ..Default::default()
    };
    let path = export_one(&source, &recipe, &settings).unwrap();
    let before = fs::read(&path).unwrap();
    assert!(export_one(&source, &recipe, &settings).is_err());
    assert_eq!(fs::read(path).unwrap(), before);
    assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 2);

    let dir = tempfile::tempdir().unwrap();
    let settings = ExportSettings {
        output_dir: dir.path().into(),
        ..settings
    };
    // Standard APP1 cannot contain this packet. Fail rather than truncate metadata.
    let packet =
        sidecar::XmpPacket::from_selection(&Default::default(), &sidecar::MarkPreset::lightroom())
            .with_metadata(
                &Default::default(),
                &sidecar::Metadata {
                    description: "x".repeat(70_000),
                    ..Default::default()
                },
                &sidecar::MarkPreset::lightroom(),
            )
            .unwrap();
    source.metadata = Some(&packet);
    assert!(export_one(&source, &recipe, &settings).is_err());
    assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 0);
}
