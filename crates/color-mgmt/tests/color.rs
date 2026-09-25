use color_mgmt::*;
use std::sync::Arc;

#[test]
fn builtin_profiles_have_expected_primaries_and_curves() {
    use lcms2::{Tag, TagSignature};
    let mut registry = Registry::new();
    let cases = [
        (Builtin::DisplayP3, 0.68, 0.214041),
        (Builtin::AdobeRgb, 0.64, 0.217756),
        (Builtin::ProPhoto, 0.7347, 0.287175),
        (Builtin::Rec2020, 0.708, 0.259719),
        (Builtin::LinearRec2020, 0.708, 0.5),
    ];
    let mut digests = std::collections::HashSet::new();
    for (builtin, _red_x, half_linear) in cases {
        let profile = registry.builtin(builtin).unwrap();
        assert!(digests.insert(profile.digest()));
        let icc = lcms2::Profile::new_icc(profile.icc_bytes()).unwrap();
        assert_eq!(icc.color_space(), lcms2::ColorSpaceSignature::RgbData);
        if let Tag::ToneCurve(curve) = icc.read_tag(TagSignature::RedTRCTag) {
            assert!(
                (curve.eval::<f32>(0.5) - half_linear).abs() < 0.001,
                "{builtin:?}"
            );
        } else {
            panic!("missing TRC");
        }
    }
}

#[test]
fn srgb_registry_deduplicates_bytes_and_files() {
    let mut registry = Registry::new();
    let srgb = registry.builtin(Builtin::Srgb).unwrap();
    assert!(Arc::ptr_eq(
        &srgb,
        &registry.builtin(Builtin::Srgb).unwrap()
    ));
    assert!(Arc::ptr_eq(
        &srgb,
        &registry.load_bytes(srgb.icc_bytes()).unwrap()
    ));
    let file = tempfile::NamedTempFile::new().unwrap();
    std::fs::write(file.path(), srgb.icc_bytes()).unwrap();
    assert!(Arc::ptr_eq(
        &srgb,
        &registry.load_file(file.path()).unwrap()
    ));
    assert_eq!(srgb.digest(), *blake3::hash(srgb.icc_bytes()).as_bytes());
    assert!(registry.load_bytes(b"not ICC").is_err());
}
