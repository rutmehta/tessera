use crate::{Error, Result};
use std::path::Path;

/// Four-digit sequence, original stem, and original extension. Paths and unknown
/// tokens are rejected instead of escaping the session or silently misnaming.
pub(crate) fn render(template: &str, source: &Path, sequence: u64) -> Result<String> {
    let stem = source
        .file_stem()
        .and_then(|s| s.to_str())
        .ok_or_else(|| Error::Message("invalid original filename".into()))?;
    let ext = source
        .extension()
        .and_then(|s| s.to_str())
        .ok_or_else(|| Error::Message("missing extension".into()))?;
    let name = template
        .replace("{sequence}", &format!("{sequence:04}"))
        .replace("{original}", stem)
        .replace("{ext}", ext);
    if name.is_empty()
        || name.starts_with('.')
        || name.contains(['/', '\\', ':', '{', '}', '\0'])
        || name.chars().any(char::is_control)
        || name.len() > 240
        || !name.ends_with(&format!(".{ext}"))
    {
        return Err(Error::Message("naming must be a plain filename with {sequence}, {original}, {ext}; preserve the extension".into()));
    }
    Ok(name)
}
