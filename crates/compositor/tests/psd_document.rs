use compositor::{Depth, DocState, Document};
use engine_api::tile::Extent;

#[test]
fn document_psd_api_roundtrip() {
    let native = Document::new(DocState::new(Extent::new(2, 3), Depth::U8));
    let psd = compositor::psd::to_psd(&native).unwrap();
    let doc = Document::from_psd(psd).unwrap();
    assert_eq!(doc.state().canvas, native.state().canvas);
    assert_eq!(compositor::psd::to_psd(&doc.clone()).unwrap().width, 2);
}
