use psd::PsdDocument;
#[test]
fn upstream_photoshop_fixtures() {
    for (name, data) in [
        ("green", include_bytes!("fixtures/green-1x1.psd").as_slice()),
        (
            "groups",
            include_bytes!("fixtures/green-1x1-one-group-inside-another.psd").as_slice(),
        ),
        (
            "rle",
            include_bytes!("fixtures/rle-3-layer-8x8.psd").as_slice(),
        ),
        (
            "unicode",
            include_bytes!("fixtures/green-chinese-layer-name-1x1.psd").as_slice(),
        ),
        (
            "empty",
            include_bytes!("fixtures/rle-compressed-empty-channel.psd").as_slice(),
        ),
    ] {
        let d = PsdDocument::read(data).unwrap_or_else(|e| panic!("{name}: {e}"));
        let reloaded = PsdDocument::read(&d.write().unwrap()).unwrap();
        assert_eq!(d, reloaded, "{name}");
        if name == "green" {
            assert_eq!((d.width, d.height), (1, 1));
            assert_eq!(&d.composite[..3], &[0, 255, 0]);
        }
    }
}
