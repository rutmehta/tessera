#[path = "common/lr4_compat.rs"]
mod fixtures;
#[test]
fn legacy_44_synthetic_groups_remain_byte_identical() {
    let (digest, bytes) = fixtures::digest();
    // Measured by running the identical generator at unmodified 87ff1ff1.
    // ENG-7b (was 81_809 / aec3a2eb…43cb): Remove CA defaults to off (+2 bytes).
    assert_eq!(bytes, 81_811);
    assert_eq!(
        digest,
        "9eff2648d550cf3e88732177eef097fed5386a8e2909deb7d32822dff5e84aa7"
    );
}
