//! Layered-document tools and Actions end to end over the in-memory MCP
//! transport.
use rmcp::ServiceExt;
use serde_json::{Value, json};
use tessera_mcp::{Console, Server};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

struct Client<R, W> {
    reader: R,
    writer: W,
    id: u64,
}

impl<R: tokio::io::AsyncBufRead + Unpin, W: tokio::io::AsyncWrite + Unpin> Client<R, W> {
    async fn send(&mut self, value: Value) {
        self.writer
            .write_all(format!("{value}\n").as_bytes())
            .await
            .unwrap();
        self.writer.flush().await.unwrap();
    }
    async fn receive(&mut self) -> Value {
        let mut line = String::new();
        tokio::time::timeout(
            std::time::Duration::from_secs(60),
            self.reader.read_line(&mut line),
        )
        .await
        .unwrap()
        .unwrap();
        serde_json::from_str(&line).unwrap()
    }
    async fn request(&mut self, method: &str, params: Value) -> Value {
        self.id += 1;
        let id = self.id;
        self.send(json!({"jsonrpc":"2.0","id":id,"method":method,"params":params}))
            .await;
        let r = self.receive().await;
        assert_eq!(r["id"], id, "{r}");
        r
    }
    /// A tools/call; returns (isError, parsed first text block, content).
    async fn call(&mut self, name: &str, args: Value) -> (bool, Value, Vec<Value>) {
        let r = self
            .request("tools/call", json!({"name":name,"arguments":args}))
            .await;
        let result = &r["result"];
        assert!(result.is_object(), "{name}: {r}");
        let content = result["content"].as_array().unwrap().clone();
        let text: Value = content[0]["text"]
            .as_str()
            .map_or(Value::Null, |t| serde_json::from_str(t).unwrap());
        (result["isError"] == true, text, content)
    }
    async fn ok(&mut self, name: &str, args: Value) -> (Value, Vec<Value>) {
        let (error, text, content) = self.call(name, args).await;
        assert!(!error, "{name} failed: {text}");
        (text, content)
    }
}

fn photo(dir: &std::path::Path, name: &str) -> String {
    let path = dir.join(name);
    image::RgbImage::from_fn(80, 60, |x, y| {
        image::Rgb([(x * 3) as u8, (y * 4) as u8, ((x + y) * 2) as u8])
    })
    .save(&path)
    .unwrap();
    path.to_str().unwrap().into()
}

