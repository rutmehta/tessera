//! Layer styles (Layer ▸ Layer Style) and the document's Global Light on a
//! [`DocumentSession`] (WP B5-07).
//!
//! A layer's styles cross the bridge as `compositor::render::styles::LayerStyles`
//! JSON (`{"effects":[{"kind":"drop_shadow","settings":{…}}, …],"scale":1}`),
//! the same serde shape `.tessera-doc` stores, so every field the engine
//! keeps (including contour and jitter metadata that the renderer does not
//! evaluate) survives a read-modify-write in the app. Edits go through
//! `DocOp::SetProps` with the layer's other properties (opacity, fill, blend
//! mode, Blend If, knockout, locks, …) taken from the live state, so only the
//! styles change. `interactive: true` edits (a slider drag in the Layer
//! Style inspector) are live on the scratch document until
//! [`DocumentSession::commit`], like `set_opacity`; styles and the other
//! properties of one layer share one pending key, so a drag that touches
//! both still records one net node.
//!
//! [`style_effects_schema_json`] describes each effect kind, its fields and
//! their ranges so the app's editors are generated from the engine's model.

use super::{DocumentSession, DocumentUpdate, Pending, batch, find};
use crate::{Result, failure};
use compositor::{
    DocOp, GroupMode, Layer, LayerId, LayerKind,
    render::styles::{
        Bevel, GlobalLight, Glow, LayerStyles, Overlay, Satin, Shadow, Stroke, StyleEffect,
    },
};
use serde_json::{Value, json};
use std::sync::Mutex;

/// Pending key of an interactive Global Light drag. Layer ids count up from
/// 1 and never reach it, so it cannot collide with a layer's props key.
const GLOBAL_LIGHT_KEY: u64 = u64::MAX;

/// Layer ▸ Layer Style ▸ Copy Layer Style: application-wide, like
/// Photoshop's, so styles paste across documents.
static STYLE_CLIPBOARD: Mutex<Option<LayerStyles>> = Mutex::new(None);

/// The document's light: `angle` in degrees (0 lights from the right, 90
/// from above), `altitude` (elevation) in `0…90` degrees.
#[derive(Clone, Copy, Debug, PartialEq, uniffi::Record)]
pub struct GlobalLightRecord {
    pub angle: f32,
    pub altitude: f32,
}

/// One effect of a styled layer, for the Layers panel's effect rows.
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct StyleEffectSummary {
    /// Index into the layer's `effects` (the JSON array).
    pub index: u32,
    /// Serde kind name (`drop_shadow`, `stroke`, …).
    pub kind: String,
    pub enabled: bool,
}

/// The effects of one styled layer in the engine's stacking order, **top
/// first** (the order the Layers panel and the inspector list them).
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct LayerStyleSummary {
    pub layer: u64,
    pub effects: Vec<StyleEffectSummary>,
}

/// Stacking rank of an effect kind (0 = bottom), as `render::styles` composites them.
fn rank(e: &StyleEffect) -> u8 {
    match e {
        StyleEffect::DropShadow(_) => 0,
        StyleEffect::OuterGlow(_) => 1,
        StyleEffect::PatternOverlay(_) => 2,
        StyleEffect::GradientOverlay(_) => 3,
        StyleEffect::ColorOverlay(_) | StyleEffect::Overlay(_) => 4,
        StyleEffect::Satin(_) => 5,
        StyleEffect::InnerGlow(_) => 6,
        StyleEffect::InnerShadow(_) => 7,
        StyleEffect::Stroke(_) => 8,
        StyleEffect::Bevel(_) => 9,
    }
}

fn kind_name(e: &StyleEffect) -> String {
    serde_json::to_value(e)
        .ok()
        .and_then(|v| v.get("kind").and_then(Value::as_str).map(str::to_owned))
        .unwrap_or_default()
}

fn enabled(e: &StyleEffect) -> bool {
    match e {
        StyleEffect::DropShadow(s) | StyleEffect::InnerShadow(s) => s.enabled,
        StyleEffect::OuterGlow(g) | StyleEffect::InnerGlow(g) => g.enabled,
        StyleEffect::Bevel(b) => b.enabled,
        StyleEffect::Satin(s) => s.enabled,
        StyleEffect::ColorOverlay(o)
        | StyleEffect::GradientOverlay(o)
        | StyleEffect::PatternOverlay(o)
        | StyleEffect::Overlay(o) => o.enabled,
        StyleEffect::Stroke(s) => s.enabled,
    }
}

