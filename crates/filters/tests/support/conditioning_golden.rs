//! Persistent encoded references plus opt-in capture for the isolated ENG-1b audit.
use compositor::raster::Raster;
use std::path::PathBuf;

pub fn begin() {
    if let Some(dir) = std::env::var_os("ENG1_CAPTURE") {
        std::fs::write(PathBuf::from(dir).join("trace.tsv"), []).unwrap();
    }
}

pub fn check(name: &str, output: &Raster) {
    let e = output.extent();
    let mut bytes = Vec::new();
    for y in 0..e.height {
        for x in 0..e.width {
            for v in output.pixel(x, y) {
                assert!(v.is_finite());
                bytes.extend_from_slice(&v.to_le_bytes());
            }
        }
    }
    if let Some(dir) = std::env::var_os("ENG1_CAPTURE") {
        let dir = PathBuf::from(dir);
        std::fs::write(dir.join(format!("{name}.rgba")), bytes).unwrap();
        std::fs::copy(dir.join("trace.tsv"), dir.join(format!("{name}.tsv"))).unwrap();
    } else {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests/goldens/conditioning")
            .join(format!("{name}.rgba"));
        let expected = std::fs::read(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        assert_eq!(bytes.len(), expected.len());
        let max = bytes
            .chunks_exact(4)
            .zip(expected.chunks_exact(4))
            .map(|(a, b)| {
                (f32::from_le_bytes(a.try_into().unwrap())
                    - f32::from_le_bytes(b.try_into().unwrap()))
                .abs()
            })
            .fold(0_f32, f32::max);
        assert!(
            max < super::GOLDEN_TOLERANCE,
            "{name}: stored encoded golden delta {max}"
        );
    }
}
