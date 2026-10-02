#[allow(dead_code)]
#[path = "../../raw-decode/tests/support/mod.rs"]
mod support;

#[test]
fn camera_dng_refuses_unconsumed_opcodes_instead_of_dropping_corrections() {
    let mut dng = raw_decode::lossy_dng::read(&mut std::io::Cursor::new(support::lossy_dng(false,false))).unwrap().unwrap();
    // Valid optional unknown opcode. It may be ignored by a generic parser,
    // but this cropped source has not executed its pixel-correction lists.
    dng.metadata.opcode_lists[1] = Some([1u32,999,0x01030000,1,0].into_iter().flat_map(u32::to_be_bytes).collect());
    assert!(pipeline_cpu::CameraLinearProxy::from_dng(dng).is_err());
}
