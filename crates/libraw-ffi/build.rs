fn main() {
    let root = std::path::PathBuf::from("vendor/LibRaw/LibRaw-0.22.2");
    let include = root.join("libraw");
    let mut build = cc::Build::new();
    build
        .cpp(true)
        .std("c++17")
        .define("USE_ZLIB", "0")
        .include(&root)
        .include(&include);
    fn add_cpp(build: &mut cc::Build, dir: &std::path::Path) {
        for entry in std::fs::read_dir(dir).expect("LibRaw sources") {
            let path = entry.expect("source entry").path();
            if path.is_dir() {
                add_cpp(build, &path);
            } else if path.extension().is_some_and(|ext| ext == "cpp")
                && path
                    .file_name()
                    .is_some_and(|name| name != "write_ph.cpp" && name != "postprocessing_ph.cpp")
            {
                build.file(path);
            }
        }
    }
    add_cpp(&mut build, &root.join("src"));
    build.compile("raw");
    // LibRaw's DNG deflate decoder references zlib even with optional thumbnail
    // zlib support disabled; macOS provides the system libz.
    println!("cargo:rustc-link-lib=z");
    let bindings = bindgen::Builder::default()
        .header("vendor/LibRaw/LibRaw-0.22.2/libraw/libraw.h")
        .clang_arg("-Ivendor/LibRaw/LibRaw-0.22.2")
        .clang_arg("-DUSE_ZLIB=0")
        .allowlist_function("libraw_.*")
        .allowlist_type("libraw_.*")
        .allowlist_type("LibRaw_.*")
        .allowlist_var("LIBRAW_.*")
        .generate()
        .expect("generate LibRaw bindings");
    bindings
        .write_to_file(
            std::path::PathBuf::from(std::env::var("OUT_DIR").unwrap()).join("bindings.rs"),
        )
        .unwrap();
    println!("cargo:rerun-if-changed=vendor/LibRaw");
}
