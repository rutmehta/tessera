//! `text:` (and bare words) match generated captions and text found in the
//! image (OCR), not only names, keywords and imported captions (M3-15).
use index::{Index, NoopMetadataProvider, NoopSidecarReader, OcrRegion, Query, Understanding};
use library::SavedSearch;

#[test]
fn text_terms_match_generated_captions_and_ocr() {
    let dir = tempfile::tempdir().unwrap();
    for name in ["sign.cr3", "street.cr3"] {
        std::fs::write(dir.path().join(name), name).unwrap();
    }
    let mut index = Index::open(dir.path().join("index.sqlite")).unwrap();
    index
        .scan(dir.path(), &NoopSidecarReader, &NoopMetadataProvider)
        .unwrap();
    let ids = index.search(&Query::default()).unwrap();
    let sign = ids[0];
    index
        .set_understanding(
            sign,
            &Understanding {
                model_version: "test".into(),
                keywords: vec![],
                caption: "A painted wooden board.".into(),
                alt_text: String::new(),
                ocr: vec![OcrRegion {
                    text: "EXIT ONLY".into(),
                    bbox: [0.1, 0.1, 0.4, 0.2],
                    confidence: 0.9,
                }],
            },
        )
        .unwrap();
    let find = |text: &str| {
        let query = text.parse::<SavedSearch>().unwrap().compile().unwrap();
        index.search(&query).unwrap()
    };
    assert_eq!(find(r#"text:"exit only""#), vec![sign]);
    assert_eq!(find("exit"), vec![sign]);
    assert_eq!(find("text:wooden"), vec![sign]);
    assert_eq!(find(r#"text:"only exit""#), vec![]);
    assert_eq!(find("NOT text:exit").len(), ids.len() - 1);
    // Quotes inside the value are literal (JSON-escaped in the grammar).
    assert_eq!(find(r#"text:"say \"exit\"""#), vec![]);
}
