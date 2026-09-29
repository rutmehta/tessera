use std::path::Path;
#[cfg(target_os = "macos")]
#[path = "gain_map_imageio.rs"]
mod native_decode;
#[allow(dead_code)] // Shared fixture: FFI configures its indexed recipe through the host API.
pub fn hdr_recipe(path: &Path) {
    let mut doc = sidecar::RecipeDocument::default();
    doc.recipe
        .edit(engine_api::recipe::EditMeta::user("HDR", 0), |s| {
            s.output.hdr = true;
            s.output.hdr_headroom_stops = 2.;
        })
        .unwrap();
    sidecar::Sidecar::write_recipe(sidecar::Sidecar::paths(path).recipe, &doc).unwrap();
}
pub fn check(path: &Path) {
    let data = std::fs::read(path).unwrap();
    let iso = b"urn:iso:std:iso:ts:21496:-1\0";
    assert_eq!(data.windows(iso.len()).filter(|s| *s == iso).count(), 2);
    let out = std::process::Command::new("exiftool")
        .args(["-b", "-MPImage2"])
        .arg(path)
        .output()
        .unwrap();
    assert!(out.status.success());
    assert!(out.stdout.starts_with(b"\xff\xd8"));
    image::load_from_memory(&out.stdout).unwrap();
    assert_eq!(image::load_from_memory(&data).unwrap().width(), 80);
    #[cfg(target_os = "macos")]
    {
        let sdr = native_decode::decode(&data, false, true);
        let hdr = native_decode::decode(&data, true, true);
        assert!(
            hdr[8 * 80 + 40][0] > sdr[8 * 80 + 40][0] + 0.2,
            "host must export reconstructible HDR pixels"
        );
    }
}
