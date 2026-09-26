use rmcp::ServiceExt;
use serde_json::{Value, json};
use tessera_mcp::{Console, Server};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

async fn send<W: tokio::io::AsyncWrite + Unpin>(writer: &mut W, value: Value) {
    writer
        .write_all(format!("{value}\n").as_bytes())
        .await
        .unwrap();
    writer.flush().await.unwrap();
}
async fn receive<R: tokio::io::AsyncBufRead + Unpin>(reader: &mut R) -> Value {
    let mut line = String::new();
    tokio::time::timeout(
        std::time::Duration::from_secs(20),
        reader.read_line(&mut line),
    )
    .await
    .unwrap()
    .unwrap();
    serde_json::from_str(&line).unwrap()
}
async fn initialize<R: tokio::io::AsyncBufRead + Unpin, W: tokio::io::AsyncWrite + Unpin>(
    reader: &mut R,
    writer: &mut W,
) {
    send(writer,json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-03-26","capabilities":{},"clientInfo":{"name":"test","version":"1"}}})).await;
    let response = receive(reader).await;
    assert_eq!(response["id"], 1);
    assert!(
        response["result"]["capabilities"]["tools"].is_object(),
        "{response}"
    );
    send(
        writer,
        json!({"jsonrpc":"2.0","method":"notifications/initialized"}),
    )
    .await;
}
#[tokio::test]
async fn memory_initialize_list_call_bad_schema_compare_and_resources() {
    let dir = tempfile::tempdir().unwrap();
    let photo = dir.path().join("photo.jpg");
    image::RgbImage::from_pixel(16, 16, image::Rgb([100, 120, 90]))
        .save(&photo)
        .unwrap();
    let mut console = Console::open(dir.path().join("app")).unwrap();
    let image = console.open_image(&photo).unwrap();
    let server = Server::new(console);
    let (client, transport) = tokio::io::duplex(1 << 20);
    let task = tokio::spawn(async move {
        server
            .serve(transport)
            .await
            .unwrap()
            .waiting()
            .await
            .unwrap();
    });
    let (reader, mut writer) = tokio::io::split(client);
    let mut reader = BufReader::new(reader);
    initialize(&mut reader, &mut writer).await;
    send(
        &mut writer,
        json!({"jsonrpc":"2.0","id":2,"method":"tools/list"}),
    )
    .await;
    let list = receive(&mut reader).await;
    let tools = list["result"]["tools"].as_array().unwrap();
    assert_eq!(tools.len(), 32);
    for name in engine_api::tools::ToolCall::NAMES {
        assert!(tools.iter().any(|t| t["name"] == name));
    }
    assert!(tools.iter().all(|t| t["inputSchema"]["type"] == "object"));
    for (id, args) in [
        (3, json!({"image":image,"exposure":"bright"})),
        (4, json!({"exposure":1})),
        (5, json!({"image":image,"typo":1})),
    ] {
        send(&mut writer,json!({"jsonrpc":"2.0","id":id,"method":"tools/call","params":{"name":"set_tone","arguments":args}})).await;
        assert_eq!(receive(&mut reader).await["error"]["code"], -32602);
    }
    send(&mut writer,json!({"jsonrpc":"2.0","id":6,"method":"tools/call","params":{"name":"set_tone","arguments":{"image":image,"exposure":0.5,"rationale":"Lift","group":2}}})).await;
    assert_eq!(receive(&mut reader).await["result"]["isError"], false);
    send(&mut writer,json!({"jsonrpc":"2.0","id":7,"method":"tools/call","params":{"name":"compare","arguments":{"image_a":image,"image_b":image}}})).await;
    let response = receive(&mut reader).await;
    let content = response["result"]["content"].as_array().unwrap();
    assert_eq!(content.iter().filter(|c| c["type"] == "image").count(), 2);
    send(
        &mut writer,
        json!({"jsonrpc":"2.0","id":8,"method":"resources/list"}),
    )
    .await;
    assert!(
        receive(&mut reader).await["result"]["resources"]
            .as_array()
            .unwrap()
            .len()
            >= 3
    );
    for (id, kind) in [(9, "render"), (10, "histogram")] {
        send(&mut writer,json!({"jsonrpc":"2.0","id":id,"method":"resources/read","params":{"uri":format!("tessera://images/{image}/{kind}")}})).await;
        let result = receive(&mut reader).await;
        assert_eq!(result["result"]["contents"][0]["mimeType"], "image/png");
        assert!(
            result["result"]["contents"][0]["blob"]
                .as_str()
                .unwrap()
                .len()
                > 20
        );
    }
    drop(writer);
    drop(reader);
    tokio::time::timeout(std::time::Duration::from_secs(5), task)
        .await
        .unwrap()
        .unwrap();
}
#[tokio::test]
async fn spawned_stdio_lists_tools_without_stdout_noise() {
    let dir = tempfile::tempdir().unwrap();
    let mut child = tokio::process::Command::new(env!("CARGO_BIN_EXE_tessera-mcp"))
        .arg("--app-dir")
        .arg(dir.path())
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::inherit())
        .kill_on_drop(true)
        .spawn()
        .unwrap();
    let mut writer = child.stdin.take().unwrap();
    let mut reader = BufReader::new(child.stdout.take().unwrap());
    initialize(&mut reader, &mut writer).await;
    send(
        &mut writer,
        json!({"jsonrpc":"2.0","id":2,"method":"tools/list"}),
    )
    .await;
    assert_eq!(
        receive(&mut reader).await["result"]["tools"]
            .as_array()
            .unwrap()
            .len(),
        32
    );
    drop(writer);
    assert!(
        tokio::time::timeout(std::time::Duration::from_secs(5), child.wait())
            .await
            .unwrap()
            .unwrap()
            .success()
    );
}
