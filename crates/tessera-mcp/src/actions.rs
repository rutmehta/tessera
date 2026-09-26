//! Actions (spec 02 §11): recording executed tool calls as [`Action`]
//! descriptors, the `.tessera-action` file format, and playback on other
//! documents (batch).
//!
//! # File format
//!
//! A `.tessera-action` file is UTF-8 JSON:
//!
//! ```json
//! {
//!   "format": "tessera-action",
//!   "version": 1,
//!   "name": "Warm grade",
//!   "inputs": 1,
//!   "steps": [
//!     {"command": "apply_adjustment_layer",
//!      "params": {"document": {"$input": 0}, "adjustment": {"kind": "invert"}},
//!      "rationale": "Invert for the negative look"},
//!     {"command": "set_layer_props",
//!      "params": {"document": {"$input": 0}, "layer": {"$layer": 0}, "opacity": 0.5}}
//!   ]
//! }
//! ```
//!
//! Each step is an engine-api [`Action`] (`command` from
//! [`engine_api::action::COMMANDS`], `params` the call's JSON members) plus
//! an optional `rationale` and an `enabled` flag (default `true`). Values
//! that name session objects are symbolic so an action replays on any
//! document:
//!
//! - `{"$input": i}`: the i-th document the action is played on
//!   (`0 ≤ i < inputs`);
//! - `{"$doc": k}`: the document opened by step `k`;
//! - `{"$layer": k}`: the layer created by step `k` (`add_layer`,
//!   `apply_adjustment_layer`);
//! - `{"$selection": k}`: the selection saved by step `k` (`save_as`).
//! - `{"$channel": k}`: the channel created by step `k` (`add_channel` or `save_as`).
//!
//! References only point at earlier steps. Layer ids that existed before
//! recording started are recorded literally (for example `1`, the
//! Background of an opened flat image). Recipe tool calls are recorded with
//! their literal parameters. Read-only calls (`list_layers`,
//! `get_histogram`, `compare`, `get_scores`) are not recorded.
use std::collections::BTreeMap;
use std::path::Path;

use engine_api::action::{Action, ActionCall, CommandEffect};
use engine_api::id::{ChannelId, DocumentId, LayerId, SelectionId};
use engine_api::tools::{
    DocumentToolCall, DocumentToolOutput, DocumentToolRequest, DocumentToolResponse,
    LibraryToolRequest, ToolRequest, ToolResponse,
};
use engine_api::{EngineError, EngineResult};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

/// The `format` member of every action file.
pub const ACTION_FORMAT: &str = "tessera-action";
/// Current file version.
pub const ACTION_VERSION: u32 = 1;
/// File extension.
pub const ACTION_EXTENSION: &str = "tessera-action";

fn yes() -> bool {
    true
}

/// One recorded step.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ActionStep {
    /// Command and (symbolic) parameters.
    #[serde(flatten)]
    pub action: Action,
    /// Rationale recorded with the original call.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rationale: Option<String>,
    /// Disabled steps are skipped on playback.
    #[serde(default = "yes")]
    pub enabled: bool,
}

/// A `.tessera-action` file.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ActionFile {
    /// Always [`ACTION_FORMAT`].
    pub format: String,
    /// File version ([`ACTION_VERSION`]).
    pub version: u32,
    /// Display name.
    pub name: String,
    /// Number of documents the action is played on.
    #[serde(default)]
    pub inputs: u32,
    /// Steps in order.
    pub steps: Vec<ActionStep>,
}

const REFS: [&str; 5] = ["$input", "$doc", "$layer", "$selection", "$channel"];

/// A symbolic reference, if `v` is one.
fn reference(v: &Value) -> Option<(&str, u64)> {
    let obj = v.as_object()?;
    if obj.len() != 1 {
        return None;
    }
    let (k, n) = obj.iter().next()?;
    REFS.contains(&k.as_str()).then_some(())?;
    Some((k.as_str(), n.as_u64()?))
}

