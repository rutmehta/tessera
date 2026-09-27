use engine_api::tools::{ToolOutput, ToolResponse};
use serde_json::json;
use tessera_mcp::Console;

#[test]
fn hdr_mcp_exports_and_publishes_controls() {
    let tool = tessera_mcp::schema::tools()
        .into_iter()
        .find(|t| t["name"] == "export")
        .unwrap();
    let properties = &tool["inputSchema"]["$defs"]["ExportSettings"]["properties"];
    assert!(properties.get("hdr_transfer").is_some());
    assert!(properties.get("avif_bit_depth").is_some());
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("chart.png");
    image::RgbImage::from_pixel(32, 32, image::Rgb([100; 3]))
        .save(&path)
        .unwrap();
    let mut console = Console::open(dir.path().join("app")).unwrap();
    let id = console.open_image(&path).unwrap();
    for transfer in ["pq", "hlg"] {
        let out = dir.path().join(transfer);
        let response = console.execute(serde_json::from_value(json!({"tool":"export","images":[id],"settings":{
            "destination":out,"format":{"format":"png","bit_depth":16},"hdr":true,"hdr_transfer":transfer
        }})).unwrap());
        assert!(
            matches!(
                response,
                ToolResponse::Ok(ToolOutput::ExportQueued { images: 1, .. })
            ),
            "{response:?}"
        );
        let bytes = std::fs::read(out.join("chart.png")).unwrap();
        assert!(bytes.windows(8).any(|v| v
            == [
                b'c',
                b'I',
                b'C',
                b'P',
                9,
                if transfer == "pq" { 16 } else { 18 },
                0,
                1
            ]));
    }
}
