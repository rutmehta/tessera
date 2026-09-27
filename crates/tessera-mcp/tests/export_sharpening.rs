use engine_api::tools::{ToolOutput, ToolRequest, ToolResponse};
use serde_json::json;
use tessera_mcp::Console;

#[test]
fn sharpening_schema_and_legacy_defaults() {
    let settings: engine_api::tools::ExportSettings = serde_json::from_value(json!({
        "destination": "/unused", "format": {"format": "png", "bit_depth": 8}
    }))
    .unwrap();
    assert_eq!(settings.sharpening, None);
    assert_eq!(settings.sharpening_amount, None);
    assert_eq!(settings.ppi, None);
    let tool = tessera_mcp::schema::tools()
        .into_iter()
        .find(|t| t["name"] == "export")
        .unwrap();
    let schema = &tool["inputSchema"]["$defs"]["ExportSettings"];
    for field in ["sharpening", "sharpening_amount", "ppi"] {
        assert!(schema["properties"][field].is_object(), "{tool}");
        assert!(
            !schema["required"]
                .as_array()
                .unwrap()
                .contains(&json!(field))
        );
    }
}

#[test]
fn invalid_sharpening_fails_before_decoding_or_writing() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("chart.png");
    image::RgbImage::new(16, 16).save(&path).unwrap();
    let mut console = Console::open(dir.path().join("app")).unwrap();
    let id = console.open_image(&path).unwrap();
    std::fs::write(&path, b"must not decode invalid settings").unwrap();
    let sidecar_path = sidecar::Sidecar::paths(&path).recipe;
    let before = std::fs::read(&sidecar_path).ok();
    for (field, value) in [
        ("sharpening", json!("laser")),
        ("sharpening_amount", json!("extreme")),
        ("ppi", json!(0)),
        ("ppi", json!(9601)),
    ] {
        let out = dir.path().join("invalid");
        let mut settings = json!({
            "destination": out, "format": {"format": "png", "bit_depth": 8}
        });
        settings[field] = value;
        let request = serde_json::from_value(json!({
            "tool": "export", "images": [id], "settings": settings
        }))
        .unwrap();
        let response = console.execute(request);
        let ToolResponse::Error(error) = response else {
            panic!("{response:?}")
        };
        assert!(error.to_string().contains(field), "{field}: {error}");
        assert!(!out.exists());
        assert_eq!(std::fs::read(&sidecar_path).ok(), before);
    }
}

#[test]
fn sharpening_controls_reach_exported_chart_pixels() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("chart.png");
    image::RgbImage::from_fn(129, 81, |x, y| {
        image::Rgb([if x > 64 + y / 8 { 160 } else { 100 }; 3])
    })
    .save(&path)
    .unwrap();
    let mut console = Console::open(dir.path().join("app")).unwrap();
    let id = console.open_image(&path).unwrap();
    let mut run = |medium: &str, amount: &str, ppi: u32| {
        let out = dir.path().join(format!("{medium}-{amount}-{ppi}"));
        let mut settings = json!({
            "destination": out, "format": {"format": "png", "bit_depth": 8},
            "resize": {"fit": "long_edge", "pixels": 65}
        });
        if !medium.is_empty() {
            settings["sharpening"] = json!(medium);
        }
        if !amount.is_empty() {
            settings["sharpening_amount"] = json!(amount);
        }
        if ppi != 0 {
            settings["ppi"] = json!(ppi);
        }
        let request: ToolRequest = serde_json::from_value(json!({
            "tool": "export", "images": [id],
            "settings": settings
        }))
        .unwrap();
        let response = console.execute(request);
        assert!(
            matches!(
                response,
                ToolResponse::Ok(ToolOutput::ExportQueued { images: 1, .. })
            ),
            "{response:?}"
        );
        image::open(out.join("chart.png")).unwrap().to_rgb8()
    };
    for medium in ["screen", "matte", "glossy"] {
        let low = run(medium, "low", 300);
        let standard = run(medium, "standard", 300);
        let high = run(medium, "high", 300);
        assert_eq!(low.dimensions(), (65, 41));
        assert_ne!(low, standard, "{medium}: low/standard");
        assert_ne!(standard, high, "{medium}: standard/high");
        assert_eq!(standard, run(medium, "", 0), "{medium}: defaults");
        let lower_density = run(medium, "standard", 150);
        if medium == "screen" {
            assert_eq!(standard, lower_density);
        } else {
            assert_ne!(standard, lower_density, "{medium}: density");
        }
    }
    assert_eq!(run("", "", 0), run("none", "high", 300));
}