impl ActionFile {
    /// An empty action.
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            format: ACTION_FORMAT.into(),
            version: ACTION_VERSION,
            name: name.into(),
            inputs: 0,
            steps: Vec::new(),
        }
    }

    /// Checks the header, command names and that every reference points at
    /// an input or at an earlier step that produces that kind of object.
    pub fn validate(&self) -> EngineResult<()> {
        if self.format != ACTION_FORMAT {
            return Err(EngineError::invalid(
                "format",
                format!("expected `{ACTION_FORMAT}`"),
            ));
        }
        if self.version == 0 || self.version > ACTION_VERSION {
            return Err(EngineError::SchemaVersion {
                document: ACTION_FORMAT.into(),
                found: self.version,
                supported: ACTION_VERSION,
            });
        }
        for (k, step) in self.steps.iter().enumerate() {
            if step.action.info().is_none() {
                return Err(EngineError::invalid(
                    "command",
                    format!("step {k}: unknown command `{}`", step.action.command),
                ));
            }
            for (key, v) in &step.action.params {
                if let Some(obj) = v.as_object()
                    && obj.keys().any(|k| k.starts_with('$'))
                    && reference(v).is_none()
                {
                    return Err(EngineError::invalid(
                        key.as_str(),
                        format!("step {k}: malformed reference {v}"),
                    ));
                }
                let Some((kind, n)) = reference(v) else {
                    continue;
                };
                let producer = |want: &[&str]| -> EngineResult<()> {
                    let target = usize::try_from(n).ok().filter(|t| *t < k).ok_or_else(|| {
                        EngineError::invalid(
                            key.as_str(),
                            format!("step {k}: {kind} {n} is not an earlier step"),
                        )
                    })?;
                    let cmd = self.steps[target].action.command.as_str();
                    if want.contains(&cmd) {
                        Ok(())
                    } else {
                        Err(EngineError::invalid(
                            key.as_str(),
                            format!("step {k}: step {n} (`{cmd}`) produces no {kind}"),
                        ))
                    }
                };
                match kind {
                    "$input" if n < u64::from(self.inputs) => {}
                    "$input" => {
                        return Err(EngineError::invalid(
                            key.as_str(),
                            format!("step {k}: input {n} of {}", self.inputs),
                        ));
                    }
                    "$doc" => producer(&["open_document"])?,
                    "$layer" => producer(&["add_layer", "apply_adjustment_layer"])?,
                    "$channel" => producer(&["add_channel", "set_pixel_selection"])?,
                    _ => producer(&["set_pixel_selection", "add_channel"])?,
                }
            }
        }
        Ok(())
    }

    /// Parses and validates JSON.
    pub fn from_json(text: &str) -> EngineResult<Self> {
        let file: Self = serde_json::from_str(text)
            .map_err(|e| EngineError::invalid("action", e.to_string()))?;
        file.validate()?;
        Ok(file)
    }

    /// Reads and validates a file.
    pub fn read(path: impl AsRef<Path>) -> EngineResult<Self> {
        let path = path.as_ref();
        let text = std::fs::read_to_string(path).map_err(|e| EngineError::io_at(path, &e))?;
        Self::from_json(&text)
    }

    /// Writes pretty JSON (atomically).
    pub fn write(&self, path: impl AsRef<Path>) -> EngineResult<()> {
        self.validate()?;
        let path = path.as_ref();
        let tmp = path.with_extension("tessera-action.tmp");
        let text = serde_json::to_string_pretty(self)?;
        std::fs::write(&tmp, text).map_err(|e| EngineError::io_at(&tmp, &e))?;
        std::fs::rename(&tmp, path).map_err(|e| EngineError::io_at(path, &e))
    }
}

/// Records executed calls into an [`ActionFile`].
#[derive(Debug, Clone)]
pub struct Recorder {
    file: ActionFile,
    inputs: BTreeMap<DocumentId, u64>,
    opened: BTreeMap<DocumentId, u64>,
    layers: BTreeMap<(DocumentId, LayerId), u64>,
    selections: BTreeMap<(DocumentId, SelectionId), u64>,
    channels: BTreeMap<(DocumentId, ChannelId), u64>,
}

