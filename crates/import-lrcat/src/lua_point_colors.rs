//! Normalize only PointColors' explicit Lua array keys, without changing other
//! table rendering or touching the source spans retained by the caller.
use super::{LuaKey, LuaTable};
use std::{borrow::Cow, collections::BTreeMap};

pub(super) fn sequence(table: &LuaTable) -> Result<Cow<'_, LuaTable>, String> {
    if table.fields.is_empty() {
        return Ok(Cow::Borrowed(table));
    }
    let invalid = || "PointColors requires a contiguous, unmixed array".to_string();
    if !table.items.is_empty() {
        return Err(invalid());
    }
    let mut ordered = BTreeMap::new();
    for (key, value) in &table.fields {
        let LuaKey::Num(key) = key else {
            return Err(invalid());
        };
        let index = key.parse::<f64>().map_err(|_| invalid())?;
        if index < 1.
            || index > table.fields.len() as f64
            || index.fract() != 0.
            || ordered.insert(index as usize, value).is_some()
        {
            return Err(invalid());
        }
    }
    Ok(Cow::Owned(LuaTable {
        items: ordered.into_values().cloned().collect(),
        fields: Vec::new(),
    }))
}
