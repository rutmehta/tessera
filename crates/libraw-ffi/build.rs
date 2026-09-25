use std::{env, path::PathBuf};

fn main() {
    let root = PathBuf::from("vendor/LibRaw");
    let mut build = cc::Build::new();
    build.cpp(true).std("c++17").define("USE_ZLIB", "0").include(&root).include(root.join("libraw"));
    for source in glob::glob("vendor/LibRaw/src/*.cpp").expect("source glob") {
        build.file(source.expect("source path"));
    }
    build.compile("raw");
    let bindings = bindgen::Builder::default()
        .header("vendor/LibRaw/libraw/libraw.h")
        .clang_arg("-Ivendor/LibRaw")
        .clang_arg("-Ivendor/LibRaw/libraw")
        .allowlist_function("libraw_.*")
        .allowlist_type("libraw_.*")
        .allowlist_var("libraw_.*")
        .generate()
        .expect("generate LibRaw bindings");
    bindings.write_to_file(PathBuf::from(env::var("OUT_DIR").unwrap()).join("bindings.rs")).unwrap();
    println!("cargo:rerun-if-changed=vendor/LibRaw");
}
