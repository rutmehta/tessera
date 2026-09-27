use crate::{Error, Result, TextModel, TextRun};
use psd::metadata::{Text, Value};

/// Editable TySh import. Retain the original TySh block for unknown descriptor
/// fields and pass its EngineData to `export_engine_data` when editing.
#[derive(Clone, Debug)]
pub struct ImportedText {
    pub model: TextModel,
    /// Original PSD affine order [xx, xy, yx, yy, tx, ty], not compositor order.
    pub transform: [f64; 6],
    pub bounds: [f64; 4],
    pub original_engine_data: Option<Vec<u8>>,
    pub warnings: Vec<String>,
}
fn byte_index(text: &str, units: usize) -> Result<usize> {
    let mut count = 0;
    for (index, ch) in text.char_indices() {
        if count == units {
            return Ok(index);
        }
        count += ch.len_utf16();
        if count > units {
            return Err(Error::Invalid("PSD range splits a surrogate pair"));
        }
    }
    if count == units {
        Ok(text.len())
    } else {
        Err(Error::Invalid("PSD range exceeds text"))
    }
}
/// Maps descriptor ranges, or EngineData FontSet/StyleRun lengths when absent.
/// Descriptor sizes in points are treated as pixels at 72 dpi. The caller must
/// apply document DPI and the preserved transform; unsupported units warn.
pub fn import_tysh(source: &Text<'_>) -> Result<ImportedText> {
    let text = source
        .text()
        .ok_or(Error::Invalid("TySh has no text string"))?;
    if text.len() > 1_000_000 {
        return Err(Error::Invalid("text limit exceeded"));
    }
    let mut imported = ImportedText {
        model: TextModel::point(text, "sans-serif", 24.0),
        transform: source.transform,
        bounds: source.bounds,
        original_engine_data: source.engine_data().map(<[u8]>::to_vec),
        warnings: Vec::new(),
    };
    // Ranges in UTF-16 code units. Build boundary intervals to retain unstyled
    // gaps rather than dropping source characters or duplicating overlaps.
    let total = text.encode_utf16().count();
    let mut styles = Vec::new();
    let descriptor_runs = source.style_runs();
    if !descriptor_runs.is_empty() {
        for run in descriptor_runs {
            let from = run.from.unwrap_or(0);
            let to = run.to.unwrap_or(total as i32);
            if from < 0 || to < from {
                return Err(Error::Invalid("invalid PSD style range"));
            }
            let mut style = TextRun::default();
            if let Some(name) = run.font_name() {
                style.family = name.into();
            }
            if let Some(value) = run.size() {
                match value {
                    Value::Unit { unit, value } if unit == b"#Pnt" || unit == b"#Pxl" => {
                        style.size = *value as f32
                    }
                    Value::Double(value) => style.size = *value as f32,
                    _ => imported
                        .warnings
                        .push("Unsupported descriptor font-size unit".into()),
                }
            }
            styles.push((
                byte_index(text, from as usize)?,
                byte_index(text, to as usize)?,
                style,
            ));
        }
    } else {
        match source.engine_styles() {
            Ok(Some(engine)) => {
                let mut start = 0usize;
                for (i, run) in engine.runs.iter().enumerate() {
                    let length = run.length.unwrap_or(total.saturating_sub(start));
                    let end = start
                        .checked_add(length)
                        .ok_or(Error::Invalid("PSD range overflow"))?;
                    // Photoshop may include one final implicit paragraph marker.
                    if end > total.saturating_add(1) {
                        return Err(Error::Invalid("PSD range exceeds text"));
                    }
                    let mut style = TextRun::default();
                    if let Some(name) = engine.font_for_run(i) {
                        style.family = name.into();
                    }
                    if let Some(size) = run.size {
                        style.size = size as f32;
                    }
                    styles.push((
                        byte_index(text, start.min(total))?,
                        byte_index(text, end.min(total))?,
                        style,
                    ));
                    start = end;
                }
            }
            Err(error) => imported
                .warnings
                .push(format!("EngineData retained but not interpreted: {error}")),
            Ok(None) => {}
        }
    }
    if !styles.is_empty() {
        let mut boundaries = vec![0, text.len()];
        for (start, end, _) in &styles {
            boundaries.extend([*start, *end]);
        }
        boundaries.sort_unstable();
        boundaries.dedup();
        let mut runs = Vec::new();
        for pair in boundaries.windows(2) {
            let mut run = styles
                .iter()
                .rev()
                .find(|(start, end, _)| *start <= pair[0] && *end >= pair[1])
                .map(|(_, _, style)| style.clone())
                .unwrap_or_default();
            run.text = text[pair[0]..pair[1]].into();
            runs.push(run);
        }
        imported.model.runs = runs;
    }
    if let Err(error) = apply_engine(source, &mut imported.model) {
        imported.warnings.push(error.to_string());
    }
    imported.model.validate()?;
    Ok(imported)
}

