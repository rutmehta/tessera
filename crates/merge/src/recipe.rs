//! Editable auto-tone defaults and lossless recipe JSON inside XMP.
use crate::{LinearImage, Result};
use engine_api::recipe::{Author, EditMeta, Recipe, crs::TS_NAMESPACE};

/// Set middle gray from log-average camera luminance; never alter merged samples.
/// Caller assigns the catalogue image id and creation time after import.
pub fn auto_recipe(image: &LinearImage) -> Result<Recipe> {
    image.validate()?;
    let log_mean = image
        .pixels
        .iter()
        .map(|p| {
            ((p[0] as f64 + 2. * p[1] as f64 + p[2] as f64) / 4.)
                .max(1e-6)
                .ln()
        })
        .sum::<f64>()
        / image.pixels.len() as f64;
    let ev = (0.18_f64.ln() - log_mean) / std::f64::consts::LN_2;
    let mut recipe = Recipe::default();
    recipe
        .edit(
            EditMeta {
                label: "Merge auto tone".into(),
                author: Author::Import {
                    source: "tessera-merge".into(),
                },
                ..Default::default()
            },
            |s| {
                s.tone.exposure = ev.clamp(-10., 10.) as f32;
                s.tone.highlights = -35.;
                s.tone.shadows = 15.;
            },
        )
        .map_err(|e| e.to_string())?;
    Ok(recipe)
}
/// Complete native Recipe, including history/unknown fields, as an XMP property.
/// Standard CRS companions give other readers a best-effort starting rendition.
pub fn recipe_xmp(recipe: &Recipe) -> Result<String> {
    let json = serde_json::to_string(recipe).map_err(|e| e.to_string())?;
    let escaped = json
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;");
    Ok(format!(
        r#"<x:xmpmeta xmlns:x="adobe:ns:meta/"><rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#"><rdf:Description rdf:about="" xmlns:ts="{}" xmlns:crs="http://ns.adobe.com/camera-raw-settings/1.0/" crs:ProcessVersion="15.4" ts:NativeRevision="{}" crs:Exposure2012="{}" crs:Highlights2012="{}" crs:Shadows2012="{}"><ts:Recipe>{}</ts:Recipe></rdf:Description></rdf:RDF></x:xmpmeta>"#,
        TS_NAMESPACE,
        recipe.process_version.revision,
        recipe.settings.tone.exposure,
        recipe.settings.tone.highlights,
        recipe.settings.tone.shadows,
        escaped
    ))
}
/// Write the float image plus its editable recipe; no tone or WB is baked in.
pub fn write_dng<W: std::io::Write>(
    writer: &mut W,
    image: &LinearImage,
    recipe: &Recipe,
) -> std::io::Result<()> {
    let xmp = recipe_xmp(recipe).map_err(std::io::Error::other)?;
    crate::dng::write(writer, image, &xmp)
}
