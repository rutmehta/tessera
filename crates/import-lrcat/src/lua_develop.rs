//! Data-only reader for LrC 15.5 develop settings stored as a Lua table
//! literal (`s = { ... }`). RED stub for B5-29b.
use engine_api::{EngineError, EngineResult, recipe::Recipe};

/// Maximum table nesting.
pub const MAX_DEPTH: usize = 32;
/// Maximum literal size in bytes.
pub const MAX_INPUT_BYTES: usize = 4 << 20;
/// Maximum number of values in one literal.
pub const MAX_VALUES: usize = 500_000;

/// Lua develop key ↔ crs local name.
pub const KEY_MAP: &[(&str, &str)] = &[];

/// A parsed literal value.
#[derive(Debug, Clone, PartialEq)]
pub enum LuaValue {
    Nil,
}

pub fn read(_text: &str) -> EngineResult<LuaValue> {
    Err(EngineError::Decode {
        format: "lightroom-develop-lua".into(),
        message: "not implemented".into(),
    })
}

pub fn parse(_text: &str, _process_version: &str) -> EngineResult<(Recipe, Vec<String>)> {
    Err(EngineError::Decode {
        format: "lightroom-develop-lua".into(),
        message: "not implemented".into(),
    })
}