#[tokio::test]
async fn document_tools_history_conflicts_schemas_and_actions_over_mcp() {
    let dir = tempfile::tempdir().unwrap();
    let a_path = photo(dir.path(), "a.png");
    let b_path = photo(dir.path(), "b.png");
    let server = Server::new(Console::open(dir.path().join("app")).unwrap());
    let (client, transport) = tokio::io::duplex(1 << 22);
    let task = tokio::spawn(async move {
        server
            .serve(transport)
            .await
            .unwrap()
            .waiting()
            .await
            .unwrap();
    });
    let (reader, writer) = tokio::io::split(client);
    let mut c = Client {
        reader: BufReader::new(reader),
        writer,
        id: 0,
    };
    let init = c
        .request(
            "initialize",
            json!({"protocolVersion":"2025-03-26","capabilities":{},"clientInfo":{"name":"test","version":"1"}}),
        )
        .await;
    assert!(init["result"]["capabilities"]["tools"].is_object());
    c.send(json!({"jsonrpc":"2.0","method":"notifications/initialized"}))
        .await;

    // Every document tool is listed once, with an object schema.
    let list = c.request("tools/list", json!({})).await;
    let tools = list["result"]["tools"].as_array().unwrap();
    let names: Vec<&str> = tools.iter().map(|t| t["name"].as_str().unwrap()).collect();
    for name in engine_api::tools::DocumentToolCall::NAMES.iter().chain(&[
        "describe_document",
        "render_document_preview",
        "actions_record",
        "actions_stop",
        "actions_play",
    ]) {
        assert_eq!(names.iter().filter(|n| *n == name).count(), 1, "{name}");
    }
    let unique: std::collections::BTreeSet<_> = names.iter().collect();
    assert_eq!(unique.len(), names.len());
    assert!(tools.iter().all(|t| t["inputSchema"]["type"] == "object"));

    // open → adjustment → layer → paint → export.
    let (opened, _) = c.ok("open_document", json!({"path": a_path})).await;
    let doc = opened["ok"]["document"].as_u64().unwrap();
    assert_eq!(opened["ok"]["canvas"], json!({"width":80,"height":60}));
    assert_eq!(opened["warnings"], json!([]));
    let (extra, _) = c.ok("open_document", json!({"path": b_path})).await;
    let advanced_doc = extra["ok"]["document"].as_u64().unwrap();
    for operation in [
        json!({"kind":"wand","seed":[1,1],"tolerance":20}),
        json!({"kind":"quick","stroke":[[2,2],[4,4]],"radius":2}),
    ] {
        let (selected, _) = c
            .ok(
                "select_advanced",
                json!({"document":advanced_doc,"operation":operation}),
            )
            .await;
        assert!(selected["ok"]["entry"].is_u64());
        c.ok("undo", json!({"document":advanced_doc})).await;
        c.ok("redo", json!({"document":advanced_doc})).await;
        c.ok("undo", json!({"document":advanced_doc})).await;
    }
    let (brushes, _) = c.ok("list_brushes", json!({})).await;
    let (saved, _) = c
        .ok(
            "set_pixel_selection",
            json!({"document":advanced_doc,"shape":"all","save_as":"all"}),
        )
        .await;
    let (staged, _) = c
        .ok("stage_channel_raster", json!({"document":advanced_doc}))
        .await;
    let (added, _) = c.ok("add_channel", json!({"document":advanced_doc,"name":"Ink","kind":{"kind":"spot","display_rgb":[1,0,0],"solidity":0.75},"raster":staged["ok"]})).await;
    let channel = added["ok"]["channel"].clone();
    assert!(channel.is_u64());
    c.ok(
        "load_channel_as_selection",
        json!({"document":advanced_doc,"channel":channel}),
    )
    .await;
    let (summary, _) = c
        .ok(
            "describe_document",
            json!({"document":advanced_doc,"thumbnail_px":null}),
        )
        .await;
    assert_eq!(summary["summary"]["channels"][1]["id"], channel);
    assert_eq!(
        summary["summary"]["channels"][1]["kind"]["display_rgb"],
        json!([1.0, 0.0, 0.0])
    );
    c.ok(
        "refine_edge",
        json!({"document":advanced_doc,"radius":1,"smooth":1}),
    )
    .await;
    c.ok("selection_boolean", json!({"document":advanced_doc,"selection":saved["ok"]["selection"],"operation":"subtract"})).await;
    let (missing_model, _, _) = c
        .call(
            "select_advanced",
            json!({"document":advanced_doc,"operation":{"kind":"subject"}}),
        )
        .await;
    assert!(missing_model);
    c.ok(
        "set_pixel_selection",
        json!({"document":advanced_doc,"shape":"none"}),
    )
    .await;
    assert_eq!(brushes["ok"]["presets"], json!([]));
    let tip = brush::tip::SampledTip::new("tip", 2, 2, vec![1.0; 4]).unwrap();
    let abr = dir.path().join("tips.abr");
    std::fs::write(&abr, brush::abr::write_v6(&[tip], 6, 2, true).unwrap()).unwrap();
    let (imported, _) = c.ok("import_brushes", json!({"path":abr})).await;
    assert_eq!(imported["ok"]["presets"].as_array().unwrap().len(), 1);
    let (listed, _) = c.ok("list_brushes", json!({})).await;
    let preset_id = listed["ok"]["presets"][0]["id"].clone();
    c.ok(
        "paint_preset",
        json!({"document":advanced_doc,"layer":1,"preset_id":preset_id,"points":[{"x":4,"y":4}]}),
    )
    .await;
    c.ok("actions_record", json!({"name":"Look"})).await;
    let (adj, content) = c
        .ok(
            "apply_adjustment_layer",
            json!({"document":doc,"adjustment":{"kind":"hue_saturation","saturation":-60},
                   "rationale":"Mute colour for the editorial look","expect_head":null}),
        )
        .await;
    assert_eq!(adj["ok"]["entry"], 1);
    assert_eq!(content[1]["type"], "image", "edits carry a preview");
    let (layer, _) = c
        .ok(
            "add_layer",
            json!({"document":doc,"layer":{"kind":"pixel"},"name":"Dodge","props":{"blend_mode":"soft_light"},
                   "rationale":"Separate layer for dodging","expect_head":1}),
        )
        .await;
    let paint_layer = layer["ok"]["layer"].as_u64().unwrap();
    c.ok(
        "paint_stroke",
        json!({"document":doc,"layer":paint_layer,"points":[{"x":10,"y":10},{"x":70,"y":50,"pressure":0.4}],
               "brush":{"size":12,"hardness":0.5,"color":[1,1,1],"pressure_size":true},
               "rationale":"Lift the diagonal","expect_head":2}),
    )
    .await;
    c.ok(
        "actions_stop",
        json!({"path": dir.path().join("look.tessera-action").to_str().unwrap()}),
    )
    .await;
    let out_a = dir.path().join("a-out.png");
    c.ok(
        "export_document",
        json!({"document":doc,"settings":{"path":out_a.to_str().unwrap(),
               "format":{"container":"image","encoding":{"format":"png","bit_depth":8}}}}),
    )
    .await;
    assert!(out_a.exists());

    // History: one Agent entry per edit, with rationale, via describe_document.
    let (desc, content) = c
        .ok(
            "describe_document",
            json!({"document":doc,"max_px":64,"thumbnail_px":16}),
        )
        .await;
    assert_eq!(desc["history"]["entries"], 3);
    let recent = desc["history"]["recent"].as_array().unwrap();
    let rationales: Vec<&str> = recent
        .iter()
        .map(|e| e["rationale"].as_str().unwrap())
        .collect();
    assert_eq!(
        rationales,
        [
            "Lift the diagonal",
            "Separate layer for dodging",
            "Mute colour for the editorial look"
        ]
    );
    assert!(
        recent.iter().all(|e| e["author"]["kind"] == "agent"),
        "{recent:?}"
    );
    assert_eq!(desc["layers"][2]["blend_mode"], "soft_light");
    assert_eq!(
        content.len(),
        2 + desc["layers"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|l| !l["thumbnail"].is_null())
            .count()
    );
    let (_, preview) = c
        .ok(
            "render_document_preview",
            json!({"document":doc,"max_px":40}),
        )
        .await;
    assert_eq!(preview[0]["mimeType"], "image/png");

    // Stale expect_head: a tool error, and no entry is recorded.
    let (error, text, _) = c
        .call(
            "set_layer_props",
            json!({"document":doc,"layer":paint_layer,"opacity":0.3,"expect_head":1}),
        )
        .await;
    assert!(error);
    assert_eq!(text["error"]["code"], "conflict", "{text}");
    let (layers, _) = c.ok("list_layers", json!({"document":doc})).await;
    assert_eq!(layers["ok"]["layers"][2]["opacity"], 1.0);

    // Schema violations are JSON-RPC invalid params, before execution.
    for (name, args) in [
        (
            "add_layer",
            json!({"document":doc,"layer":{"kind":"pixel"},"nmae":"x"}),
        ),
        (
            "set_pixel_selection",
            json!({"document":doc,"shape":"rect","rect":{"x0":0,"y0":0,"x1":1,"y1":1},"feathr":2}),
        ),
        (
            "paint_stroke",
            json!({"document":doc,"layer":2,"points":"none"}),
        ),
        ("actions_play", json!({"documents":[1],"extra":true})),
    ] {
        let r = c
            .request("tools/call", json!({"name":name,"arguments":args}))
            .await;
        assert_eq!(r["error"]["code"], -32602, "{name}: {r}");
    }

    // Replay the recorded action on B: identical exported pixels.
    let (opened, _) = c.ok("open_document", json!({"path": b_path})).await;
    let b = opened["ok"]["document"].as_u64().unwrap();
    let (report, _) = c
        .ok(
            "actions_play",
            json!({"path": dir.path().join("look.tessera-action").to_str().unwrap(), "documents":[b]}),
        )
        .await;
    assert_eq!(report["ok"]["steps"].as_array().unwrap().len(), 3);
    let out_b = dir.path().join("b-out.png");
    c.ok(
        "export_document",
        json!({"document":b,"settings":{"path":out_b.to_str().unwrap(),
               "format":{"container":"image","encoding":{"format":"png","bit_depth":8}}}}),
    )
    .await;
    assert_eq!(
        image::open(&out_a).unwrap().to_rgba8().into_raw(),
        image::open(&out_b).unwrap().to_rgba8().into_raw()
    );
    // A failing replay (wrong input count) is a tool error.
    let (error, _, _) = c
        .call("actions_play", json!({"path": dir.path().join("look.tessera-action").to_str().unwrap(), "documents":[]}))
        .await;
    assert!(error);
    drop(c);
    task.abort();
}
