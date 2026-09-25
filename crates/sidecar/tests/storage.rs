use engine_api::recipe::{Decision, Recipe};
use sidecar::{MarkPreset, RecipeDocument, Sidecar, WriteStamp, XmpPacket};
use std::collections::BTreeMap;
#[test]
fn lww_uses_write_time_not_vector_sum_and_converges() {
    let a = RecipeDocument {
        recipe: Recipe::default(),
        vector_clock: BTreeMap::from([("a".into(), 100)]),
        last_writer: WriteStamp {
            timestamp_ms: 10,
            machine_id: "a".into(),
            counter: 100,
        },
    };
    let mut b = RecipeDocument {
        recipe: Recipe::default(),
        vector_clock: BTreeMap::from([("b".into(), 1)]),
        last_writer: WriteStamp {
            timestamp_ms: 20,
            machine_id: "b".into(),
            counter: 1,
        },
    };
    b.recipe.selection.decision = Decision::Keep;
    let merged = Sidecar::merge_last_writer_wins(&a, &b).unwrap();
    assert_eq!(merged.recipe, b.recipe);
    assert_eq!(
        merged.vector_clock,
        BTreeMap::from([("a".into(), 100), ("b".into(), 1)])
    );
    assert_eq!(merged, Sidecar::merge_last_writer_wins(&b, &a).unwrap());
    assert_eq!(
        merged,
        Sidecar::merge_last_writer_wins(&merged, &a).unwrap()
    );
    assert_eq!(
        merged,
        Sidecar::merge_last_writer_wins(&merged, &merged).unwrap()
    );
    b.record_write("b", 15).unwrap();
    assert!(b.last_writer.timestamp_ms > 20);
    assert_eq!(b.vector_clock["b"], 2);
}
#[test]
fn xmp_disk_write_is_atomic_and_readable() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/.scratch-xmp");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("photo.ARW.xmp");
    let packet = XmpPacket::from_selection(&Default::default(), &MarkPreset::lightroom());
    Sidecar::write_xmp(&path, &packet).unwrap();
    assert_eq!(Sidecar::read_xmp(&path).unwrap(), packet);
    let bad = XmpPacket {
        xml: "<broken>".into(),
    };
    assert!(Sidecar::write_xmp(&path, &bad).is_err());
    assert_eq!(Sidecar::read_xmp(&path).unwrap(), packet);
    let blocked = dir.join("blocked");
    std::fs::create_dir(&blocked).unwrap();
    assert!(Sidecar::write_xmp(&blocked, &packet).is_err());
    assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 2);
    std::fs::remove_dir_all(dir).unwrap();
}
