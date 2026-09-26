//! Action descriptors (spec 02 §11): the serializable record of one
//! command, `{"command": <name>, "params": {…}}`.
//!
//! Every tool call, recipe ([`ToolCall`]) or layered-document
//! ([`DocumentToolCall`]), converts losslessly to an [`Action`] and back, so
//! recording an action is capturing descriptors, and Actions, batch and
//! droplets can replay them later. `command` is a name from the stable
//! registry [`COMMANDS`]; `params` is the call's flat JSON object minus the
//! `"tool"` tag, with keys sorted.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::error::{EngineError, EngineResult};
use crate::tools::{DocumentToolCall, LibraryToolCall, ToolCall};

/// Which surface a command belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CommandDomain {
    /// Per-image recipe and library commands ([`ToolCall`]).
    Recipe,
    /// Layered-document commands ([`DocumentToolCall`]).
    Document,
    /// Catalog-scoped identity commands ([`LibraryToolCall`]).
    Library,
}

/// What running a command does.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CommandEffect {
    /// Changes an edit document and records exactly one history entry.
    Edit,
    /// Has side effects outside history (indexing, selection flags, export,
    /// opening a document).
    Effect,
    /// Reads only.
    Query,
}

/// One registry row.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
pub struct CommandInfo {
    /// Stable command (tool) name.
    pub name: &'static str,
    /// Surface.
    pub domain: CommandDomain,
    /// Effect.
    pub effect: CommandEffect,
}

const fn cmd(name: &'static str, domain: CommandDomain, effect: CommandEffect) -> CommandInfo {
    CommandInfo {
        name,
        domain,
        effect,
    }
}

/// The command-name registry. Names are stable once shipped (invariant 11):
/// rows are only ever appended, never renamed or removed.
pub const COMMANDS: [CommandInfo; 33] = {
    use CommandDomain::{Document as D, Library as L, Recipe as R};
    use CommandEffect::{Edit, Effect, Query};
    [
        cmd("set_tone", R, Edit),
        cmd("create_mask", R, Edit),
        cmd("adjust_mask", R, Edit),
        cmd("remove_object", R, Edit),
        cmd("retouch_skin", R, Edit),
        cmd("apply_style", R, Edit),
        cmd("crop", R, Edit),
        cmd("compare", R, Query),
        cmd("get_histogram", R, Query),
        cmd("get_scores", R, Query),
        cmd("index_folder", R, Effect),
        cmd("set_selection", R, Effect),
        cmd("export", R, Effect),
        cmd("open_document", D, Effect),
        cmd("add_layer", D, Edit),
        cmd("set_layer_props", D, Edit),
        cmd("paint_stroke", D, Edit),
        cmd("set_pixel_selection", D, Edit),
        cmd("apply_adjustment_layer", D, Edit),
        cmd("transform_layer", D, Edit),
        cmd("merge_down", D, Edit),
        cmd("export_document", D, Effect),
        cmd("list_layers", D, Query),
        cmd("add_channel", D, Edit),
        cmd("delete_channel", D, Edit),
        cmd("rename_channel", D, Edit),
        cmd("edit_channel", D, Edit),
        cmd("load_channel_as_selection", D, Edit),
        cmd("assign_person", L, Effect),
        cmd("confirm_person", L, Effect),
        cmd("merge_people", L, Effect),
        cmd("split_person", L, Effect),
        cmd("name_person", L, Effect),
    ]
};

/// Registry row for `name`.
pub fn command(name: &str) -> Option<&'static CommandInfo> {
    COMMANDS.iter().find(|c| c.name == name)
}

/// A decoded action.
#[derive(Debug, Clone, PartialEq)]
pub enum ActionCall {
    /// A recipe/library tool call.
    Recipe(ToolCall),
    /// A layered-document tool call.
    Document(DocumentToolCall),
    /// A catalog people tool call.
    Library(LibraryToolCall),
}

/// A serializable command descriptor.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct Action {
    /// Command name from [`COMMANDS`].
    pub command: String,
    /// Parameters: the call's JSON members other than `"tool"`.
    #[serde(default)]
    pub params: BTreeMap<String, Value>,
}

impl Action {
    /// Builds an action from a name and parameters (not validated; see
    /// [`Action::decode`]).
    pub fn new<K: Into<String>>(
        command: impl Into<String>,
        params: impl IntoIterator<Item = (K, Value)>,
    ) -> Self {
        Self {
            command: command.into(),
            params: params.into_iter().map(|(k, v)| (k.into(), v)).collect(),
        }
    }

    fn from_tagged(value: Value) -> EngineResult<Self> {
        let Value::Object(mut map) = value else {
            return Err(EngineError::internal(
                "tool call did not serialize to an object",
            ));
        };
        let Some(Value::String(command)) = map.remove("tool") else {
            return Err(EngineError::internal("tool call has no `tool` tag"));
        };
        Ok(Self {
            command,
            params: map.into_iter().collect(),
        })
    }

    /// The descriptor of a recipe/library tool call.
    pub fn from_tool(call: &ToolCall) -> EngineResult<Self> {
        Self::from_tagged(serde_json::to_value(call)?)
    }

    /// The descriptor of a layered-document tool call.
    pub fn from_document_tool(call: &DocumentToolCall) -> EngineResult<Self> {
        Self::from_tagged(serde_json::to_value(call)?)
    }