/// Effects of `styles` top first: descending rank, and within a kind the
/// later vector entry (composited later, so above) first.
fn summary(layer: u64, styles: &LayerStyles) -> LayerStyleSummary {
    let mut order: Vec<usize> = (0..styles.effects.len()).collect();
    order.sort_by_key(|&i| {
        (
            std::cmp::Reverse(rank(&styles.effects[i])),
            std::cmp::Reverse(i),
        )
    });
    LayerStyleSummary {
        layer,
        effects: order
            .into_iter()
            .map(|i| StyleEffectSummary {
                index: i as u32,
                kind: kind_name(&styles.effects[i]),
                enabled: enabled(&styles.effects[i]),
            })
            .collect(),
    }
}

/// Whether `l` may take `styles` (empty styles are always allowed).
fn check_styleable(l: &Layer, styles: &LayerStyles) -> Result<()> {
    if l.props.locks.all {
        return Err(failure(format!(
            "layer \"{}\" is locked: unlock it to change its layer style",
            l.props.name
        )));
    }
    if styles.effects.is_empty() {
        return Ok(());
    }
    match &l.kind {
        LayerKind::Adjustment(_) => Err(failure(
            "adjustment layers cannot have layer styles".to_owned(),
        )),
        LayerKind::Group {
            mode: GroupMode::PassThrough,
            ..
        } => Err(failure(
            "pass-through groups cannot have layer styles: set the group's mode to Isolated (Normal) first"
                .to_owned(),
        )),
        _ => Ok(()),
    }
}

fn parse_styles(json: &str) -> Result<LayerStyles> {
    let styles: LayerStyles =
        serde_json::from_str(json).map_err(|e| failure(format!("layer style JSON: {e}")))?;
    styles.validate()?;
    Ok(styles)
}

fn styles_op(s: &compositor::DocState, id: u64, styles: LayerStyles) -> Result<DocOp> {
    let l = find(s, id)?;
    check_styleable(l, &styles)?;
    let mut props = l.props.clone();
    props.styles = styles;
    Ok(DocOp::SetProps {
        id: LayerId(id),
        props,
    })
}

#[uniffi::export]
impl DocumentSession {
    /// The layer's styles as `LayerStyles` JSON (live state: shows a drag).
    pub fn layer_styles_json(&self, layer: u64) -> Result<String> {
        let st = self.shared.read()?;
        let l = find(st.live(), layer)?;
        serde_json::to_string(&l.props.styles).map_err(failure)
    }

    /// Replaces the layer's styles (`LayerStyles` JSON), keeping every other
    /// property. `interactive`: live only, no history node until `commit`
    /// (an inspector slider drag). Refused on locked (Lock All) layers,
    /// adjustment layers and pass-through groups.
    pub fn set_layer_styles_json(
        &self,
        layer: u64,
        json: String,
        interactive: bool,
    ) -> Result<DocumentUpdate> {
        let styles = parse_styles(&json)?;
        self.edit_keyed(Pending::Props(layer), interactive, |s| {
            styles_op(s, layer, styles)
        })
    }

    /// Every layer with at least one effect, in `layers()` order.
    pub fn layer_style_summaries(&self) -> Result<Vec<LayerStyleSummary>> {
        let st = self.shared.read()?;
        let mut out = Vec::new();
        fn go(v: &[std::sync::Arc<Layer>], out: &mut Vec<LayerStyleSummary>) {
            for l in v.iter().rev() {
                if !l.props.styles.effects.is_empty() {
                    out.push(summary(l.id.0, &l.props.styles));
                }
                if let Some(c) = l.children() {
                    go(c, out);
                }
            }
        }
        go(&st.live().root, &mut out);
        Ok(out)
    }

    /// The document's Global Light (live state).
    pub fn global_light(&self) -> Result<GlobalLightRecord> {
        let st = self.shared.read()?;
        let g = st.live().global_light;
        Ok(GlobalLightRecord {
            angle: g.angle,
            altitude: g.elevation,
        })
    }

