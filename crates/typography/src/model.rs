use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::{Error, Result};

/// Character panel values. Distances are document pixels, tracking is added
/// per shaped cluster, axes use native font units, colour is straight sRGBA.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct TextRun {
    pub text: String,
    pub family: String,
    pub weight: u16,
    pub italic: bool,
    pub size: f32,
    pub tracking: f32,
    pub kerning: bool,
    /// Baseline distance; zero selects 1.2 × size.
    pub leading: f32,
    pub baseline_shift: f32,
    pub color: [u8; 4],
    /// Four-byte OpenType tags, e.g. liga, dlig, ss01.
    pub features: BTreeMap<String, u32>,
    pub axes: BTreeMap<String, f32>,
}
impl Default for TextRun {
    fn default() -> Self {
        Self {
            text: String::new(),
            family: "sans-serif".into(),
            weight: 400,
            italic: false,
            size: 24.0,
            tracking: 0.0,
            kerning: true,
            leading: 0.0,
            baseline_shift: 0.0,
            color: [0, 0, 0, 255],
            features: BTreeMap::new(),
            axes: BTreeMap::new(),
        }
    }
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Alignment {
    #[default]
    Left,
    Center,
    Right,
    Justify,
}
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Paragraph {
    pub alignment: Alignment,
    /// Reserved. No language dictionary is currently applied.
    pub hyphenation: bool,
    pub left_indent: f32,
    pub right_indent: f32,
    pub first_line_indent: f32,
    pub space_before: f32,
    pub space_after: f32,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum TextBox {
    #[default]
    Point,
    /// Width constrains layout. Height clips visible lines and reports overflow.
    Paragraph { width: f32, height: f32 },
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WarpKind {
    #[default]
    Arc,
    Flag,
    Wave,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Warp {
    pub kind: WarpKind,
    pub amount: f32,
}
/// Live editable source, independent of raster caches and machine font IDs.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct TextModel {
    pub runs: Vec<TextRun>,
    pub paragraph: Paragraph,
    pub text_box: TextBox,
    /// Reserved for a vertical composer. Rendering returns UnsupportedVertical.
    pub vertical: bool,
    pub warp: Warp,
    pub path: Option<crate::PathText>,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Envelope {
    version: u32,
    model: TextModel,
}
impl TextModel {
    pub fn point(text: impl Into<String>, family: impl Into<String>, size: f32) -> Self {
        Self {
            runs: vec![TextRun {
                text: text.into(),
                family: family.into(),
                size,
                ..TextRun::default()
            }],
            ..Self::default()
        }
    }
    pub fn to_engine_data(&self) -> Result<String> {
        self.validate()?;
        Ok(serde_json::to_string(&Envelope {
            version: 1,
            model: self.clone(),
        })?)
    }
    pub fn from_engine_data(json: &str) -> Result<Self> {
        let envelope: Envelope = serde_json::from_str(json)?;
        if envelope.version != 1 {
            return Err(Error::Invalid("unsupported engine-data version"));
        }
        envelope.model.validate()?;
        Ok(envelope.model)
    }
    pub fn validate(&self) -> Result<()> {
        if let Some(path) = &self.path {
            path.to_lyon()?;
            if !matches!(self.text_box, TextBox::Point) {
                return Err(Error::Invalid("path text requires point text"));
            }
        }
        let p = &self.paragraph;
        if [
            p.left_indent,
            p.right_indent,
            p.first_line_indent,
            p.space_before,
            p.space_after,
            self.warp.amount,
        ]
        .iter()
        .any(|v| !v.is_finite() || v.abs() > 1_000_000.0)
        {
            return Err(Error::Invalid(
                "non-finite or excessive paragraph/warp value",
            ));
        }
        if let TextBox::Paragraph { width, height } = self.text_box
            && (!width.is_finite()
                || !height.is_finite()
                || width <= 0.0
                || height <= 0.0
                || width > 1_000_000.0
                || height > 1_000_000.0
                || width <= p.left_indent + p.right_indent + p.first_line_indent.max(0.0))
        {
            return Err(Error::Invalid("invalid paragraph box"));
        }
        if self.runs.iter().map(|r| r.text.len()).sum::<usize>() > 1_000_000 {
            return Err(Error::Invalid("text limit exceeded"));
        }
        for r in &self.runs {
            if !r.size.is_finite()
                || r.size <= 0.0
                || r.size > 100_000.0
                || r.weight == 0
                || r.weight > 1000
                || !r.leading.is_finite()
                || r.leading < 0.0
                || r.leading > 1_000_000.0
                || [r.tracking, r.baseline_shift]
                    .iter()
                    .any(|v| !v.is_finite() || v.abs() > 1_000_000.0)
            {
                return Err(Error::Invalid("invalid character metric"));
            }
            for tag in r.features.keys().chain(r.axes.keys()) {
                if tag.len() != 4 || !tag.bytes().all(|b| (32..=126).contains(&b)) {
                    return Err(Error::Invalid("OpenType tag must be four ASCII bytes"));
                }
            }
            if r.axes.values().any(|v| !v.is_finite()) {
                return Err(Error::Invalid("non-finite variation axis"));
            }
        }
        Ok(())
    }
}
