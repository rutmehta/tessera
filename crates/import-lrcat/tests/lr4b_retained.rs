#[path = "common/lr4b_retained.rs"]
mod fixtures;
#[test]
fn lr4c_approximate_promotions_and_untranslated_byte_pins() {
    // LR-4c intentionally promotes only brush/color indices 0,1,7,8.
    // LR-4d changes only those four diagnostic envelopes to the shared channel.
    // Other values remain pinned to a88440a4; malformed/unknown forms do not change.
    // ENG-7b: every case +2 bytes, "remove_chromatic_aberration": true -> false
    // in the history base and settings (Remove CA defaults to off). Was a88440a4-era:
    // 9468/b38d9b1f, 8458/6a00aa81, 6653/23de4681, 6589/1cbfbfc1, 6559/05af8f21, 6451/8c342293, 6499/efeb750f, 9278/6dd154e9, 9443/5ebc245d, 7271/60e30261, 7232/062f71e4
    let expected = [
        (
            9470,
            "546154d6c339cbb43da60119141fb95af5feedf212f5edbcea9e908629a843cd",
        ),
        (
            8460,
            "9009924d801f073317427303242cb5b65b6fcef36160d4c79f308f89601a356d",
        ),
        (
            6655,
            "2c2f21028ac055b57ba06f6ae5cea689a66fa3b27ebd5c7090c1627926fd07b3",
        ),
        (
            6591,
            "1119195bef3f7f55d99e5032fa3fa1f24b03d29633fe6d655ebc991bc2cf771a",
        ),
        (
            6561,
            "52d56addc99ed75e36be8c950bd3c12701c6289e6e1bbfb6e4a32cde5a5c5892",
        ),
        (
            6453,
            "c7ccc1373573e06fa2136b5094c55bf50b77d0f031e655925c98153585e0fb48",
        ),
        (
            6501,
            "74878558407a67c8ab3c70766f810faa1bfb84d4a7365a5f74d5a7cf2656e164",
        ),
        (
            9280,
            "4d39bf2edb429a8427cfb0b012049237424ef654b8aeeb8210144c9992bfc090",
        ),
        (
            9445,
            "4fd68b1f5b116cdea78c689ed8cd956da30245f3e41d7d3488e502a49453f007",
        ),
        (
            7273,
            "45c1a0ecad4aba7ab8fc3f90863dc7b4090f1bc8909d3d3060298e7953a3e887",
        ),
        (
            7234,
            "a7c25f94c375693f24ced4d99a953391216be20805331bf01453f169bbb6dd7c",
        ),
    ];
    let got = fixtures::digests();
    eprintln!("LR-4c audited fixture digests: {got:#?}");
    for ((size, digest), (want_size, want_digest)) in got.into_iter().zip(expected) {
        assert_eq!(size, want_size);
        assert_eq!(digest, want_digest);
    }
}
