//! Camera Raw translations. Unknown/unsupported structures remain in the source packet.
use crate::{MarkPreset, Metadata, XmpPacket, xml::*, xmp::metadata_body};
use engine_api::{
    error::{EngineError, EngineResult},
    recipe::{
        Author, CrsKey, CrsValueType, DevelopSettings, EditMeta, LensProfileSetup, Recipe,
        crs::NATIVE_REVISION_PROPERTY,
    },
};
use serde_json::{Value, json};

#[path = "masks.rs"]
mod masks;
#[path = "structures.rs"]
mod structures;

/// Import result. Warnings identify settings retained in XMP but not renderable by this engine.
#[derive(Debug, Clone)]
pub struct ImportedRecipe {
    pub recipe: Recipe,
    pub warnings: Vec<String>,
}

impl XmpPacket {
    pub fn from_recipe(
        recipe: &Recipe,
        metadata: &Metadata,
        preset: &MarkPreset,
    ) -> EngineResult<Self> {
        let base = Self::parse(packet(&metadata_body(&recipe.selection, metadata, preset)))?;
        base.with_recipe(recipe)
    }
    /// Update develop properties, retaining original structured data when its recipe target is unchanged.
    pub fn with_recipe(&self, recipe: &Recipe) -> EngineResult<Self> {
        recipe.validate()?;
        recipe.to_json()?;
        let tree = Tree::parse(&self.xml)?;
        let value = serde_json::to_value(recipe)?;
        let imported = self.to_recipe()?;
        let original = serde_json::to_value(&imported.recipe)?;
        let mut body = String::new();
        let mut owned = Vec::new();
        for &key in CrsKey::ALL {
            let Some(path) = key.recipe_path() else {
                continue;
            };
            let ns = key.namespace().uri();
            // Unchanged targets retain source spelling, ancillary data and opaque structures.
            if key != CrsKey::ProcessVersion
                && tree.property(ns, key.xmp_name()).is_some()
                && value.pointer(path) == original.pointer(path)
            {
                continue;
            }
            body += &encode(key, &value)?;
            owned.push((ns, key.xmp_name()));
            if key == CrsKey::ProcessVersion {
                // The companion is rewritten (or dropped) with the process version.
                owned.push((PRIVATE, NATIVE_REVISION_PROPERTY));
            }
        }
        // Canonical CRS for external readers, plus a hash-bound exact native companion.
        // Rewriting clears old center/focal metadata because homography is unit-frame.
        if recipe.settings.geometry.upright != imported.recipe.settings.geometry.upright
            || recipe.settings.geometry.upright.homography.is_some()
                && tree
                    .value(
                        CRS,
                        &format!(
                            "UprightTransform_{}",
                            upright_index(recipe.settings.geometry.upright.mode)
                        ),
                    )
                    .is_none()
        {
            for key in geometry_keys() {
                if key.starts_with("Upright") {
                    owned.push((CRS, key));
                }
            }
            let u = &recipe.settings.geometry.upright;
            if u.has_saved_solution() {
                let name = format!("crs:UprightTransform_{}", upright_index(u.mode));
                let csv = u
                    .homography
                    .unwrap()
                    .iter()
                    .flatten()
                    .map(ToString::to_string)
                    .collect::<Vec<_>>()
                    .join(",");
                body += &text(&name, &csv);
            }
            if !u.guides.is_empty() {
                body += &text("crs:UprightFourSegmentsCount", &u.guides.len().to_string());
                for (i, g) in u.guides.iter().enumerate() {
                    body += &text(
                        &format!("crs:UprightFourSegments_{i}"),
                        &g.start
                            .iter()
                            .chain(&g.end)
                            .map(ToString::to_string)
                            .collect::<Vec<_>>()
                            .join(","),
                    );
                }
            }
        }
        for (key, v) in [
            ("ChromaticAberrationR", recipe.settings.lens.legacy_ca_red),
            ("ChromaticAberrationB", recipe.settings.lens.legacy_ca_blue),
        ] {
            let original_value = match key {
                "ChromaticAberrationR" => imported.recipe.settings.lens.legacy_ca_red,
                _ => imported.recipe.settings.lens.legacy_ca_blue,
            };
            if tree.property(CRS, key).is_some() && v == original_value {
                continue;
            }
            owned.push((CRS, key));
            if let Some(v) = v {
                body += &text(&format!("crs:{key}"), &v.to_string());
            }
        }
        owned.push((PRIVATE, "GeometryLens"));
        if recipe.settings.geometry.upright.homography.is_some()
            || recipe.settings.lens.legacy_ca_red.is_some()
            || recipe.settings.lens.legacy_ca_blue.is_some()
        {
            body += &text(
                "ts:GeometryLens",
                &serde_json::to_string(&json!({
                    "upright": recipe.settings.geometry.upright,
                    "legacy_ca_red": recipe.settings.lens.legacy_ca_red,
                    "legacy_ca_blue": recipe.settings.lens.legacy_ca_blue
                }))?,
            );
        }
        let mono_keys = [
            "ConvertToGrayscale",
            "GrayMixerRed",
            "GrayMixerOrange",
            "GrayMixerYellow",
            "GrayMixerGreen",
            "GrayMixerAqua",
            "GrayMixerBlue",
            "GrayMixerPurple",
            "GrayMixerMagenta",
        ];
        if recipe.settings.color.monochrome != imported.recipe.settings.color.monochrome {
            for key in mono_keys {
                owned.push((CRS, key));
            }
            if let Some(gray) = &recipe.settings.color.monochrome {
                body += &text(
                    "crs:ConvertToGrayscale",
                    if gray.enabled { "True" } else { "False" },
                );
                let b = &gray.mixer;
                for (key, amount) in mono_keys[1..].iter().zip([
                    b.red, b.orange, b.yellow, b.green, b.aqua, b.blue, b.purple, b.magenta,
                ]) {
                    if !amount.is_finite() || !(-100. ..=100.).contains(&amount) {
                        return Err(error("monochrome mixer range"));
                    }
                    body += &text(&format!("crs:{key}"), &amount.to_string());
                }
            }
        }
        // Refresh the companion after all CRS edits, including retained opaque data.
        owned.push((PRIVATE, "LensProfileSource"));
        body += &text(
            "ts:LensProfileSource",
            value["settings"]["lens"]["profile"]["kind"]
                .as_str()
                .ok_or_else(|| error("lens profile source"))?,
        );
        owned.push((PRIVATE, "ExportHash"));
        let xml = tree.replace(&self.xml, &owned, &body)?;
        let tree = Tree::parse(&xml)?;
        let hash = crs_hash(&tree)?;
        Self::parse(tree.replace(&xml, &[], &text("ts:ExportHash", &hash))?)
    }
    /// Import all table keys, recording a single valid history edit. Informational keys
    /// (Enhance already-applied metadata) go to `Recipe::provenance` verbatim. Original XMP is
    /// retained in `Recipe::unknown["sidecar_xmp"]` for lossless later export via
    /// `from_imported_recipe`.
    pub fn to_recipe(&self) -> EngineResult<ImportedRecipe> {
        self.decode_recipe(true, true)
    }

