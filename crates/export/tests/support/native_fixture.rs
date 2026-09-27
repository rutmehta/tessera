use std::{path::Path, process::Command};
pub fn tags(path: &Path, args: &[&str]) -> String {
    let out = Command::new("exiftool")
        .args(args)
        .arg(path)
        .output()
        .expect("ExifTool required for independent metadata tests");
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout).unwrap()
}
pub fn stamp(path: &Path, rights: &str) {
    tags(
        path,
        &[
            "-overwrite_original",
            &format!("-EXIF:Copyright={rights}"),
            "-EXIF:Make=Private Camera",
            "-IPTC:City=Private City",
            "-IPTC:Contact=contact@example.test",
        ],
    );
}
