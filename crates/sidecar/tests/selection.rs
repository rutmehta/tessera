use engine_api::recipe::{Decision, Grade, Mark, Selection};
use sidecar::{MarkPreset, XmpPacket};

#[test]
fn selection_uses_dynamic_media_flags_not_keywords() {
    let preset = MarkPreset::lightroom();
    for decision in [Decision::Reject, Decision::Undecided, Decision::Keep] {
        for grade in [None, Some(Grade::One), Some(Grade::Two), Some(Grade::Three)] {
            if decision != Decision::Keep && grade.is_some() {
                continue;
            }
            for mark in [
                None,
                Some(Mark::new("Red")),
                Some(Mark::new("needs & review <now>")),
            ] {
                let selection = Selection {
                    decision,
                    grade,
                    mark,
                };
                let packet = XmpPacket::from_selection(&selection, &preset);
                assert!(!packet.serialize().contains("hierarchicalSubject"));
                assert_eq!(packet.selection().unwrap(), selection);
                if decision == Decision::Undecided {
                    assert!(!packet.serialize().contains("xmp:Rating"));
                } else {
                    assert!(packet.serialize().contains("xmpDM:pick"));
                }
            }
        }
    }
}