use psd::metadata::EngineValue as E;
fn engine_error(_: psd::Error) -> Error {
    Error::Invalid("unsupported Adobe EngineData")
}
fn num(value: Option<&E<'_>>, default: f64) -> f64 {
    value.and_then(E::number).unwrap_or(default)
}
fn eng_style(style: &E<'_>, run: &mut TextRun) {
    run.tracking = (num(style.get(b"Tracking"), 0.) * f64::from(run.size) / 1000.) as f32;
    run.leading = num(style.get(b"Leading"), 0.) as f32;
    run.baseline_shift = num(style.get(b"BaselineShift"), 0.) as f32;
    run.italic = style
        .get(b"FauxItalic")
        .and_then(E::boolean)
        .unwrap_or(false);
    run.weight = if style.get(b"FauxBold").and_then(E::boolean).unwrap_or(false) {
        700
    } else {
        400
    };
    for (key, tag) in [(b"Ligatures".as_slice(), "liga"), (b"DLigatures", "dlig")] {
        if let Some(enabled) = style.get(key).and_then(E::boolean) {
            run.features.insert(tag.into(), u32::from(enabled));
        }
    }
    run.kerning = style
        .get(b"AutoKerning")
        .and_then(E::boolean)
        .unwrap_or(true);
    if let Some(v) = style
        .get(b"FillColor")
        .and_then(|v| v.get(b"Values"))
        .and_then(E::array)
        .filter(|v| v.len() == 4)
    {
        run.color = std::array::from_fn(|i| {
            (num(v.get((i + 1) % 4), if i == 3 { 1. } else { 0. }) * 255.)
                .round()
                .clamp(0., 255.) as u8
        });
    }
}
fn apply_engine(source: &Text<'_>, model: &mut TextModel) -> Result<()> {
    if let Some(bytes) = source.engine_data() {
        let root = psd::metadata::parse_engine_data(bytes).map_err(engine_error)?;
        if let Some(engine) = root.get(b"EngineDict") {
            if let Some(runs) = engine
                .get(b"StyleRun")
                .and_then(|s| s.get(b"RunArray"))
                .and_then(E::array)
            {
                for (run, data) in model.runs.iter_mut().zip(runs) {
                    if let Some(style) = data
                        .get(b"StyleSheet")
                        .and_then(|s| s.get(b"StyleSheetData"))
                    {
                        eng_style(style, run);
                    }
                }
            }
            if let Some(p) = engine
                .get(b"ParagraphRun")
                .and_then(|v| v.get(b"RunArray"))
                .and_then(E::array)
                .and_then(|a| a.first())
                .and_then(|p| p.get(b"ParagraphSheet"))
                .and_then(|p| p.get(b"Properties"))
            {
                model.paragraph.alignment = match num(p.get(b"Justification"), 0.) as i32 {
                    1 => crate::Alignment::Center,
                    2 => crate::Alignment::Right,
                    3..=6 => crate::Alignment::Justify,
                    _ => crate::Alignment::Left,
                };
                model.paragraph.hyphenation =
                    p.get(b"Hyphenate").and_then(E::boolean).unwrap_or(false);
                model.paragraph.left_indent = num(p.get(b"StartIndent"), 0.) as f32;
                model.paragraph.right_indent = num(p.get(b"EndIndent"), 0.) as f32;
                model.paragraph.first_line_indent = num(p.get(b"FirstLineIndent"), 0.) as f32;
                model.paragraph.space_before = num(p.get(b"SpaceBefore"), 0.) as f32;
                model.paragraph.space_after = num(p.get(b"SpaceAfter"), 0.) as f32;
            }
            if let Some(shape) = engine
                .get(b"Rendered")
                .and_then(|v| v.get(b"Shapes"))
                .and_then(|v| v.get(b"Children"))
                .and_then(E::array)
                .and_then(|v| v.first())
                .and_then(|v| v.get(b"Cookie"))
                .and_then(|v| v.get(b"Photoshop"))
                && num(shape.get(b"ShapeType"), 0.) == 1.
                && let Some(b) = shape
                    .get(b"BoxBounds")
                    .and_then(E::array)
                    .filter(|b| b.len() == 4)
            {
                model.text_box = crate::TextBox::Paragraph {
                    width: (num(b.get(2), 0.) - num(b.first(), 0.)) as f32,
                    height: (num(b.get(3), 0.) - num(b.get(1), 0.)) as f32,
                };
            }
        }
    }
    model.vertical =
        matches!(source.descriptor.get(b"Ornt"), Some(Value::Enum{value,..}) if *value==b"Vrtc");
    model.warp.kind = match source.warp.get(b"warpStyle") {
        Some(Value::Enum { value, .. }) if *value == b"warpFlag" => crate::WarpKind::Flag,
        Some(Value::Enum { value, .. }) if *value == b"warpWave" => crate::WarpKind::Wave,
        _ => crate::WarpKind::Arc,
    };
    model.warp.amount = source
        .warp
        .get(b"warpValue")
        .and_then(Value::number)
        .unwrap_or(0.) as f32
        / 100.;
    Ok(())
}

