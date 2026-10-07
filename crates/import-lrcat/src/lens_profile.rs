//! ENG-7: Lightroom profile corrections Tessera cannot supply.
//!
//! `LensProfileEnable = 0` maps to `LensProfileSource::None`, with no note.
//! `LensProfileEnable = 1` maps to the named `Database` profile, or to `Auto`
//! without a profile identity. Tessera has no Adobe lens-profile (LCP)
//! database, so an enabled profile is rendered as Lightroom renders a missing
//! one: no profile correction (`Auto` still applies a camera-embedded
//! correction when the raw carries one). It is never replaced by a distortion
//! estimated from image content. One info-level note says so; the profile
//! identity stays in the recipe for export and for a future profile database.
use crate::lua_develop::{LuaKey, LuaTable, LuaValue};
use engine_api::recipe::{Recipe, settings::LensProfileSource};

const KEY: &str = "LensProfileEnable";
const FIELD: &str = "/settings/lens/profile";
const LANE: &str = "ENG-7";

fn enabled(table: &LuaTable) -> bool {
    let mut values = table.fields.iter().filter_map(|(k, v)| match k {
        LuaKey::Str(k) if k == KEY => Some(v),
        _ => None,
    });
    let (Some(v), None) = (values.next(), values.next()) else {
        return false;
    };
    match v {
        LuaValue::Bool(b) => *b,
        LuaValue::Number(n) => n.parse::<f64>().is_ok_and(|n| n != 0.),
        _ => false,
    }
}

/// Record the info note for an enabled Lightroom profile correction.
pub(crate) fn note_unavailable(table: &LuaTable, recipe: &mut Recipe) {
    if !enabled(table) {
        return;
    }
    let reason = match &recipe.settings.lens.profile {
        LensProfileSource::Database { .. } => {
            "lens profile unavailable: Tessera has no Adobe lens profile database, so the named \
             profile is not applied and no profile correction is rendered; Tessera never \
             substitutes a correction estimated from image content; the profile identity is kept"
        }
        LensProfileSource::Auto => {
            "automatic lens profile unavailable: Tessera has no Adobe lens profile database; a \
             correction embedded in the raw is applied when present, otherwise no profile \
             correction is rendered; Tessera never substitutes a correction estimated from \
             image content"
        }
        _ => return,
    };
    crate::diagnostics::push_approximate(recipe, KEY, FIELD, LANE, reason);
}
