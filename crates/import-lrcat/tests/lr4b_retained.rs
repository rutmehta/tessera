#[path = "common/lr4b_retained.rs"]
mod fixtures;
#[test]
fn lr4b_untranslated_recipes_remain_byte_identical_to_a88440a4() {
    // Captured on a detached a88440a4, before this implementation. Never repin.
    let expected = [
        (
            6788,
            "526f514ac420bcc431d51d7344e00e98a7b851f1ecd4bc6457f4cfbfda4e1e6c",
        ),
        (
            6726,
            "39762cf23901076eef0c0f4277fcd1da81683d571eb1e23aa7f5068ef6f45daf",
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
            7400,
            "ad11b1ce578c71b292ac7d27e274df906caf4d9aa39c790f2771ce5e80e63893",
        ),
        (
            7667,
            "225f8eb30213fdb941022bc8565c0177665eb2eb3774e9d4138ad7ae9575bbed",
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
    for ((size, digest), (want_size, want_digest)) in fixtures::digests().into_iter().zip(expected)
    {
        assert_eq!(size, want_size);
        assert_eq!(digest, want_digest);
    }
}