    /// Layer ▸ Layer Style ▸ Global Light: every effect with Use Global Light
    /// follows it. `interactive` as for `set_layer_styles_json`.
    pub fn set_global_light(
        &self,
        angle: f32,
        altitude: f32,
        interactive: bool,
    ) -> Result<DocumentUpdate> {
        let light = GlobalLight {
            angle,
            elevation: altitude,
        };
        light.validate()?;
        self.edit_keyed(Pending::Props(GLOBAL_LIGHT_KEY), interactive, |_| {
            Ok(DocOp::SetGlobalLight(light))
        })
    }

    /// Copy Layer Style: remembers the layer's effects and Scale Effects
    /// (application-wide). Records no history.
    pub fn copy_layer_styles(&self, from: u64) -> Result<()> {
        let styles = {
            let st = self.shared.lock()?;
            find(st.live().state(), from)?.props.styles.clone()
        };
        *STYLE_CLIPBOARD.lock().map_err(failure)? = Some(styles);
        Ok(())
    }

    /// Whether Copy Layer Style has something to paste.
    pub fn can_paste_layer_styles(&self) -> bool {
        STYLE_CLIPBOARD.lock().is_ok_and(|c| c.is_some())
    }

    /// Paste Layer Style onto every layer of `to`: one history node.
    pub fn paste_layer_styles(&self, to: Vec<u64>) -> Result<DocumentUpdate> {
        let styles = STYLE_CLIPBOARD
            .lock()
            .map_err(failure)?
            .clone()
            .ok_or_else(|| failure("no layer style has been copied"))?;
        if to.is_empty() {
            return Err(failure("select the layers to paste the layer style onto"));
        }
        let op = {
            let st = self.shared.lock()?;
            let s = st.live().state();
            batch(
                to.iter()
                    .map(|id| styles_op(s, *id, styles.clone()))
                    .collect::<Result<Vec<_>>>()?,
            )
        };
        self.edit(op, Some("Paste Layer Style"))
    }

    /// Clear Layer Style: removes every effect (Scale Effects back to 100 %)
    /// as one history node.
    pub fn clear_layer_styles(&self, layer: u64) -> Result<DocumentUpdate> {
        let op = {
            let st = self.shared.lock()?;
            styles_op(st.live().state(), layer, LayerStyles::default())?
        };
        self.edit(op, Some("Clear Layer Style"))
    }
}

// ─────────────────────────────── schema ───────────────────────────────

fn num(key: &str, title: &str, min: f32, max: f32, unit: &str, display_scale: f32) -> Value {
    json!({"key": key, "title": title, "type": "number", "min": min, "max": max,
           "unit": unit, "display_scale": display_scale})
}
fn pct(key: &str, title: &str) -> Value {
    num(key, title, 0.0, 1.0, "%", 100.0)
}
fn px(key: &str, title: &str, max: f32) -> Value {
    num(key, title, 0.0, max, "px", 1.0)
}
fn angle(key: &str, title: &str) -> Value {
    json!({"key": key, "title": title, "type": "angle", "min": -180.0, "max": 180.0, "unit": "°"})
}
fn boolean(key: &str, title: &str) -> Value {
    json!({"key": key, "title": title, "type": "bool"})
}
fn color(key: &str, title: &str) -> Value {
    json!({"key": key, "title": title, "type": "color"})
}
fn mode(key: &str, title: &str) -> Value {
    json!({"key": key, "title": title, "type": "blend_mode"})
}
fn choice(key: &str, title: &str, options: &[(&str, &str)]) -> Value {
    json!({"key": key, "title": title, "type": "enum",
           "options": options.iter().map(|(v, t)| json!({"value": v, "title": t})).collect::<Vec<_>>()})
}
fn fill(key: &str, title: &str) -> Value {
    json!({"key": key, "title": title, "type": "fill"})
}

/// Contour / jitter (and, from PSD, texture) are kept but not rendered.
fn shape_metadata() -> Value {
    json!([{"key": "shape", "title": "Contour",
            "note": "Kept from the source file; not rendered by this engine"}])
}

fn settings<T: serde::Serialize>(t: T) -> Value {
    serde_json::to_value(t).unwrap_or(Value::Null)
}