    /// The descriptor of a catalog people tool call.
    pub fn from_library_tool(call: &LibraryToolCall) -> EngineResult<Self> {
        Self::from_tagged(serde_json::to_value(call)?)
    }

    /// Registry row, if the command is registered.
    pub fn info(&self) -> Option<&'static CommandInfo> {
        command(&self.command)
    }

    fn tagged(&self) -> Value {
        let mut map: Map<String, Value> = self
            .params
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        map.insert("tool".into(), Value::String(self.command.clone()));
        Value::Object(map)
    }

    /// Decodes into the typed call. Fails with `InvalidArgument` for an
    /// unregistered command or parameters that do not parse.
    pub fn decode(&self) -> EngineResult<ActionCall> {
        let info = self.info().ok_or_else(|| {
            EngineError::invalid("command", format!("unknown command `{}`", self.command))
        })?;
        let bad = |e: serde_json::Error| EngineError::invalid("params", e.to_string());
        Ok(match info.domain {
            CommandDomain::Recipe => {
                ActionCall::Recipe(serde_json::from_value(self.tagged()).map_err(bad)?)
            }
            CommandDomain::Document => {
                ActionCall::Document(serde_json::from_value(self.tagged()).map_err(bad)?)
            }
            CommandDomain::Library => {
                ActionCall::Library(serde_json::from_value(self.tagged()).map_err(bad)?)
            }
        })
    }
}

impl TryFrom<&ToolCall> for Action {
    type Error = EngineError;
    fn try_from(call: &ToolCall) -> EngineResult<Self> {
        Self::from_tool(call)
    }
}

impl TryFrom<&DocumentToolCall> for Action {
    type Error = EngineError;
    fn try_from(call: &DocumentToolCall) -> EngineResult<Self> {
        Self::from_document_tool(call)
    }
}

impl TryFrom<&LibraryToolCall> for Action {
    type Error = EngineError;
    fn try_from(call: &LibraryToolCall) -> EngineResult<Self> {
        Self::from_library_tool(call)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn registry_matches_tool_enums() {
        let names: Vec<&str> = COMMANDS.iter().map(|c| c.name).collect();
        let expected: Vec<&str> = ToolCall::NAMES
            .iter()
            .chain(DocumentToolCall::NAMES.iter())
            .chain(LibraryToolCall::NAMES.iter())
            .copied()
            .collect();
        assert_eq!(names, expected);
        let mut unique = names.clone();
        unique.sort();
        unique.dedup();
        assert_eq!(
            unique.len(),
            names.len(),
            "command names are globally unique"
        );
        for c in &COMMANDS[..ToolCall::NAMES.len()] {
            assert_eq!(c.domain, CommandDomain::Recipe);
        }
        let library_start = ToolCall::NAMES.len() + DocumentToolCall::NAMES.len();
        for c in &COMMANDS[ToolCall::NAMES.len()..library_start] {
            assert_eq!(c.domain, CommandDomain::Document);
        }
        for c in &COMMANDS[library_start..] {
            assert_eq!(c.domain, CommandDomain::Library);
        }
    }

    #[test]
    fn registry_effects_match_call_predicates() {
        for call in crate::tools::tests::all_calls() {
            let info = command(call.name()).unwrap();
            assert_eq!(
                info.effect == CommandEffect::Edit,
                call.edits_recipe(),
                "{}",
                call.name()
            );
            assert_eq!(
                info.effect == CommandEffect::Query,
                call.is_read_only(),
                "{}",
                call.name()
            );
            let a = Action::from_tool(&call).unwrap();
            assert_eq!(a.command, call.name());
            assert!(!a.params.contains_key("tool"));
            let v = serde_json::to_value(&a).unwrap();
            let back: Action = serde_json::from_value(v).unwrap();
            assert_eq!(back.decode().unwrap(), ActionCall::Recipe(call));
        }
        for call in crate::tools::tests::all_document_calls() {
            let info = command(call.name()).unwrap();
            assert_eq!(
                info.effect == CommandEffect::Edit,
                call.edits_document(),
                "{}",
                call.name()
            );
            assert_eq!(
                info.effect == CommandEffect::Query,
                call.is_read_only(),
                "{}",
                call.name()
            );
            let a = Action::try_from(&call).unwrap();
            let v = serde_json::to_value(&a).unwrap();
            let back: Action = serde_json::from_value(v).unwrap();
            assert_eq!(back.decode().unwrap(), ActionCall::Document(call));
        }
    }

    #[test]
    fn descriptor_form_is_stable() {
        let a = Action::new("merge_down", [("layer", json!(4)), ("document", json!(1))]);
        assert_eq!(
            serde_json::to_string(&a).unwrap(),
            r#"{"command":"merge_down","params":{"document":1,"layer":4}}"#
        );
        assert!(matches!(a.decode().unwrap(), ActionCall::Document(_)));
        assert!(Action::new("no_such", Vec::<(String, Value)>::new())
            .decode()
            .is_err());
        let bad = Action::new("merge_down", [("document", json!("x"))]);
        assert!(matches!(
            bad.decode(),
            Err(EngineError::InvalidArgument { .. })
        ));
    }
}
