use psd::metadata::{Descriptor, Text, Value};
use typography::*;
#[test]
fn adobe_engine_edits_preserve_unknown_and_character_paragraph_fields() {
    let data = b"<< /Unknown << /Token /Opaque /Enabled true >> /EngineDict << /StyleRun << /RunLengthArray [ 3 ] /RunArray [ << /StyleSheet << /StyleSheetData << /Font 0 /FontSize 18 /Tracking 100 /Leading 23 /BaselineShift 2 /FauxItalic true /FillColor << /Type 1 /Values [ 1 0.2 0.4 0.6 ] >> /Future 77 >> >> >> ] >> /ParagraphRun << /RunLengthArray [ 3 ] /RunArray [ << /ParagraphSheet << /Properties << /Justification 2 >> >> >> ] >> >> /ResourceDict << /FontSet [ << /Name (NotoSans-Regular) >> ] >> >>";
    let empty = Descriptor {
        name: String::new(),
        class_id: b"null",
        items: vec![],
    };
    let source = Text {
        transform: [1., 0., 0., 1., 0., 0.],
        bounds: [0., 0., 100., 40.],
        descriptor: Descriptor {
            items: vec![
                (b"Txt ", Value::Text("abc".into())),
                (b"EngineData", Value::Raw(data)),
            ],
            ..empty.clone()
        },
        warp: empty,
    };
    let mut model = import_tysh(&source).unwrap().model;
    assert_eq!(model.runs[0].tracking, 1.8);
    assert_eq!(model.runs[0].color, [51, 102, 153, 255]);
    assert_eq!(model.paragraph.alignment, Alignment::Right);
    model.runs[0].text = "😀z".into();
    let edited = export_engine_data(&model, Some(data)).unwrap();
    let root = psd::metadata::parse_engine_data(&edited).unwrap();
    assert!(root.get(b"Unknown").is_some());
    assert!(String::from_utf8_lossy(&edited).contains("/Future 77"));
    assert!(String::from_utf8_lossy(&edited).contains("/RunLengthArray [ 3"));
}
