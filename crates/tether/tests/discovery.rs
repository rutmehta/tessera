fn main() {
    use tether::TetherBackend;
    #[cfg(target_os = "macos")]
    {
        let mut backend = tether::native::NativeBackend::new().unwrap();
        let devices = backend.devices().unwrap();
        println!("ImageCaptureCore discovery: {} cameras", devices.len());
        if std::env::var_os("TESSERA_EXPECT_NO_CAMERA").is_some() {
            assert!(devices.is_empty());
        }
    }
}
