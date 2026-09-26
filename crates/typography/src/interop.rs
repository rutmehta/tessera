use crate::{Error, Result, TextModel, TextRun};
use psd::metadata::{Text, Value};

/// Basic TySh import. Keep the source PSD's entire TySh block when exporting:
/// this hook does not claim to encode Adobe's undocumented engine language.
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
    imported.model.validate()?;
    Ok(imported)
}
