#[path = "common/lr4_compat.rs"]
mod fixtures;
#[test]
fn legacy_44_synthetic_groups_remain_byte_identical() {
    let (digest, bytes) = fixtures::digest();
    // Measured by running the identical generator at unmodified 87ff1ff1.
    assert_eq!(bytes, 81_809);
    assert_eq!(
        digest,
        "aec3a2eb9f1a31a1596d063b27ba785ae1d55219ba1931745c79a7cf9d8043cb"
    );
}
