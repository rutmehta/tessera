//! MCP-local extensions while engine-api remains frozen.
use super::*;
use crate::Console;
use serde_json::{Value, json};

pub(crate) const NAMES: [&str; 8] = [
    "select_advanced",
    "refine_edge",
    "selection_boolean",
    "undo",
    "redo",
    "import_brushes",
    "list_brushes",
    "paint_preset",
];

pub(crate) fn schemas() -> Vec<Value> {
    let mut out = vec![
        serde_json::to_value(schemars::schema_for!(advanced::SelectAdvanced)).unwrap(),
        serde_json::to_value(schemars::schema_for!(advanced::RefineSelection)).unwrap(),
        serde_json::to_value(schemars::schema_for!(advanced::SelectionBoolean)).unwrap(),
        json!({"type":"object","properties":{"document":{"type":"integer"}},"required":["document"]}),
        json!({"type":"object","properties":{"document":{"type":"integer"}},"required":["document"]}),
        json!({"type":"object","properties":{"path":{"type":"string"}},"required":["path"]}),
        json!({"type":"object","properties":{}}),
        json!({"type":"object","properties":{"document":{"type":"integer"},"layer":{"type":"integer"},"preset_id":{"type":"integer"},"seed":{"type":"integer","default":0},"points":{"type":"array","items":{"type":"object","properties":{"x":{"type":"number"},"y":{"type":"number"},"pressure":{"type":"number","default":1}},"required":["x","y"]}},"target":{"enum":["pixels","mask"],"default":"pixels"}},"required":["document","layer","preset_id","points"]}),
    ];
    for (index, schema) in out.iter_mut().enumerate() {
        if index < 5 || index == 7 {
            schema["properties"]["rationale"] = json!({"type":["string","null"]});
            schema["properties"]["expect_head"] = json!({"type":["integer","null"]});
        }
        schema["additionalProperties"] = json!(false);
    }
    out
}

pub(crate) fn validate(name: &str, args: &Value) -> Result<(), String> {
    let index = NAMES
        .iter()
        .position(|n| *n == name)
        .ok_or("unknown tool")?;
    let schema = &schemas()[index];
    let obj = args.as_object().ok_or("object required")?;
    for key in obj.keys() {
        if schema["properties"].get(key).is_none() {
            return Err(format!("unknown field `{key}`"));
        }
    }
    for key in schema["required"].as_array().into_iter().flatten() {
        if !obj.contains_key(key.as_str().unwrap()) {
            return Err(format!("missing {key}"));
        }
    }
    Ok(())
}

pub(crate) fn execute(console: &mut Console, name: &str, mut args: Value) -> EngineResult<Value> {
    validate(name, &args).map_err(|e| EngineError::invalid("arguments", e))?;
    if console.is_recording() {
        return Err(crate::unsupported(
            "MCP-local tools cannot yet be recorded as portable Actions",
        ));
    }
    if name == "import_brushes" || name == "list_brushes" {
        let store = brush_presets::BrushPresetStore::open(&console.app)?;
        return if name == "list_brushes" {
            Ok(json!({"presets":store.list()?}))
        } else {
            let path = args["path"]
                .as_str()
                .ok_or_else(|| EngineError::invalid("path", "string required"))?;
            if !std::path::Path::new(path).is_absolute() {
                return Err(EngineError::invalid("path", "absolute path required"));
            }
            Ok(serde_json::to_value(store.import_abr_file(path)?)?)
        };
    }
    let id = DocumentId(
        args["document"]
            .as_u64()
            .ok_or_else(|| EngineError::invalid("document", "unsigned id required"))?,
    );
    let session = console.documents.session(id)?;
    if let Some(expected) = args.get("expect_head") {
        let expected: Option<HistoryEntryId> = serde_json::from_value(expected.clone())?;
        if session.history.head != expected {
            return Err(EngineError::Conflict {
                message: "document head changed".into(),
            });
        }
    }
    let rationale = args
        .get("rationale")
        .cloned()
        .map(serde_json::from_value::<Option<String>>)
        .transpose()?
        .flatten();
    let object = args.as_object_mut().unwrap();
    object.remove("expect_head");
    object.remove("rationale");
    if name == "undo" || name == "redo" {
        let changed = if name == "undo" {
            console.documents.undo(id)?
        } else {
            console.documents.redo(id)?
        };
        return Ok(
            json!({"document":id,"changed":changed,"head":console.documents.session(id)?.history.head}),
        );
    }
    let mut action_args = args.clone();
    let op = if name == "paint_preset" {
        let preset_id = args["preset_id"]
            .as_u64()
            .ok_or_else(|| EngineError::invalid("preset_id", "unsigned id required"))?;
        let preset = brush_presets::BrushPresetStore::open(&console.app)?
            .get(brush_presets::BrushPresetId(preset_id))?;
        let seed = args
            .get("seed")
            .map(|v| {
                v.as_u64()
                    .ok_or_else(|| EngineError::invalid("seed", "unsigned integer required"))
            })
            .transpose()?
            .unwrap_or(0);
        action_args["brush_snapshot"] = serde_json::to_value(&preset.brush)?;
        let layer = LayerId(
            args["layer"]
                .as_u64()
                .ok_or_else(|| EngineError::invalid("layer", "unsigned id required"))?,
        );
        let points: Vec<StrokePoint> = serde_json::from_value(args["points"].clone())?;
        let target: StrokeTarget =
            serde_json::from_value(args.get("target").cloned().unwrap_or(json!("pixels")))?;
        let engine = paint::PresetBrush::new(preset.brush, seed)?;
        let old = std::mem::replace(&mut console.documents.brush, Box::new(engine));
        let result = console.documents.paint(
            console.documents.session(id)?,
            layer,
            &points,
            &BrushParams::default(),
            target,
        );
        console.documents.brush = old;
        result?
    } else {
        let s = console
            .documents
            .sessions
            .get(&id)
            .expect("validated session");
        let current = s.state().selection.as_deref();
        let raster = match name {
            "select_advanced" => advanced::compute_with_model(
                &s.doc,
                current,
                &serde_json::from_value(args.clone())?,
                console
                    .documents
                    .segment_model
                    .as_mut()
                    .map(|m| m.as_mut() as &mut dyn selection::ml::SegmentModel),
            )?,
            "refine_edge" => {
                advanced::compute_refine(&s.doc, current, &serde_json::from_value(args.clone())?)?
            }
            "selection_boolean" => {
                let p: advanced::SelectionBoolean = serde_json::from_value(args.clone())?;
                let saved = s.saved_selections();
                let operand = saved
                    .iter()
                    .find(|v| v.id.0 == p.selection)
                    .ok_or_else(|| EngineError::not_found("selection", p.selection))?;
                advanced::compute_boolean(&s.doc, current, &operand.mask, &p)?
            }
            _ => return Err(crate::unsupported(name)),
        };
        DocOp::SetSelection {
            selection: Some(raster),
        }
    };
    let s = console.documents.session_mut(id)?;
    let applied = s.doc.apply(op)?;
    let entry = s.history.record(
        Action::new(name, action_args.as_object().unwrap().clone()),
        EditMeta {
            label: name.into(),
            author: Author::Agent {
                name: "tessera-mcp".into(),
            },
            timestamp_ms: crate::console::now(),
            group: None,
            rationale,
        },
    );
    s.nodes.push(applied.node);

    Ok(json!({"document":id,"entry":entry}))
}