    /// Decode ordinary CRS fields into an uncommitted catalog import transaction.
    /// The catalog supplies its authoritative process version and geometry hooks,
    /// then records history once, after all hooks have completed.
    pub fn to_catalog_recipe(&self) -> EngineResult<ImportedRecipe> {
        self.decode_recipe(false, true)
    }

    /// Decode a catalog transaction with audited foreign mask extensions enabled or disabled.
    pub fn to_catalog_recipe_with_foreign_mask_extensions(
        &self,
        extensions: bool,
    ) -> EngineResult<ImportedRecipe> {
        self.decode_recipe(false, extensions)
    }

    fn decode_recipe(
        &self,
        standalone: bool,
        foreign_mask_extensions: bool,
    ) -> EngineResult<ImportedRecipe> {
        let tree = Tree::parse(&self.xml)?;
        let mut recipe = Recipe {
            selection: self.selection()?,
            ..Recipe::default()
        };
        let mut value = serde_json::to_value(&recipe)?;
        let mut warnings = Vec::new();
        for &key in CrsKey::ALL {
            let ns = key.namespace().uri();
            if tree.property(ns, key.xmp_name()).is_none() {
                continue;
            }
            if key.is_informational() {
                let raw = tree.value(ns, key.xmp_name()).unwrap_or_default();
                recipe
                    .provenance
                    .properties
                    .insert(key.qualified_name(), raw);
                continue;
            }
            if let Err(e) = decode(key, &tree, &mut value, foreign_mask_extensions) {
                warnings.push(format!("{key}: {e}; retained in original XMP"));
            }
        }
        let mut gray = engine_api::recipe::settings::MonochromeSettings::default();
        let mut has_gray = false;
        if let Some(raw) = tree.value(CRS, "ConvertToGrayscale") {
            match bool_value(&raw) {
                Ok(enabled) => {
                    gray.enabled = enabled;
                    has_gray = true;
                }
                Err(e) => warnings.push(format!(
                    "crs:ConvertToGrayscale: {e}; retained in original XMP"
                )),
            }
        }
        for (key, target) in [
            "GrayMixerRed",
            "GrayMixerOrange",
            "GrayMixerYellow",
            "GrayMixerGreen",
            "GrayMixerAqua",
            "GrayMixerBlue",
            "GrayMixerPurple",
            "GrayMixerMagenta",
        ]
        .into_iter()
        .zip([
            &mut gray.mixer.red,
            &mut gray.mixer.orange,
            &mut gray.mixer.yellow,
            &mut gray.mixer.green,
            &mut gray.mixer.aqua,
            &mut gray.mixer.blue,
            &mut gray.mixer.purple,
            &mut gray.mixer.magenta,
        ]) {
            if let Some(raw) = tree.value(CRS, key) {
                match raw
                    .parse::<f32>()
                    .ok()
                    .filter(|v| v.is_finite() && (-100. ..=100.).contains(v))
                {
                    Some(v) => {
                        *target = v;
                        has_gray = true;
                    }
                    None => warnings.push(format!(
                        "crs:{key}: invalid mixer amount; retained in original XMP"
                    )),
                }
            }
        }
        if has_gray && (gray.enabled || gray.mixer != Default::default()) {
            value["settings"]["color"]["monochrome"] = serde_json::to_value(gray)?;
        }
        let settings: DevelopSettings = serde_json::from_value(value["settings"].clone())?;
        recipe.process_version = serde_json::from_value(value["process_version"].clone())?;
        recipe.settings = settings;
        recipe.ids.next_mask = recipe
            .settings
            .locals
            .adjustments
            .iter()
            .map(|a| a.id.0 + 1)
            .max()
            .unwrap_or(0);
        recipe
            .unknown
            .insert("sidecar_xmp".into(), Value::String(self.xml.clone()));
        let properties: Vec<_> = geometry_keys()
            .into_iter()
            .filter_map(|key| tree.value(CRS, key).map(|v| (key, v)))
            .collect();
        let mut source = serde_json::Map::new();
        for desc in tree.descriptions() {
            for a in &desc.attrs {
                if a.ns == CRS && geometry_keys().contains(&a.local.as_str()) {
                    source.insert(a.local.clone(), json!(&self.xml[a.span.clone()]));
                }
            }
            for &i in &desc.children {
                let n = &tree.nodes[i];
                if n.ns == CRS && geometry_keys().contains(&n.local.as_str()) {
                    source.insert(n.local.clone(), json!(&self.xml[n.span.clone()]));
                }
            }
        }
        if standalone {
            let decoded = crate::geometry::apply(
                &mut recipe,
                &mut warnings,
                properties.iter().map(|(k, v)| (*k, v.as_str())),
            )?;
            if decoded
                .iter()
                .any(|entry| matches!(entry.kind, crate::GeometryEntryKind::Approximate { .. }))
                && !source.is_empty()
            {
                recipe.unknown.insert(
                    "lrcat_develop_source".into(),
                    json!({"shape":"xmp-fragments","properties":source}),
                );
            }
        }
        if standalone
            && tree.value(PRIVATE, "ExportHash").as_deref() == Some(crs_hash(&tree)?.as_str())
            && let Some(raw) = tree.value(PRIVATE, "GeometryLens")
        {
            let v: Value = serde_json::from_str(&raw)?;
            recipe.settings.geometry.upright = serde_json::from_value(v["upright"].clone())?;
            recipe.settings.lens.legacy_ca_red =
                serde_json::from_value(v["legacy_ca_red"].clone())?;
            recipe.settings.lens.legacy_ca_blue =
                serde_json::from_value(v["legacy_ca_blue"].clone())?;
        }
        if standalone {
            recipe.history.record(
                &recipe.history.base.clone(),
                &recipe.settings,
                EditMeta {
                    label: "Import XMP".into(),
                    author: Author::Import {
                        source: "xmp".into(),
                    },
                    ..EditMeta::default()
                },
            )?;
        }
        if standalone {
            recipe.validate()?;
        }
        Ok(ImportedRecipe { recipe, warnings })
    }
    /// Export a recipe imported from XMP without losing foreign properties.
    pub fn from_imported_recipe(recipe: &Recipe, preset: &MarkPreset) -> EngineResult<Self> {
        let source = recipe
            .unknown
            .get("sidecar_xmp")
            .and_then(Value::as_str)
            .ok_or_else(|| EngineError::invalid("recipe", "no source XMP"))?;
        let packet = Self::parse(source)?;
        packet
            .with_metadata(&recipe.selection, &packet.metadata()?, preset)?
            .with_recipe(recipe)
    }
}
/// Namespace-resolved, length-delimited JSON makes prefix/attribute ordering irrelevant.
/// Include unknown CRS properties and complete nested trees, not only mapped sliders.
/// Array order remains significant. This is an edit detector, not authentication.
fn crs_hash(tree: &Tree) -> EngineResult<String> {
    fn node_value(tree: &Tree, n: &Node) -> Value {
        if n.children.is_empty() && n.attrs.is_empty() {
            return json!(n.text.trim());
        }
        let mut attrs: Vec<_> = n
            .attrs
            .iter()
            .map(|a| (&a.ns, &a.local, &a.value))
            .collect();
        attrs.sort();
        let children: Vec<_> = n
            .children
            .iter()
            .map(|i| {
                let c = &tree.nodes[*i];
                json!([c.ns, c.local, node_value(tree, c)])
            })
            .collect();
        json!([n.text.trim(), attrs, children])
    }
    let mut properties = Vec::new();
    for desc in tree.descriptions() {
        for a in &desc.attrs {
            if a.ns == CRS {
                properties.push(json!([a.local, a.value.trim()]));
            }
        }
        for i in &desc.children {
            let n = &tree.nodes[*i];
            if n.ns == CRS {
                properties.push(json!([n.local, node_value(tree, n)]));
            }
        }
    }
    properties.sort_by_cached_key(Value::to_string);
    Ok(blake3::hash(&serde_json::to_vec(&properties)?)
        .to_hex()
        .to_string())
}

