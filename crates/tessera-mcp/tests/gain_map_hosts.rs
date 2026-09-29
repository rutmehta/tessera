#[path = "../../export/tests/support/gain_map_fixture.rs"]
mod gain_map_fixture;
use engine_api::tools::{ToolOutput, ToolResponse};
#[test]
fn mcp_hdr_jpeg_uses_iso_gain_map_and_rejects_transfer() {
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("source.jpg");
    image::RgbImage::from_pixel(80, 16, image::Rgb([255; 3]))
        .save(&source)
        .unwrap();
    gain_map_fixture::hdr_recipe(&source);
    let mut console = tessera_mcp::Console::open(dir.path().join("app")).unwrap();
    let id = console.open_image(&source).unwrap();
    let out = dir.path().join("out");
    let request = serde_json::from_value(serde_json::json!({"tool":"export","images":[id],"settings":{"destination":out,"format":{"format":"jpeg","quality":95},"hdr":true}})).unwrap();
    let response = console.execute(request);
    assert!(
        matches!(
            response,
            ToolResponse::Ok(ToolOutput::ExportQueued { images: 1, .. })
        ),
        "{response:?}"
    );
    gain_map_fixture::check(&out.join("source.jpg"));
    for extra in [
        serde_json::json!({"hdr_transfer":"pq"}),
        serde_json::json!({"avif_bit_depth":10}),
    ] {
        let failed = dir.path().join("invalid");
        let mut settings = serde_json::json!({"destination":failed,"format":{"format":"jpeg","quality":95},"hdr":true});
        settings
            .as_object_mut()
            .unwrap()
            .extend(extra.as_object().unwrap().clone());
        let response = console.execute(
            serde_json::from_value(
                serde_json::json!({"tool":"export","images":[id],"settings":settings}),
            )
            .unwrap(),
        );
        assert!(matches!(response, ToolResponse::Error(_)), "{response:?}");
        assert!(!failed.exists());
    }
}
