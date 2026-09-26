fn main() {
    println!("cargo:rerun-if-changed=native/bridge.m");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("macos") {
        return;
    }
    cc::Build::new()
        .file("native/bridge.m")
        .flag("-fobjc-arc")
        .flag("-fblocks")
        // Delegate protocol methods necessarily have unused SDK parameters.
        .flag("-Wno-unused-parameter")
        .warnings(true)
        .compile("tessera_tether_native");
    println!("cargo:rustc-link-lib=framework=Foundation");
    println!("cargo:rustc-link-lib=framework=ImageCaptureCore");
}
