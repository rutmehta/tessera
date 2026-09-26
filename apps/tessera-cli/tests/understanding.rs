use assert_cmd::cargo::cargo_bin_cmd;
#[test]
fn understanding_commands_are_available_and_require_images() {
    for command in ["keywords", "caption", "ocr"] {
        cargo_bin_cmd!("tessera")
            .args(["ml", command, "--help"])
            .assert()
            .success();
        cargo_bin_cmd!("tessera")
            .args(["ml", command])
            .assert()
            .failure();
    }
    cargo_bin_cmd!("tessera")
        .args(["ml", "keywords", "x.jpg", "--write-xmp"])
        .assert()
        .failure();
}
