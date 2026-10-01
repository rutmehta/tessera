#[path = "common/lr4b_retained.rs"]
mod fixtures;
#[test]
fn lr4c_approximate_promotions_and_untranslated_byte_pins() {
    // LR-4c intentionally promotes only brush/color indices 0,1,7,8.
    // LR-4d changes only those four diagnostic envelopes to the shared channel.
    // Other values remain pinned to a88440a4; malformed/unknown forms do not change.
    let expected = [
        (
            9468,
            "b38d9b1f8cae792f235f949bade9f39b4831c2b6b9fa5bf576cf5d7549a645fb",
        ),
        (
            8458,
            "6a00aa810387599912de150354ccdfc462cf484f5309b25d04e7c3dff2f958fc",
        ),
        (
            6653,
            "23de468146a603a5f9243684885206db554e9bf710682cf244d9e963e6a450f0",
        ),
        (
            6589,
            "1cbfbfc12e35918b0ec3fdbbd387f105718773e17b9395d20cb1f9c00ac9fb65",
        ),
        (
            6559,
            "05af8f21013f5d97418985b9ff8ee1e9ada5f2d3060331d9c352aa6d8aa95401",
        ),
        (
            6451,
            "8c3422935d63e21231bb696c1e86880343d71b4565bb504b96bdfcab1e10c01f",
        ),
        (
            6499,
            "efeb750fcfd403097091448cca65d3db40c1e6e52e83c6d6c2e84f539abf79c4",
        ),
        (
            9278,
            "6dd154e9b5f2c0487da994788635defc1161d31876feb2f4220a7dc55cde298c",
        ),
        (
            9443,
            "5ebc245d5c5390a00efecc0a617898fbe95707271d1c4233ab90cdd5d6bcc463",
        ),
        (
            7271,
            "60e3026103584e777f0944e12bf1e722944f810d4145fd2128f6e3141962a7d8",
        ),
        (
            7232,
            "062f71e400c5a9864cdb477f77dac3dfbbf66f98954ef9cb26b9b30f36b87a82",
        ),
    ];
    let got = fixtures::digests();
    eprintln!("LR-4c audited fixture digests: {got:#?}");
    for ((size, digest), (want_size, want_digest)) in got.into_iter().zip(expected) {
        assert_eq!(size, want_size);
        assert_eq!(digest, want_digest);
    }
}
