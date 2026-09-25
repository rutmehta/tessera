use color_mgmt::*;
use std::path::PathBuf;

fn output_profile(description: &str) -> Vec<u8> {
    use lcms2::*;
    let xy = |x, y| CIExyY { x, y, Y: 1.0 };
    let curve = ToneCurve::new(2.2);
    let mut p = lcms2::Profile::new_rgb(
        &xy(0.3457, 0.3585),
        &CIExyYTRIPLE {
            Red: xy(0.48, 0.34),
            Green: xy(0.30, 0.48),
            Blue: xy(0.23, 0.20),
        },
        &[&curve, &curve, &curve],
    )
    .unwrap();
    p.set_device_class(ProfileClassSignature::OutputClass);
    let mut mlu = MLU::new(1);
    mlu.set_text(description, Locale::none());
    p.write_tag(TagSignature::ProfileDescriptionTag, Tag::MLU(&mlu));
    p.icc().unwrap()
}

#[test]
fn scans_output_profiles_only_once_and_by_name() {
    let dir = tempfile::tempdir().unwrap();
    let nested = dir.path().join("Vendor/Printer.bundle/Contents/Resources");
    std::fs::create_dir_all(&nested).unwrap();
    std::fs::write(nested.join("glossy.icc"), output_profile("Zeta Glossy")).unwrap();
    std::fs::write(dir.path().join("matte.ICM"), output_profile("Alpha Matte")).unwrap();
    // Same bytes under another name are one profile.
    std::fs::write(dir.path().join("copy.icc"), output_profile("Alpha Matte")).unwrap();
    // Display class and junk are skipped.
    std::fs::write(
        dir.path().join("display.icc"),
        lcms2::Profile::new_srgb().icc().unwrap(),
    )
    .unwrap();
    std::fs::write(dir.path().join("junk.icc"), b"not a profile").unwrap();
    std::fs::write(dir.path().join("notes.txt"), output_profile("Ignored")).unwrap();
    let found = output_profiles(&[dir.path().to_path_buf(), PathBuf::from("/nonexistent")]);
    let names: Vec<_> = found.iter().map(|p| p.name.as_str()).collect();
    assert_eq!(names, ["Alpha Matte", "Zeta Glossy"]);
    assert!(found.iter().all(|p| p.color_space == "RGB"));
    assert!(found[1].path.ends_with("glossy.icc"));
}

#[test]
fn converts_to_rgb_and_cmyk_device_space() {
    let mut r = Registry::new();
    let srgb = r.builtin(Builtin::Srgb).unwrap();
    let printer = r.load_bytes(&output_profile("Test")).unwrap();
    let out = convert_rgb(
        &srgb,
        &printer,
        Intent::RelativeColorimetric,
        true,
        &[[1.0; 3], [0.0; 3], [1.0, 0.0, 0.0]],
    )
    .unwrap();
    assert_eq!(out.channels, 3);
    assert_eq!(out.data.len(), 9);
    assert!(out.data[..3].iter().all(|v| *v >= 250), "{:?}", out.data);
    assert!(out.data[3..6].iter().all(|v| *v <= 5), "{:?}", out.data);
    // The system's generic CMYK printer profile: white is no ink, black is heavy ink.
    let cmyk = std::path::Path::new("/System/Library/ColorSync/Profiles/Generic CMYK Profile.icc");
    if cmyk.exists() {
        let bytes = std::fs::read(cmyk).unwrap();
        let info = describe_output(cmyk, &bytes).unwrap();
        assert_eq!(info.color_space, "CMYK");
        let printer = r.load_bytes(&bytes).unwrap();
        let out = convert_rgb(
            &srgb,
            &printer,
            Intent::Perceptual,
            true,
            &[[1.0; 3], [0.0; 3]],
        )
        .unwrap();
        assert_eq!(out.channels, 4);
        assert!(out.data[..4].iter().all(|v| *v <= 3), "{:?}", out.data);
        assert!(out.data[4..].iter().map(|v| u32::from(*v)).sum::<u32>() > 500);
    }
}