/// The effect kinds `LayerStyles` JSON may hold, their fields (JSON keys of
/// `settings`) with UI ranges, defaults and flags, top first in the engine's
/// stacking order. Field `type`s: `number` (`min`/`max` in stored units,
/// shown × `display_scale` with `unit`), `angle` (degrees), `bool`, `color`
/// (straight RGBA), `blend_mode` (a `blend_mode_names()` name), `enum`
/// (`options`), `fill` (`compositor::Fill` JSON). `metadata` fields are
/// preserved but not rendered. `repeatable` kinds may appear more than once;
/// `global_light` kinds follow Use Global Light; `psd` kinds survive a PSD
/// save (solid fills only, one of each).
#[uniffi::export]
pub fn style_effects_schema_json() -> String {
    let shadow = |inner: bool| {
        vec![
            mode("mode", "Blend Mode"),
            color("color", "Color"),
            pct("opacity", "Opacity"),
            angle("angle", "Angle"),
            boolean("use_global_light", "Use Global Light"),
            px("distance", "Distance", 1000.0),
            px("spread", if inner { "Choke" } else { "Spread" }, 250.0),
            px("size", "Size", 250.0),
        ]
    };
    let glow = |inner: bool| {
        let mut fields = vec![
            mode("mode", "Blend Mode"),
            color("color", "Color"),
            pct("opacity", "Opacity"),
            px("spread", if inner { "Choke" } else { "Spread" }, 250.0),
            px("size", "Size", 250.0),
        ];
        if inner {
            fields.push(json!({"key": "center", "title": "Source", "type": "enum",
                               "options": [{"value": false, "title": "Edge"}, {"value": true, "title": "Center"}]}));
        }
        fields
    };
    let overlay = || {
        vec![
            mode("mode", "Blend Mode"),
            pct("opacity", "Opacity"),
            fill("fill", "Fill"),
        ]
    };
    let gradient_fill = json!({"kind": "gradient", "gradient": "linear", "start": [0.0, 0.0], "end": [256.0, 0.0],
                               "stops": [{"position": 0.0, "color": [0.0, 0.0, 0.0, 1.0]},
                                         {"position": 1.0, "color": [1.0, 1.0, 1.0, 1.0]}]});
    let pattern_fill = json!({"kind": "pattern", "width": 2, "height": 2, "origin": [0.0, 0.0],
                              "rgba": [1.0, 1.0, 1.0, 1.0, 0.5, 0.5, 0.5, 1.0, 0.5, 0.5, 0.5, 1.0, 1.0, 1.0, 1.0, 1.0]});
    let with_fill = |f: Value| {
        let mut o = settings(Overlay::default());
        o["fill"] = f;
        o
    };
    let entry = |kind: &str,
                 title: &str,
                 effect: StyleEffect,
                 defaults: Value,
                 fields: Vec<Value>,
                 repeatable: bool,
                 global_light: bool,
                 psd: bool,
                 behind: bool,
                 metadata: Value| {
        json!({"kind": kind, "title": title, "rank": rank(&effect), "repeatable": repeatable,
               "global_light": global_light, "psd": psd, "behind": behind,
               "defaults": defaults, "fields": fields, "metadata": metadata})
    };
    let effects = vec![
        entry(
            "bevel",
            "Bevel & Emboss",
            StyleEffect::Bevel(Bevel::default()),
            settings(Bevel::default()),
            vec![
                choice(
                    "kind",
                    "Style",
                    &[
                        ("inner", "Inner Bevel"),
                        ("outer", "Outer Bevel"),
                        ("emboss", "Emboss"),
                        ("pillow", "Pillow Emboss"),
                    ],
                ),
                num("depth", "Depth", 0.0, 10.0, "%", 100.0),
                json!({"key": "down", "title": "Direction", "type": "enum",
                       "options": [{"value": false, "title": "Up"}, {"value": true, "title": "Down"}]}),
                px("size", "Size", 250.0),
                px("soften", "Soften", 16.0),
                angle("angle", "Angle"),
                num("elevation", "Altitude", 0.0, 90.0, "°", 1.0),
                boolean("use_global_light", "Use Global Light"),
                mode("highlight_mode", "Highlight Mode"),
                color("highlight_color", "Highlight Color"),
                pct("highlight_opacity", "Highlight Opacity"),
                mode("shadow_mode", "Shadow Mode"),
                color("shadow_color", "Shadow Color"),
                pct("shadow_opacity", "Shadow Opacity"),
            ],
            false,
            true,
            false,
            false,
            json!([{"key": "shape", "title": "Contour",
                    "note": "Kept from the source file; not rendered by this engine"},
                   {"key": "texture", "title": "Texture",
                    "note": "Kept in the PSD's effect descriptor; not rendered or editable here"}]),
        ),
        entry(
            "stroke",
            "Stroke",
            StyleEffect::Stroke(Stroke::default()),
            settings(Stroke::default()),
            vec![
                px("size", "Size", 250.0),
                choice(
                    "position",
                    "Position",
                    &[
                        ("outside", "Outside"),
                        ("inside", "Inside"),
                        ("center", "Center"),
                    ],
                ),
                mode("mode", "Blend Mode"),
                pct("opacity", "Opacity"),
                fill("fill", "Fill"),
            ],
            true,
            false,
            true,
            false,
            json!([]),
        ),
        entry(
            "inner_shadow",
            "Inner Shadow",
            StyleEffect::InnerShadow(Shadow::default()),
            settings(Shadow::default()),
            shadow(true),
            true,
            true,
            true,
            false,
            shape_metadata(),
        ),
        entry(
            "inner_glow",
            "Inner Glow",
            StyleEffect::InnerGlow(Glow::default()),
            settings(Glow::default()),
            glow(true),
            false,
            false,
            true,
            false,
            shape_metadata(),
        ),
        entry(
            "satin",
            "Satin",
            StyleEffect::Satin(Satin::default()),
            settings(Satin::default()),
            vec![
                mode("mode", "Blend Mode"),
                color("color", "Color"),
                pct("opacity", "Opacity"),
                angle("angle", "Angle"),
                px("distance", "Distance", 250.0),
                px("size", "Size", 250.0),
                boolean("invert", "Invert"),
            ],
            false,
            false,
            false,
            false,
            shape_metadata(),
        ),
        entry(
            "color_overlay",
            "Color Overlay",
            StyleEffect::ColorOverlay(Overlay::default()),
            settings(Overlay::default()),
            overlay(),
            true,
            false,
            true,
            false,
            json!([]),
        ),
        entry(
            "overlay",
            "Overlay",
            StyleEffect::Overlay(Overlay::default()),
            settings(Overlay::default()),
            overlay(),
            true,
            false,
            true,
            false,
            json!([]),
        ),
        entry(
            "gradient_overlay",
            "Gradient Overlay",
            StyleEffect::GradientOverlay(Overlay::default()),
            with_fill(gradient_fill),
            overlay(),
            true,
            false,
            false,
            false,
            json!([]),
        ),
        entry(
            "pattern_overlay",
            "Pattern Overlay",
            StyleEffect::PatternOverlay(Overlay::default()),
            with_fill(pattern_fill),
            overlay(),
            true,
            false,
            false,
            false,
            json!([]),
        ),
        entry(
            "outer_glow",
            "Outer Glow",
            StyleEffect::OuterGlow(Glow::default()),
            settings(Glow::default()),
            glow(false),
            false,
            false,
            true,
            true,
            shape_metadata(),
        ),
        entry(
            "drop_shadow",
            "Drop Shadow",
            StyleEffect::DropShadow(Shadow::default()),
            settings(Shadow::default()),
            shadow(false),
            true,
            true,
            true,
            true,
            shape_metadata(),
        ),
    ];
    let light = GlobalLight::default();
    json!({
        "version": 1,
        "effects": effects,
        "scale": {"key": "scale", "title": "Scale Effects", "type": "number", "min": 0.01, "max": 10.0,
                  "unit": "%", "display_scale": 100.0, "default": 1.0},
        "global_light": {"angle": light.angle, "altitude": light.elevation,
                         "altitude_min": 0.0, "altitude_max": 90.0},
        "max_effects": 64,
        "max_per_kind": 10,
    })
    .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn schema_lists_every_kind_top_first_with_valid_defaults() {
        let v: Value = serde_json::from_str(&style_effects_schema_json()).unwrap();
        let effects = v["effects"].as_array().unwrap();
        let ranks: Vec<u64> = effects
            .iter()
            .map(|e| e["rank"].as_u64().unwrap())
            .collect();
        assert!(ranks.windows(2).all(|w| w[0] >= w[1]), "{ranks:?}");
        for e in effects {
            let json = json!({"effects": [{"kind": e["kind"], "settings": e["defaults"]}]});
            let styles: LayerStyles = serde_json::from_value(json).unwrap();
            styles.validate().unwrap();
            assert_eq!(kind_name(&styles.effects[0]), e["kind"].as_str().unwrap());
        }
    }
}
