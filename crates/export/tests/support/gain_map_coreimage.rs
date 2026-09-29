//! Supplemental software Core Image readback. This never substitutes for the
//! separate ImageIO CGImage assertions, including their retained four-stop failure.
use std::{fs, process::Command, sync::OnceLock};

pub struct Pixels {
    pub base: Vec<[f32; 4]>,
    pub expanded: Vec<[f32; 4]>,
    pub explicit: Vec<[f32; 4]>,
}

pub fn decode(bytes: &[u8], headroom: f32, expected_gain: bool) -> Pixels {
    static HELPER: OnceLock<tempfile::TempDir> = OnceLock::new();
    let helper = HELPER.get_or_init(|| {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("reader.m");
        fs::write(&source, include_str!("gain_map_coreimage.m")).unwrap();
        let output = Command::new("xcrun")
            .args([
                "clang",
                "-fobjc-arc",
                "-framework",
                "Foundation",
                "-framework",
                "CoreImage",
                "-framework",
                "CoreGraphics",
            ])
            .arg(&source)
            .arg("-o")
            .arg(dir.path().join("reader"))
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "Core Image helper compile: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        dir
    });
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("input.jpg");
    fs::write(&input, bytes).unwrap();
    let output = Command::new(helper.path().join("reader"))
        .arg(input)
        .arg(dir.path())
        .arg(headroom.to_string())
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "Core Image readback: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let result: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(result["gain_present"], expected_gain, "{result}");
    let read = |name: &str| {
        assert_eq!(result[name]["valid"], true, "{result}");
        let data = fs::read(dir.path().join(format!("{name}.f32"))).unwrap();
        assert_eq!(data.len(), 80 * 16 * 4 * 4);
        let pixels: Vec<[f32; 4]> = data
            .chunks_exact(16)
            .map(|pixel| {
                std::array::from_fn(|c| {
                    f32::from_le_bytes(pixel[c * 4..c * 4 + 4].try_into().unwrap())
                })
            })
            .collect();
        assert!(
            pixels
                .iter()
                .all(|pixel| pixel.iter().all(|v| v.is_finite()) && pixel[3] > 0.99)
        );
        pixels
    };
    Pixels {
        base: read("base"),
        expanded: read("expanded"),
        explicit: read("explicit"),
    }
}
