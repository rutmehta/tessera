use library::{Keyword, Library};
use ml_caption::{map_keyword, parse_ocr};

#[test]
fn ocr_quadrilaterals_are_normalized_and_malformed_output_is_rejected() {
    let regions = parse_ocr(
        "<s>SIGN<loc_100><loc_200><loc_300><loc_190><loc_310><loc_400><loc_90><loc_390></s>",
        0.8,
    )
    .unwrap();
    assert_eq!(regions.len(), 1);
    assert_eq!(regions[0].text, "SIGN");
    assert_eq!(regions[0].bbox, [0.0905, 0.1905, 0.3105, 0.4005]);
    assert!(parse_ocr("</s>", 0.5).unwrap().is_empty());
    for text in [
        "SIGN",
        "SIGN<loc_2>",
        "SIGN<loc_1000><loc_2><loc_2><loc_2><loc_2><loc_2><loc_2><loc_2>",
        "<loc_1><loc_1><loc_1><loc_1><loc_1><loc_1><loc_1><loc_1>",
    ] {
        assert!(parse_ocr(text, 0.5).is_err());
    }
    assert!(parse_ocr("", f32::NAN).is_err());
}
#[test]
fn ambiguous_synonyms_require_a_decision() {
    let library = Library {
        keywords: vec![
            Keyword {
                id: 1,
                name: "River bank".into(),
                synonyms: vec!["bank".into()],
                children: vec![],
            },
            Keyword {
                id: 2,
                name: "Bank building".into(),
                synonyms: vec!["bank".into()],
                children: vec![],
            },
        ],
        ..Default::default()
    };
    assert!(
        map_keyword(&library, "BANK")
            .unwrap_err()
            .to_string()
            .contains("ambiguous")
    );
}