fn unsupported(key: CrsKey) -> EngineError {
    EngineError::Unsupported {
        what: format!("{key} cannot be translated losslessly"),
    }
}
fn bool_value(s: &str) -> EngineResult<bool> {
    match s.trim() {
        "True" | "true" | "1" => Ok(true),
        "False" | "false" | "0" => Ok(false),
        _ => Err(error(format!("invalid boolean {s}"))),
    }
}
fn number(s: &str) -> EngineResult<f64> {
    let n: f64 = s.trim().parse().map_err(error)?;
    if !n.is_finite() {
        return Err(error("nonfinite number"));
    }
    Ok(n)
}
fn enum_value(s: &str, choices: &[&str]) -> EngineResult<Value> {
    let i: usize = s.trim().parse().map_err(error)?;
    choices
        .get(i)
        .map(|s| json!(s))
        .ok_or_else(|| error("invalid enumeration"))
}
fn decode(
    key: CrsKey,
    tree: &Tree,
    doc: &mut Value,
    foreign_mask_extensions: bool,
) -> EngineResult<()> {
    use CrsKey::*;
    let path = key.recipe_path().ok_or_else(|| unsupported(key))?;
    let s = tree
        .value(key.namespace().uri(), key.xmp_name())
        .unwrap_or_default();
    let target = doc.pointer_mut(path).ok_or_else(|| error(path))?;
    let next = match key {
        ProcessVersion => serde_json::to_value(engine_api::recipe::ProcessVersion::from_xmp(
            &s,
            if tree.value(PRIVATE, "ExportHash").as_deref() == Some(crs_hash(tree)?.as_str()) {
                tree.value(PRIVATE, NATIVE_REVISION_PROPERTY)
            } else {
                None
            }
            .as_deref(),
        )?)?,
        WhiteBalance => json!(if s == "As Shot" {
            "as_shot".into()
        } else {
            s.to_lowercase()
        }),
        PostCropVignetteStyle => enum_value(
            &s,
            &["", "highlight_priority", "color_priority", "paint_overlay"],
        )?,
        PerspectiveUpright => {
            enum_value(&s, &["off", "auto", "full", "level", "vertical", "guided"])?
        }
        DefringePurpleHueLo | DefringeGreenHueLo | DefringePurpleHueHi | DefringeGreenHueHi => {
            let index = usize::from(key.xmp_name().ends_with("Hi"));
            target[index] = json!(number(&s)? * 3.6);
            return Ok(());
        }
        HasCrop => {
            bool_value(&s)?;
            return Ok(());
        }
        // The five profile keys jointly decode to one value; each computes the same result.
        LensProfileEnable | LensProfileSetup | LensProfileName | LensProfileFilename
        | LensProfileDigest => match lens_profile(tree)? {
            Some(v) => v,
            None => return Ok(()),
        },
        MaskGroupBasedCorrections => masks::import_masks(tree, foreign_mask_extensions)?,
        PointColors | LensBlur | RetouchAreas | RetouchInfo => structures::decode(key, tree)?,
        Look => {
            let Some(Property::Node(n)) = tree.property(CRS, key.xmp_name()) else {
                return Err(unsupported(key));
            };
            let name = field(tree, n, "Name");
            if name.is_empty() && field(tree, n, "Amount").is_empty() {
                Value::Null
            } else {
                json!({"style":name,"amount":num_field(tree,n,"Amount",1.0)?*100.0})
            }
        }
        _ => match key.value_type() {
            CrsValueType::PointList => {
                let Some(Property::Node(root)) = tree.property(CRS, key.xmp_name()) else {
                    return Err(error("curve requires RDF sequence"));
                };
                let points = tree
                    .items(root)
                    .iter()
                    .map(|p| {
                        let (x, y) = p
                            .text
                            .split_once(',')
                            .ok_or_else(|| error("bad curve point"))?;
                        let mut xy = [number(x)? / 255.0, number(y)? / 255.0];
                        for (i, name) in ["x", "y"].iter().enumerate() {
                            if let Some(a) =
                                p.attrs.iter().find(|a| a.ns == PRIVATE && a.local == *name)
                            {
                                let precise = number(&a.value)?;
                                // Native sub-grid precision survives only while Adobe's visible
                                // point still matches. External curve edits take precedence.
                                if (precise * 255.0).round() == xy[i] * 255.0 {
                                    xy[i] = precise;
                                }
                            }
                        }
                        Ok(json!({"x":xy[0],"y":xy[1]}))
                    })
                    .collect::<EngineResult<Vec<_>>>()?;
                json!(points)
            }
            CrsValueType::Structure => {
                let Some(Property::Node(n)) = tree.property(CRS, key.xmp_name()) else {
                    return Err(unsupported(key));
                };
                if tree.items(n).is_empty()
                    && n.children.iter().all(|i| {
                        let n = &tree.nodes[*i];
                        n.ns == RDF && n.children.is_empty()
                    })
                {
                    return Ok(());
                }
                return Err(unsupported(key));
            }
            CrsValueType::Boolean => json!(bool_value(&s)?),
            CrsValueType::Integer { .. } | CrsValueType::Real { .. } => {
                if target.is_boolean() {
                    json!(bool_value(&s)?)
                } else {
                    json!(number(&s)?)
                }
            }
            _ => json!(s),
        },
    };
    // Check the resulting whole settings before accepting a key; bad enums do not poison other fields.
    let old = std::mem::replace(target, next);
    if let Err(e) = serde_json::from_value::<Recipe>(doc.clone()) {
        *doc.pointer_mut(path).ok_or_else(|| error(path))? = old;
        return Err(e.into());
    }
    Ok(())
}
fn encode(key: CrsKey, doc: &Value) -> EngineResult<String> {
    use CrsKey::*;
    let v = doc
        .pointer(key.recipe_path().ok_or_else(|| unsupported(key))?)
        .ok_or_else(|| error(key))?;
    let scalar = match key {
        ProcessVersion => {
            let pv: engine_api::recipe::ProcessVersion = serde_json::from_value(v.clone())?;
            // Native recipes export as best-effort PV6 plus the ts:NativeRevision companion.
            let crs = pv.crs_value().ok_or_else(|| unsupported(key))?;
            let mut out = text("crs:ProcessVersion", crs.process_version);
            if let Some(revision) = crs.native_revision {
                out += &text(
                    &format!("ts:{NATIVE_REVISION_PROPERTY}"),
                    &revision.to_string(),
                );
            }
            return Ok(out);
        }
        WhiteBalance => {
            if v == "as_shot" {
                "As Shot".into()
            } else {
                let s = v.as_str().ok_or_else(|| error(key))?;
                let mut c = s.chars();
                c.next()
                    .map(|f| f.to_uppercase().collect::<String>() + c.as_str())
                    .unwrap_or_default()
            }
        }
        PostCropVignetteStyle => enum_number(
            v,
            &["", "highlight_priority", "color_priority", "paint_overlay"],
        )?,
        PerspectiveUpright => {
            enum_number(v, &["off", "auto", "full", "level", "vertical", "guided"])?
        }
        DefringePurpleHueLo | DefringeGreenHueLo | DefringePurpleHueHi | DefringeGreenHueHi => {
            let i = usize::from(key.xmp_name().ends_with("Hi"));
            (v[i].as_f64().ok_or_else(|| error(key))? / 3.6).to_string()
        }
        HasCrop => if v == &json!({"left":0.0,"top":0.0,"right":1.0,"bottom":1.0}) {
            "False"
        } else {
            "True"
        }
        .into(),
        LensProfileEnable => if v["kind"] == "none" { "0" } else { "1" }.into(),
        LensProfileSetup => if v["kind"] == "database" {
            serde_json::from_value::<engine_api::recipe::LensProfileSetup>(
                v["profile"]["setup"].clone(),
            )?
            .crs_value()
        } else {
            "Auto"
        }
        .into(),
        LensProfileName | LensProfileFilename | LensProfileDigest => {
            let field = match key {
                LensProfileName => "name",
                LensProfileFilename => "filename",
                _ => "digest",
            };
            v["profile"][field].as_str().unwrap_or("").into()
        }
        MaskGroupBasedCorrections => return masks::export_masks(v),
        PointColors | LensBlur | RetouchAreas | RetouchInfo => return structures::encode(key, v),
        Look => {
            let name = format!("crs:{key}").replace("crs:crs:", "crs:");
            let body = if v.is_null() {
                String::new()
            } else {
                text("crs:Name", v["style"].as_str().unwrap_or(""))
                    + &text(
                        "crs:Amount",
                        &(v["amount"].as_f64().unwrap_or(100.0) / 100.0).to_string(),
                    )
            };
            return Ok(format!(
                "<{name} rdf:parseType=\"Resource\">{body}</{name}>"
            ));
        }
        _ => match key.value_type() {
            CrsValueType::PointList => {
                let points = v
                    .as_array()
                    .ok_or_else(|| error(key))?
                    .iter()
                    .map(|p| {
                        format!(
                            "<rdf:li ts:x=\"{}\" ts:y=\"{}\">{}, {}</rdf:li>",
                            p["x"],
                            p["y"],
                            (p["x"].as_f64().unwrap_or(0.0) * 255.0).round(),
                            (p["y"].as_f64().unwrap_or(0.0) * 255.0).round()
                        )
                    })
                    .collect::<Vec<_>>();
                let name = key.qualified_name();
                return Ok(format!(
                    "<{name}><rdf:Seq>{}</rdf:Seq></{name}>",
                    points.concat()
                ));
            }
            CrsValueType::Structure => {
                if !v.is_null() && v.as_array().is_none_or(|a| !a.is_empty()) {
                    return Err(unsupported(key));
                }
                return Ok(container(&format!("crs:{}", key.xmp_name()), "Seq", &[]));
            }
            CrsValueType::Boolean => if v.as_bool().unwrap_or(false) {
                "True"
            } else {
                "False"
            }
            .into(),
            _ if v.is_boolean() => if v.as_bool().unwrap_or(false) {
                "1"
            } else {
                "0"
            }
            .into(),
            _ if v.is_string() => v.as_str().unwrap_or("").into(),
            _ => v.to_string(),
        },
    };
    Ok(text(&format!("crs:{}", key.xmp_name()), &scalar))
}
/// `LensProfileSource` from all five `crs:LensProfile*` properties, or `None` to keep the
/// current value. A disabled profile wins; any identifying field yields a named profile.
fn lens_profile(tree: &Tree) -> EngineResult<Option<Value>> {
    let get = |name: &str| tree.value(CRS, name).unwrap_or_default();
    let source = if tree.value(PRIVATE, "ExportHash").as_deref() == Some(crs_hash(tree)?.as_str()) {
        tree.value(PRIVATE, "LensProfileSource").unwrap_or_default()
    } else {
        String::new()
    };
    if matches!(source.as_str(), "embedded" | "auto_calibrated") {
        return Ok(Some(json!({"kind":source})));
    }
    let enable = get("LensProfileEnable");
    if !enable.is_empty() && !bool_value(&enable)? {
        return Ok(Some(json!({"kind":"none"})));
    }
    let setup_text = get("LensProfileSetup");
    let setup = if setup_text.is_empty() {
        LensProfileSetup::default()
    } else {
        LensProfileSetup::from_crs(&setup_text)
            .ok_or_else(|| error(format!("invalid LensProfileSetup {setup_text}")))?
    };
    let (name, filename, digest) = (
        get("LensProfileName"),
        get("LensProfileFilename"),
        get("LensProfileDigest"),
    );
    if source == "database" || !(name.is_empty() && filename.is_empty() && digest.is_empty()) {
        return Ok(Some(json!({
            "kind": "database",
            "profile": {"name": name, "filename": filename, "digest": digest, "setup": setup},
        })));
    }
    Ok((!enable.is_empty() || !setup_text.is_empty()).then(|| json!({"kind":"auto"})))
}
fn enum_number(v: &Value, values: &[&str]) -> EngineResult<String> {
    values
        .iter()
        .position(|s| v == *s)
        .map(|i| i.to_string())
        .ok_or_else(|| error("invalid enum"))
}
fn resource<'a>(tree: &'a Tree, n: &'a Node) -> &'a Node {
    n.children
        .iter()
        .map(|i| &tree.nodes[*i])
        .find(|n| n.ns == RDF && n.local == "Description")
        .unwrap_or(n)
}
fn field(tree: &Tree, n: &Node, name: &str) -> String {
    let n = resource(tree, n);
    n.attrs
        .iter()
        .find(|a| a.ns == CRS && a.local == name)
        .map(|a| a.value.clone())
        .or_else(|| {
            n.children
                .iter()
                .map(|i| &tree.nodes[*i])
                .find(|c| c.ns == CRS && c.local == name)
                .map(|c| c.text.clone())
        })
        .unwrap_or_default()
}
fn num_field(tree: &Tree, n: &Node, name: &str, default: f64) -> EngineResult<f64> {
    let s = field(tree, n, name);
    if s.is_empty() {
        Ok(default)
    } else {
        number(&s)
    }
}

fn upright_index(mode: engine_api::recipe::settings::UprightMode) -> usize {
    use engine_api::recipe::settings::UprightMode::*;
    match mode {
        Off => 0,
        Auto => 1,
        Full => 2,
        Level => 3,
        Vertical => 4,
        Guided => 5,
    }
}
fn geometry_keys() -> Vec<&'static str> {
    vec![
        "ChromaticAberrationR",
        "ChromaticAberrationB",
        "UprightTransform_0",
        "UprightTransform_1",
        "UprightTransform_2",
        "UprightTransform_3",
        "UprightTransform_4",
        "UprightTransform_5",
        "UprightFourSegmentsCount",
        "UprightFourSegments_0",
        "UprightFourSegments_1",
        "UprightFourSegments_2",
        "UprightFourSegments_3",
        "UprightCenterMode",
        "UprightCenterNormX",
        "UprightCenterNormY",
        "UprightFocalMode",
        "UprightFocalLength35mm",
        "EnableDistractionRemoval",
        "GenerativeRemove",
        "GenerativeFill",
    ]
}