/// Update modeled Adobe EngineData fields, retaining unknown dictionaries and
/// unknown fields inside existing runs. This is not the native JSON envelope.
pub fn export_engine_data(model: &TextModel, original: Option<&[u8]>) -> Result<Vec<u8>> {
    model.validate()?;
    let mut root = original
        .map(psd::metadata::parse_engine_data)
        .transpose()
        .map_err(engine_error)?
        .unwrap_or_else(E::dict);
    let mut fonts = root
        .get(b"ResourceDict")
        .or_else(|| root.get(b"DocumentResources"))
        .and_then(|r| r.get(b"FontSet"))
        .and_then(E::array)
        .unwrap_or(&[])
        .to_vec();
    let mut font_indices = Vec::new();
    for run in &model.runs {
        let index = fonts
            .iter()
            .position(|f| {
                f.get(b"Name").and_then(|v| v.string().ok()).as_deref() == Some(run.family.as_str())
            })
            .unwrap_or_else(|| {
                let mut f = E::dict();
                f.put(b"Name", E::unicode(&run.family));
                fonts.push(f);
                fonts.len() - 1
            });
        font_indices.push(index);
    }
    root.entry(b"ResourceDict").put(b"FontSet", E::Array(fonts));
    let engine = root.entry(b"EngineDict");
    let text: String = model.runs.iter().map(|r| r.text.as_str()).collect();
    engine.entry(b"Editor").put(b"Text", E::unicode(&text));
    let style_run = engine.entry(b"StyleRun");
    let old = style_run
        .get(b"RunArray")
        .and_then(E::array)
        .unwrap_or(&[])
        .to_vec();
    let mut styles = Vec::new();
    for (i, run) in model.runs.iter().enumerate() {
        let mut item = old.get(i).cloned().unwrap_or_else(E::dict);
        let s = item.entry(b"StyleSheet").entry(b"StyleSheetData");
        for (k, v) in [
            (b"Font".as_slice(), font_indices[i] as f64),
            (b"FontSize", f64::from(run.size)),
            (
                b"Tracking",
                f64::from(run.tracking) / f64::from(run.size) * 1000.,
            ),
            (b"Leading", f64::from(run.leading)),
            (b"BaselineShift", f64::from(run.baseline_shift)),
        ] {
            s.put(k, E::Number(v));
        }
        for (key, tag) in [(b"Ligatures".as_slice(), "liga"), (b"DLigatures", "dlig")] {
            if let Some(value) = run.features.get(tag) {
                s.put(key, E::Bool(*value != 0));
            }
        }
        s.put(b"FauxItalic", E::Bool(run.italic));
        s.put(b"FauxBold", E::Bool(run.weight >= 600));
        s.put(b"AutoKerning", E::Bool(run.kerning));
        let c = s.entry(b"FillColor");
        c.put(b"Type", E::Number(1.));
        c.put(
            b"Values",
            E::Array(
                [run.color[3], run.color[0], run.color[1], run.color[2]]
                    .map(|v| E::Number(f64::from(v) / 255.))
                    .to_vec(),
            ),
        );
        styles.push(item);
    }
    style_run.put(b"RunArray", E::Array(styles));
    style_run.put(
        b"RunLengthArray",
        E::Array(
            model
                .runs
                .iter()
                .map(|r| E::Number(r.text.encode_utf16().count() as f64))
                .collect(),
        ),
    );
    let paragraph = engine.entry(b"ParagraphRun");
    let mut p = paragraph
        .get(b"RunArray")
        .and_then(E::array)
        .and_then(|v| v.first())
        .cloned()
        .unwrap_or_else(E::dict);
    let props = p.entry(b"ParagraphSheet").entry(b"Properties");
    let alignment = match model.paragraph.alignment {
        crate::Alignment::Left => 0.,
        crate::Alignment::Center => 1.,
        crate::Alignment::Right => 2.,
        crate::Alignment::Justify => 3.,
    };
    let pmodel = &model.paragraph;
    props.put(b"Hyphenate", E::Bool(pmodel.hyphenation));
    for (k, v) in [
        (b"Justification".as_slice(), alignment),
        (b"StartIndent", f64::from(pmodel.left_indent)),
        (b"EndIndent", f64::from(pmodel.right_indent)),
        (b"FirstLineIndent", f64::from(pmodel.first_line_indent)),
        (b"SpaceBefore", f64::from(pmodel.space_before)),
        (b"SpaceAfter", f64::from(pmodel.space_after)),
    ] {
        props.put(k, E::Number(v));
    }
    paragraph.put(b"RunArray", E::Array(vec![p]));
    paragraph.put(
        b"RunLengthArray",
        E::Array(vec![E::Number(text.encode_utf16().count() as f64)]),
    );
    let shapes = engine.entry(b"Rendered").entry(b"Shapes");
    let mut children = shapes
        .get(b"Children")
        .and_then(E::array)
        .unwrap_or(&[])
        .to_vec();
    if children.is_empty() {
        children.push(E::dict());
    }
    let shape = children[0].entry(b"Cookie").entry(b"Photoshop");
    match model.text_box {
        crate::TextBox::Point => shape.put(b"ShapeType", E::Number(0.)),
        crate::TextBox::Paragraph { width, height } => {
            shape.put(b"ShapeType", E::Number(1.));
            shape.put(
                b"BoxBounds",
                E::Array(
                    [0., 0., f64::from(width), f64::from(height)]
                        .map(E::Number)
                        .to_vec(),
                ),
            );
        }
    }
    shapes.put(b"Children", E::Array(children));
    root.encode().map_err(engine_error)
}