impl Recorder {
    /// Starts an empty recording.
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            file: ActionFile::new(name),
            inputs: BTreeMap::new(),
            opened: BTreeMap::new(),
            layers: BTreeMap::new(),
            selections: BTreeMap::new(),
            channels: BTreeMap::new(),
        }
    }

    /// Steps so far.
    pub fn len(&self) -> usize {
        self.file.steps.len()
    }

    /// True before the first step.
    pub fn is_empty(&self) -> bool {
        self.file.steps.is_empty()
    }

    /// Ends the recording.
    pub fn finish(self) -> ActionFile {
        self.file
    }

    fn symbolic_document(&mut self, doc: DocumentId) -> Value {
        if let Some(k) = self.opened.get(&doc) {
            return json!({"$doc": k});
        }
        let next = self.inputs.len() as u64;
        let i = *self.inputs.entry(doc).or_insert(next);
        self.file.inputs = self.inputs.len() as u32;
        json!({"$input": i})
    }

    /// Records a successful layered-document call.
    pub fn record_document(&mut self, request: &DocumentToolRequest, output: &DocumentToolOutput) {
        if request.call.is_read_only() {
            return;
        }
        let Ok(mut action) = Action::from_document_tool(&request.call) else {
            return;
        };
        let step = self.file.steps.len() as u64;
        if let Some(doc) = request.call.document() {
            action
                .params
                .insert("document".into(), self.symbolic_document(doc));
            for key in ["layer", "parent", "above"] {
                if let Some(id) = action.params.get(key).and_then(Value::as_u64)
                    && let Some(k) = self.layers.get(&(doc, LayerId(id)))
                {
                    action.params.insert(key.into(), json!({"$layer": k}));
                }
            }
            if let Some(id) = action.params.get("channel").and_then(Value::as_u64)
                && let Some(k) = self.channels.get(&(doc, ChannelId(id)))
            {
                action
                    .params
                    .insert("channel".into(), json!({"$channel": k}));
            }
            if let Some(id) = action.params.get("selection").and_then(Value::as_u64)
                && let Some(k) = self.selections.get(&(doc, SelectionId(id)))
            {
                action
                    .params
                    .insert("selection".into(), json!({"$selection": k}));
            }
        }
        match output {
            DocumentToolOutput::DocumentOpened { document, .. } => {
                self.opened.insert(*document, step);
            }
            DocumentToolOutput::DocumentEdited {
                document,
                layer,
                selection,
                channel,
                ..
            } => {
                if matches!(
                    request.call,
                    DocumentToolCall::AddLayer { .. }
                        | DocumentToolCall::ApplyAdjustmentLayer { .. }
                ) && let Some(l) = layer
                {
                    self.layers.insert((*document, *l), step);
                }
                if let Some(s) = selection {
                    self.selections.insert((*document, *s), step);
                }
                if matches!(
                    request.call,
                    DocumentToolCall::AddChannel { .. }
                        | DocumentToolCall::SetPixelSelection {
                            save_as: Some(_),
                            ..
                        }
                ) && let Some(c) = channel
                {
                    self.channels.insert((*document, *c), step);
                }
            }
            _ => {}
        }
        self.file.steps.push(ActionStep {
            action,
            rationale: request.rationale.clone(),
            enabled: true,
        });
    }

    /// Records a successful catalog people call (literal references).
    pub fn record_library(&mut self, request: &LibraryToolRequest) {
        if let Ok(action) = Action::from_library_tool(&request.call) {
            self.file.steps.push(ActionStep {
                action,
                rationale: request.rationale.clone(),
                enabled: true,
            });
        }
    }

    /// Records a successful recipe/library call (literal parameters).
    pub fn record_tool(&mut self, request: &ToolRequest) {
        if request.call.is_read_only() {
            return;
        }
        if let Ok(action) = Action::from_tool(&request.call)
            && action
                .info()
                .is_some_and(|i| i.effect != CommandEffect::Query)
        {
            self.file.steps.push(ActionStep {
                action,
                rationale: request.rationale.clone(),
                enabled: true,
            });
        }
    }
}

/// What one played step produced.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct StepOutput {
    /// Step index.
    pub step: usize,
    /// Command.
    pub command: String,
    /// Skipped (disabled).
    pub skipped: bool,
    /// Engine output (`{"ok": …}` payload).
    #[serde(skip_serializing_if = "Value::is_null")]
    pub output: Value,
    #[serde(skip)]
    document: Option<DocumentId>,
    #[serde(skip)]
    layer: Option<LayerId>,
    #[serde(skip)]
    selection: Option<SelectionId>,
    #[serde(skip)]
    channel: Option<ChannelId>,
}

/// Result of playing an action: every step that ran, and the failure that
/// stopped playback, if any. Steps before a failure stay applied (each is
/// its own history entry and can be undone).
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct PlayReport {
    /// Action name.
    pub action: String,
    /// Documents played on.
    pub inputs: Vec<DocumentId>,
    /// Steps that ran (or were skipped), in order.
    pub steps: Vec<StepOutput>,
    /// First failing step and its error.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub failed: Option<PlayFailure>,
}

/// A failed step.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct PlayFailure {
    /// Step index.
    pub step: usize,
    /// The engine error.
    pub error: EngineError,
}

impl PlayReport {
    /// True if every step ran.
    pub fn ok(&self) -> bool {
        self.failed.is_none()
    }

    /// Documents opened by `open_document` steps.
    pub fn opened(&self) -> Vec<DocumentId> {
        self.steps
            .iter()
            .filter(|s| s.command == "open_document")
            .filter_map(|s| s.document)
            .collect()
    }
}

