use psd::metadata::{Descriptor, Text, Value};
use typography::*;
#[test]
fn tysh_engine_styles_use_utf16_lengths_and_preserve_original() {
    let bytes = b"<< /EngineDict << /StyleRun << /RunLengthArray [ 2 1 ] /RunArray [ << /StyleSheet << /StyleSheetData << /Font 0 /FontSize 18 >> >> >> << /StyleSheet << /StyleSheetData << /Font 1 /FontSize 24 >> >> >> ] >> >> /ResourceDict << /FontSet [ << /Name (NotoSans-Regular) >> << /Name (Other) >> ] >> >>";
    let empty = Descriptor {
        name: String::new(),
        class_id: b"",
        items: vec![],
    };
    let tysh = Text {
        transform: [1.0, 0.0, 0.0, 1.0, 5.0, 7.0],
        bounds: [0.0, 0.0, 100.0, 50.0],
        warp: empty.clone(),
        descriptor: Descriptor {
            items: vec![
                (b"Txt ", Value::Text("😀x".into())),
                (b"EngineData", Value::Raw(bytes)),
            ],
            ..empty
        },
    };
    let imported = import_tysh(&tysh).unwrap();
    assert_eq!(imported.model.runs.len(), 2);
    assert_eq!(imported.model.runs[0].text, "😀");
    assert_eq!(imported.model.runs[0].family, "NotoSans-Regular");
    assert_eq!(imported.model.runs[1].text, "x");
    assert_eq!(imported.model.runs[1].size, 24.0);
    assert_eq!(
        imported.original_engine_data.as_deref(),
        Some(bytes.as_slice())
    );
    assert_eq!(imported.transform, tysh.transform);
}
