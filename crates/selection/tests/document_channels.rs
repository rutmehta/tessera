use compositor::{Depth, DocState, Document};
use engine_api::tile::Extent;
use selection::{Mask, channels};
#[test]
fn save_load_document_preserves_soft_samples_and_history() {
    let mut doc = Document::new(DocState::new(Extent::new(3, 1), Depth::U8));
    let mask = Mask::from_vec(3, 1, vec![0.0, 0.123456, 1.0]).unwrap();
    let id = channels::save(&mut doc, "soft", &mask).unwrap();
    assert_eq!(channels::load(doc.state(), id).unwrap(), mask);
    assert!(doc.undo());
    assert!(channels::load(doc.state(), id).is_err());
    assert!(doc.redo());
    assert_eq!(channels::load(doc.state(), id).unwrap(), mask);
}