/// Executes decoded calls for [`play`]; implemented by the console.
pub trait ActionTarget {
    /// Runs a layered-document request.
    fn run_document(&mut self, request: DocumentToolRequest) -> DocumentToolResponse;
    /// Runs a recipe/library request.
    fn run_tool(&mut self, request: ToolRequest) -> ToolResponse;
    /// Runs a catalog people request. Legacy targets explicitly reject these calls.
    fn run_library(&mut self, _request: LibraryToolRequest) -> EngineResult<Value> {
        Err(crate::unsupported(
            "catalog people calls are unavailable on this target",
        ))
    }
}

fn substitute(
    file: &ActionFile,
    step: usize,
    inputs: &[DocumentId],
    done: &[StepOutput],
) -> EngineResult<Action> {
    let mut action = file.steps[step].action.clone();
    for (key, v) in action.params.iter_mut() {
        let Some((kind, n)) = reference(v) else {
            continue;
        };
        let missing = || {
            EngineError::invalid(
                key.as_str(),
                format!("step {step}: {kind} {n} did not produce a value"),
            )
        };
        let producer = || {
            done.get(n as usize)
                .filter(|s| !s.skipped)
                .ok_or_else(missing)
        };
        *v = match kind {
            "$input" => json!(inputs.get(n as usize).ok_or_else(missing)?),
            "$doc" => json!(producer()?.document.ok_or_else(missing)?),
            "$layer" => json!(producer()?.layer.ok_or_else(missing)?),
            "$channel" => json!(producer()?.channel.ok_or_else(missing)?),
            _ => json!(producer()?.selection.ok_or_else(missing)?),
        };
    }
    Ok(action)
}

/// Plays `file` on `inputs` (one document per `$input`). Stops at the first
/// failing step. Every document step is recorded as an Agent history entry
/// whose rationale names the action and step.
pub fn play(
    target: &mut impl ActionTarget,
    file: &ActionFile,
    inputs: &[DocumentId],
) -> EngineResult<PlayReport> {
    file.validate()?;
    if inputs.len() != file.inputs as usize {
        return Err(EngineError::invalid(
            "documents",
            format!(
                "action `{}` takes {} document(s), got {}",
                file.name,
                file.inputs,
                inputs.len()
            ),
        ));
    }
    let mut report = PlayReport {
        action: file.name.clone(),
        inputs: inputs.to_vec(),
        ..Default::default()
    };
    for (k, step) in file.steps.iter().enumerate() {
        let mut out = StepOutput {
            step: k,
            command: step.action.command.clone(),
            ..Default::default()
        };
        if !step.enabled {
            out.skipped = true;
            report.steps.push(out);
            continue;
        }
        let rationale = Some(match &step.rationale {
            Some(r) => format!("{r} (action `{}`, step {})", file.name, k + 1),
            None => format!("action `{}`, step {}", file.name, k + 1),
        });
        let result = substitute(file, k, inputs, &report.steps)
            .and_then(|a| a.decode())
            .and_then(|call| match call {
                ActionCall::Document(call) => {
                    match target.run_document(DocumentToolRequest {
                        call,
                        rationale,
                        group: None,
                        expect_head: None,
                    }) {
                        DocumentToolResponse::Ok(o) => {
                            match &o {
                                DocumentToolOutput::DocumentOpened { document, .. } => {
                                    out.document = Some(*document);
                                }
                                DocumentToolOutput::DocumentEdited {
                                    document,
                                    layer,
                                    selection,
                                    channel,
                                    ..
                                } => {
                                    out.document = Some(*document);
                                    out.layer = *layer;
                                    out.selection = *selection;
                                    out.channel = *channel;
                                }
                                _ => {}
                            }
                            Ok(serde_json::to_value(o)?)
                        }
                        DocumentToolResponse::Error(e) => Err(e),
                    }
                }
                ActionCall::Library(call) => {
                    target.run_library(LibraryToolRequest { call, rationale })
                }
                ActionCall::Recipe(call) => match target.run_tool(ToolRequest {
                    call,
                    rationale,
                    group: None,
                    expect_recipe: None,
                }) {
                    ToolResponse::Ok(o) => Ok(serde_json::to_value(o)?),
                    ToolResponse::Error(e) => Err(e),
                },
            });
        match result {
            Ok(v) => {
                out.output = v;
                report.steps.push(out);
            }
            Err(error) => {
                report.failed = Some(PlayFailure { step: k, error });
                break;
            }
        }
    }
    Ok(report)
}
