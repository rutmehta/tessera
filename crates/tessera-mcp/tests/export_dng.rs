use engine_api::tools::{ToolOutput, ToolResponse};
use serde_json::json;
use tessera_mcp::Console;

#[test]
fn dng_mcp_embeds_original_and_publishes_schema() {
    let tool = tessera_mcp::schema::tools()
        .into_iter()
        .find(|t| t["name"] == "export")
        .unwrap();
    assert_eq!(
        tool["inputSchema"]["$defs"]["ExportSettings"]["properties"]["embed_original_raw"]["type"],
        "boolean"
    );
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("chart.png");
    image::RgbImage::from_pixel(16, 16, image::Rgb([100; 3]))
        .save(&path)
        .unwrap();
    let mut console = Console::open(dir.path().join("app")).unwrap();
    let id = console.open_image(&path).unwrap();
    let out = dir.path().join("out");
    let response = console.execute(
        serde_json::from_value(json!({
            "tool":"export", "images":[id], "settings": {
                "destination":out, "format":{"format":"dng"}, "embed_original_raw":true
            }
        }))
        .unwrap(),
    );
    assert!(
        matches!(
            response,
            ToolResponse::Ok(ToolOutput::ExportQueued { images: 1, .. })
        ),
        "{response:?}"
    );
    let bytes = std::fs::read(out.join("chart.dng")).unwrap();
    assert!(bytes.windows(10).any(|v| v == b"chart.png\0"));
    let rejected = console.execute(serde_json::from_value(json!({
        "tool":"export", "images":[id], "settings": {
            "destination":dir.path().join("private"), "format":{"format":"dng"}, "embed_original_raw":true, "embed_metadata":false
        }
    })).unwrap());
    assert!(matches!(rejected, ToolResponse::Error(_)));
    assert!(!dir.path().join("private").exists());
}
