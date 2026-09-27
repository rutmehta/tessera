#[path = "../../export/tests/support/native_fixture.rs"]
mod native_fixture;
use engine_api::tools::{ToolOutput, ToolResponse};
use native_fixture::{stamp, tags};
use serde_json::json;
#[test]
fn mcp_native_metadata_policies_and_schema() {
    let tool = tessera_mcp::schema::tools()
        .into_iter()
        .find(|t| t["name"] == "export")
        .unwrap();
    let props = &tool["inputSchema"]["$defs"]["ExportSettings"]["properties"];
    for field in [
        "metadata",
        "remove_person_info",
        "remove_location",
        "keywords_as_hierarchy",
    ] {
        assert!(!props[field].is_null(), "missing {field}");
    }
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("source.jpg");
    image::RgbImage::from_pixel(16, 16, image::Rgb([90; 3]))
        .save(&path)
        .unwrap();
    stamp(&path, "Native Rights");
    let mut console = tessera_mcp::Console::open(dir.path().join("app")).unwrap();
    let id = console.open_image(&path).unwrap();
    for policy in [
        "all",
        "copyright",
        "copyright_and_contact",
        "all_except_camera",
        "none",
    ] {
        let out = dir.path().join(policy);
        let response=console.execute(serde_json::from_value(json!({"tool":"export","images":[id],"settings":{"destination":out,"format":{"format":"png","bit_depth":8},"metadata":policy,"remove_location":true}})).unwrap());
        assert!(
            matches!(
                response,
                ToolResponse::Ok(ToolOutput::ExportQueued { images: 1, .. })
            ),
            "{response:?}"
        );
        let path = out.join("source.png");
        assert_eq!(
            tags(&path, &["-s3", "-EXIF:Copyright"]).contains("Native Rights"),
            policy != "none"
        );
        assert_eq!(
            tags(&path, &["-s3", "-EXIF:Make"]).contains("Private Camera"),
            policy == "all"
        );
        assert!(tags(&path, &["-s3", "-IPTC:City"]).trim().is_empty());
        assert_eq!(
            tags(&path, &["-s3", "-IPTC:Contact"]).contains("contact@"),
            matches!(
                policy,
                "all" | "copyright_and_contact" | "all_except_camera"
            )
        );
    }
}
