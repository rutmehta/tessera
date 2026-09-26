use psd::{ColorMode, Compression, ImageResource, LayerSection, PsdDocument, Version};

fn document(resources: Vec<ImageResource>) -> PsdDocument {
    PsdDocument {
        version: Version::Psd,
        width: 1,
        height: 1,
        depth: 8,
        channels: 4,
        color_mode: ColorMode::Rgb,
        color_data: vec![],
        resources,
        layer_section: LayerSection::default(),
        composite: vec![0; 4],
        compression: Compression::Raw,
    }
}

#[test]
fn external_legacy_names_are_unpadded_macroman_unicode_takes_precedence() {
    let mut d = document(vec![ImageResource::new(1006, vec![2, b'A', 0x8e, 1, b'B'])]);
    assert_eq!(d.alpha_names().unwrap().unwrap(), ["Aé", "B"]);
    // Length is UTF-16 code units, with no padding between names.
    d.resources.push(ImageResource::new(
        1045,
        vec![0, 0, 0, 1, 3, 0xbb, 0, 0, 0, 2, 0xd8, 0x3e, 0xdd, 0x80],
    ));
    assert_eq!(d.alpha_names().unwrap().unwrap(), ["λ", "🦀"]);
    let reread = PsdDocument::read(&d.write().unwrap()).unwrap();
    assert_eq!(reread.resources, d.resources);
}

#[test]
fn standard_name_writer_uses_1045_not_tagged_layer_unam() {
    let names = vec!["é".into(), "🦀".into()];
    let d = document(psd::resources::alpha_name_resources(&names).unwrap());
    assert_eq!(d.resource(1006).unwrap(), [1, 0x8e, 1, b'?']);
    assert_eq!(
        d.resource(1045).unwrap(),
        [0, 0, 0, 1, 0, 0xe9, 0, 0, 0, 2, 0xd8, 0x3e, 0xdd, 0x80]
    );
    assert_eq!(d.alpha_names().unwrap().unwrap(), names);
}

#[test]
fn external_display_records_have_distinct_legacy_and_modern_framing() {
    // RGB red ink, 75 percent solidity, spot mode 2.
    let record = vec![0, 0, 255, 255, 0, 0, 0, 0, 0, 0, 0, 75, 2];
    let mut legacy = record.clone();
    legacy.push(0);
    let mut modern = vec![0, 0, 0, 1];
    modern.extend_from_slice(&record);
    for (id, data) in [(1007, legacy), (1077, modern)] {
        let d = document(vec![ImageResource::new(id, data)]);
        let records = d.channel_display_info().unwrap().unwrap();
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].color, [65535, 0, 0, 0]);
        assert_eq!(records[0].opacity, 75);
        assert_eq!(records[0].mode, 2);
    }
}

#[test]
fn malformed_channel_resources_fail_without_panics() {
    for (id, data) in [
        (1006, vec![3, b'a']),
        (1045, vec![0, 0, 0, 1, 0xd8, 0]),
        (1045, vec![255, 255, 255, 255]),
        (1045, vec![0]),
        (1007, vec![0; 13]),
        (1077, vec![0, 0, 0, 2]),
        (1077, vec![0, 0, 0, 1, 0]),
        (1007, vec![0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 101, 2, 0]),
        (1007, vec![0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 50, 3, 0]),
        (1007, vec![0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 50, 2, 1]),
    ] {
        let d = document(vec![ImageResource::new(id, data)]);
        assert!(
            if matches!(id, 1006 | 1045) {
                d.alpha_names().is_err()
            } else {
                d.channel_display_info().is_err()
            },
            "{id}"
        );
    }
    for id in [1006, 1045, 1007, 1077] {
        let data = if id == 1077 { vec![0, 0, 0, 1] } else { vec![] };
        let d = document(vec![
            ImageResource::new(id, data.clone()),
            ImageResource::new(id, data),
        ]);
        assert!(if matches!(id, 1006 | 1045) {
            d.alpha_names().is_err()
        } else {
            d.channel_display_info().is_err()
        });
    }
}
