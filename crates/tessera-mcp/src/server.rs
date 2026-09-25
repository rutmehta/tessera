use crate::{Console, schema};
use base64::Engine as _;
use engine_api::{
    EngineError,
    tools::{ToolCall, ToolRequest, ToolResponse},
};
use rmcp::{ErrorData, RoleServer, ServerHandler, model::*, service::RequestContext};
use serde::de::DeserializeOwned;
use serde_json::{Value, json};
use std::sync::{Arc, Mutex};

/// MCP front end. Blocking engine work runs off the async transport thread, and
/// one mutex serializes mutations and comparisons within this server instance.
#[derive(Clone)]
pub struct Server {
    console: Arc<Mutex<Console>>,
}
impl Server {
    pub fn new(console: Console) -> Self {
        Self {
            console: Arc::new(Mutex::new(console)),
        }
    }
    async fn with_console<T: Send + 'static>(
        &self,
        f: impl FnOnce(&mut Console) -> Result<T, ErrorData> + Send + 'static,
    ) -> Result<T, ErrorData> {
        let console = self.console.clone();
        tokio::task::spawn_blocking(move || {
            let mut console = console
                .lock()
                .map_err(|_| ErrorData::internal_error("console lock poisoned", None))?;
            f(&mut console)
        })
        .await
        .map_err(|e| ErrorData::internal_error(e.to_string(), None))?
    }
}
impl ServerHandler for Server {
    fn get_info(&self) -> ServerConfig {
        serde_json::from_value(json!({"protocolVersion":"2025-03-26","capabilities":{"tools":{},"resources":{}},"serverInfo":{"name":"tessera-mcp","version":env!("CARGO_PKG_VERSION")},"instructions":"Non-generative photo editing. Mutation arguments accept rationale, group, and expect_recipe. Export and indexing complete synchronously. See crate README for engine limitations."})).expect("valid static server config")
    }
    fn supported_protocol_versions(&self) -> std::borrow::Cow<'static, [ProtocolVersion]> {
        std::borrow::Cow::Owned(vec![ProtocolVersion::V_2025_03_26])
    }
    async fn list_tools(
        &self,
        _: Option<PaginatedRequestParams>,
        _: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, ErrorData> {
        model(json!({"tools":schema::tools()}))
    }
    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        _: RequestContext<RoleServer>,
    ) -> Result<CallToolResponse, ErrorData> {
        let name = request.name.to_string();
        let args = Value::Object(request.arguments.unwrap_or_default());
        schema::validate(&name, &args)
            .map_err(|message| ErrorData::invalid_params(message, None))?;
        // Deserialize before executing: malformed wire arguments are JSON-RPC
        // errors, whereas valid operations that fail are MCP tool errors.
        let input = Input::parse(&name, args)?;
        self.with_console(move |console| dispatch(console, input))
            .await
            .map(Into::into)
    }
    async fn list_resources(
        &self,
        _: Option<PaginatedRequestParams>,
        _: RequestContext<RoleServer>,
    ) -> Result<ListResourcesResult, ErrorData> {
        self.with_console(|console| {
            let mut resources=vec![json!({"uri":"tessera://images","name":"Images","mimeType":"application/json"}),json!({"uri":"tessera://albums","name":"Albums","mimeType":"application/json"})];
            for image in console.list_images(None).map_err(resource_error)? {
                let id=image["id"].as_str().ok_or_else(||ErrorData::internal_error("missing catalog ID",None))?;
                for kind in ["render","histogram"] {resources.push(json!({"uri":format!("tessera://images/{id}/{kind}"),"name":format!("{id} {kind}"),"mimeType":"image/png"}));}
            }
            let library=library::Library::read(console.app.join("library.json")).map_err(resource_error)?;
            for album in library.albums.values() {resources.push(json!({"uri":format!("tessera://albums/{}",album.id),"name":album.name,"mimeType":"application/json"}));}
            model(json!({"resources":resources}))
        }).await
    }
    async fn read_resource(
        &self,
        request: ReadResourceRequestParams,
        _: RequestContext<RoleServer>,
    ) -> Result<ReadResourceResponse, ErrorData> {
        self.with_console(move |console| read_resource(console, &request.uri))
            .await
            .map(Into::into)
    }
}
enum Input {
    Engine(Box<ToolRequest>),
    Open(schema::OpenImage),
    Render(schema::RenderPreview),
    List(schema::ListImages),
    Describe(schema::DescribeImage),
}
impl Input {
    fn parse(name: &str, mut args: Value) -> Result<Self, ErrorData> {
        Ok(match name {
            "open_image" => Self::Open(arguments(args)?),
            "render_preview" => Self::Render(arguments(args)?),
            "list_images" => Self::List(arguments(args)?),
            "describe_image" => Self::Describe(arguments(args)?),
            _ => {
                args["tool"] = json!(name);
                Self::Engine(Box::new(arguments(args)?))
            }
        })
    }
}
fn dispatch(console: &mut Console, input: Input) -> Result<CallToolResult, ErrorData> {
    let result: Result<Vec<Value>, EngineError> = (|| match input {
        Input::Open(args) => Ok(vec![text(json!({"image":console.open_image(args.path)?}))]),
        Input::Render(args) => Ok(vec![image(
            &console.render_preview(args.image.parse()?, args.max_px)?,
        )?]),
        Input::List(args) => Ok(vec![text(json!(console.list_images(args.query)?))]),
        Input::Describe(args) => Ok(vec![text(console.describe_image(args.image.parse()?)?)]),
        Input::Engine(request) => {
            if let ToolCall::Compare {
                image_a,
                image_b,
                metric,
            } = request.call
            {
                let compared = console.compare_images(image_a, image_b, metric)?;
                return Ok(vec![
                    image(&compared.images[0])?,
                    image(&compared.images[1])?,
                    text(
                        json!({"metric":metric,"distance":compared.distance,"metrics":compared.metrics}),
                    ),
                ]);
            }
            match console.execute(*request) {
                ToolResponse::Ok(output) => Ok(vec![text(json!({"ok":output}))]),
                ToolResponse::Error(error) => Err(error),
            }
        }
    })();
    match result {
        Ok(content) => model(json!({"content":content,"isError":false})),
        Err(error) => model(json!({"content":[text(json!({"error":error}))],"isError":true})),
    }
}
fn text(value: Value) -> Value {
    json!({"type":"text","text":value.to_string()})
}
fn image(rgb: &image::RgbImage) -> Result<Value, EngineError> {
    Ok(json!({"type":"image","data":png(rgb)?,"mimeType":"image/png"}))
}
fn png(rgb: &image::RgbImage) -> Result<String, EngineError> {
    let mut bytes = std::io::Cursor::new(Vec::new());
    image::DynamicImage::ImageRgb8(rgb.clone())
        .write_to(&mut bytes, image::ImageFormat::Png)
        .map_err(|e| EngineError::Encode {
            format: "png".into(),
            message: e.to_string(),
        })?;
    Ok(base64::engine::general_purpose::STANDARD.encode(bytes.into_inner()))
}
fn arguments<T: DeserializeOwned>(value: Value) -> Result<T, ErrorData> {
    serde_json::from_value(value).map_err(|e| ErrorData::invalid_params(e.to_string(), None))
}
fn model<T: DeserializeOwned>(value: Value) -> Result<T, ErrorData> {
    serde_json::from_value(value).map_err(|e| ErrorData::internal_error(e.to_string(), None))
}
fn resource_error(error: EngineError) -> ErrorData {
    ErrorData::invalid_params(error.to_string(), Some(json!(error)))
}
fn read_resource(console: &Console, uri: &str) -> Result<ReadResourceResult, ErrorData> {
    let data = match uri {
        "tessera://images" => Some(json!(console.list_images(None).map_err(resource_error)?)),
        "tessera://albums" => Some(json!(
            library::Library::read(console.app.join("library.json"))
                .map_err(resource_error)?
                .albums
        )),
        _ => None,
    };
    if let Some(data) = data {
        return model(
            json!({"contents":[{"uri":uri,"mimeType":"application/json","text":data.to_string()}]}),
        );
    }
    if let Some(id) = uri.strip_prefix("tessera://albums/") {
        let id: i64 = id
            .parse()
            .map_err(|_| ErrorData::invalid_params("invalid album ID", None))?;
        let library =
            library::Library::read(console.app.join("library.json")).map_err(resource_error)?;
        let album = library
            .albums
            .values()
            .find(|a| a.id == id)
            .ok_or_else(|| ErrorData::invalid_params("album not found", None))?;
        return model(
            json!({"contents":[{"uri":uri,"mimeType":"application/json","text":serde_json::to_string(album).map_err(|e|ErrorData::internal_error(e.to_string(),None))?}]}),
        );
    }
    let (id, kind) = uri
        .strip_prefix("tessera://images/")
        .and_then(|s| s.split_once('/'))
        .ok_or_else(|| ErrorData::invalid_params("unknown resource URI", None))?;
    if !matches!(kind, "render" | "histogram") {
        return Err(ErrorData::invalid_params("unknown image resource", None));
    }
    let preview = console
        .render_preview(id.parse().map_err(resource_error)?, 1024)
        .map_err(resource_error)?;
    let rgb = if kind == "histogram" {
        histogram_image(&preview).map_err(resource_error)?
    } else {
        preview
    };
    model(
        json!({"contents":[{"uri":uri,"mimeType":"image/png","blob":png(&rgb).map_err(resource_error)?}]}),
    )
}
fn histogram_image(rgb: &image::RgbImage) -> Result<image::RgbImage, EngineError> {
    let h = crate::pixels::histogram(rgb, 256)?;
    let mut chart = image::RgbImage::from_pixel(256, 128, image::Rgb([16; 3]));
    let max = h
        .red
        .iter()
        .chain(&h.green)
        .chain(&h.blue)
        .copied()
        .max()
        .unwrap_or(1)
        .max(1);
    for (channel, bins) in [&h.red, &h.green, &h.blue].iter().enumerate() {
        for (x, count) in bins.iter().enumerate() {
            let height = (u64::from(*count) * 127 / u64::from(max)) as u32;
            for y in 128 - height..128 {
                chart.get_pixel_mut(x as u32, y)[channel] = 255;
            }
        }
    }
    Ok(chart)
}
