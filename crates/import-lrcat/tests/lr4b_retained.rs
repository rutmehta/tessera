#[path = "common/lr4b_retained.rs"]
mod fixtures;
#[test]
fn lr4c_approximate_promotions_and_untranslated_byte_pins() {
    // LR-4c intentionally promotes only brush/color indices 0,1,7,8.
    // Other values remain pinned to a88440a4; malformed/unknown forms do not change.
    let expected = [
        (
            8715,
            "817d3beffd7daded3d41f8f9063d5cebb9eca83f6529b6f8491bff6d61bb9cbb",
        ),
        (
            8332,
            "3eb9af24b3a9ce579ef30009696f104e68b85bb4596be89796c805d1c88bd506",
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
            8785,
            "6c0d7c8610e5c5468fb7fcae0262e7b7ad908d9145696beac15a1cb98333f225",
        ),
        (
            9317,
            "09f355b0d9730ef395a047e093704a1a9737bc2022d09b8235278dbd7849b74b",
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
