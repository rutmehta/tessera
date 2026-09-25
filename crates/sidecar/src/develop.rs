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
            if tree.property(ns, key.xmp_name()).is_some()
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
        Self::parse(tree.replace(&self.xml, &owned, &body)?)
    }
    /// Import all table keys, recording a single valid history edit. Informational keys
    /// (Enhance already-applied metadata) go to `Recipe::provenance` verbatim. Original XMP is
    /// retained in `Recipe::unknown["sidecar_xmp"]` for lossless later export via
    /// `from_imported_recipe`.
    pub fn to_recipe(&self) -> EngineResult<ImportedRecipe> {
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
            if let Err(e) = decode(key, &tree, &mut value) {
                warnings.push(format!("{key}: {e}; retained in original XMP"));
            }
        }
        let settings: DevelopSettings = serde_json::from_value(value["settings"].clone())?;
        recipe.process_version = serde_json::from_value(value["process_version"].clone())?;
        recipe.edit(
            EditMeta {
                label: "Import XMP".into(),
                author: Author::Import {
                    source: "xmp".into(),
                },
                ..EditMeta::default()
            },
            |s| *s = settings,
        )?;
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
        recipe.validate()?;
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
fn decode(key: CrsKey, tree: &Tree, doc: &mut Value) -> EngineResult<()> {
    use CrsKey::*;
    let path = key.recipe_path().ok_or_else(|| unsupported(key))?;
    let s = tree
        .value(key.namespace().uri(), key.xmp_name())
        .unwrap_or_default();
    let target = doc.pointer_mut(path).ok_or_else(|| error(path))?;
    let next = match key {
        ProcessVersion => serde_json::to_value(engine_api::recipe::ProcessVersion::from_xmp(
            &s,
            tree.value(PRIVATE, NATIVE_REVISION_PROPERTY).as_deref(),
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
        MaskGroupBasedCorrections => import_masks(tree)?,
        Look => {
            let Some(Property::Node(n)) = tree.property(CRS, key.xmp_name()) else {
                return Err(unsupported(key));
            };
            let name = field(tree, n, "Name");
            if name.is_empty() {
                Value::Null
            } else {
                json!({"style":name,"amount":num_field(tree,n,"Amount",1.0)?*100.0})
            }
        }
        _ => match key.value_type() {
            CrsValueType::PointList => {
                let points = tree
                    .list(CRS, key.xmp_name())
                    .iter()
                    .map(|p| {
                        let (x, y) = p.split_once(',').ok_or_else(|| error("bad curve point"))?;
                        Ok(json!({"x":number(x)?/255.0,"y":number(y)?/255.0}))
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
        MaskGroupBasedCorrections => return export_masks(v),
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
                            "{}, {}",
                            (p["x"].as_f64().unwrap_or(0.0) * 255.0).round(),
                            (p["y"].as_f64().unwrap_or(0.0) * 255.0).round()
                        )
                    })
                    .collect::<Vec<_>>();
                return Ok(container(
                    &format!("crs:{}", key.xmp_name()),
                    "Seq",
                    &points,
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
    if !(name.is_empty() && filename.is_empty() && digest.is_empty()) {
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
fn import_masks(tree: &Tree) -> EngineResult<Value> {
    let Some(Property::Node(n)) = tree.property(CRS, "MaskGroupBasedCorrections") else {
        return Err(error("masks require a sequence"));
    };
    let mut masks = Vec::new();
    for (id, item) in tree.items(n).iter().enumerate() {
        let n = resource(tree, item);
        let mut local = serde_json::to_value(engine_api::recipe::LocalAdjustment::default())?;
        local["id"] = json!(id);
        local["name"] = json!(field(tree, n, "CorrectionName"));
        local["enabled"] = json!(!matches!(
            field(tree, n, "CorrectionActive").as_str(),
            "false" | "False" | "0"
        ));
        local["amount"] = json!(num_field(tree, n, "CorrectionAmount", 1.0)? * 100.0);
        for (name, path) in LOCAL_PARAMS {
            let s = field(tree, n, name);
            if !s.is_empty() {
                local["params"][*path] = json!(number(&s)?);
            }
        }
        let mut components = Vec::new();
        if let Some(m) = n
            .children
            .iter()
            .map(|i| &tree.nodes[*i])
            .find(|c| c.ns == CRS && c.local == "CorrectionMasks")
        {
            for c in tree.items(m) {
                if matches!(
                    field(tree, c, "MaskActive").as_str(),
                    "False" | "false" | "0"
                ) {
                    continue;
                }
                let kind = field(tree, c, "What");
                let mut component = match kind.as_str() {
                    "Mask/Gradient" => {
                        json!({"kind":"linear", "start":[num_field(tree,c,"FullX",0.0)?,num_field(tree,c,"FullY",0.0)?], "end":[num_field(tree,c,"ZeroX",1.0)?,num_field(tree,c,"ZeroY",1.0)?]})
                    }
                    _ => return Err(error(format!("unsupported mask kind {kind}"))),
                };
                component["invert"] = json!(matches!(
                    field(tree, c, "MaskInverted").as_str(),
                    "true" | "True" | "1"
                ));
                component["combine"] = json!("add");
                components.push(component);
            }
        }
        local["components"] = json!(components);
        masks.push(local);
    }
    Ok(json!(masks))
}
const LOCAL_PARAMS: &[(&str, &str)] = &[
    ("LocalExposure2012", "exposure"),
    ("LocalContrast2012", "contrast"),
    ("LocalHighlights2012", "highlights"),
    ("LocalShadows2012", "shadows"),
    ("LocalWhites2012", "whites"),
    ("LocalBlacks2012", "blacks"),
    ("LocalTemperature", "temperature"),
    ("LocalTint", "tint"),
    ("LocalHue", "hue"),
    ("LocalSaturation", "saturation"),
    ("LocalTexture", "texture"),
    ("LocalClarity2012", "clarity"),
    ("LocalDehaze", "dehaze"),
    ("LocalSharpness", "sharpness"),
    ("LocalLuminanceNoise", "noise"),
    ("LocalMoire", "moire"),
    ("LocalDefringe", "defringe"),
];
fn export_masks(v: &Value) -> EngineResult<String> {
    let mut items = String::new();
    for local in v.as_array().ok_or_else(|| error("mask array"))? {
        let mut body = text("crs:What", "Correction")
            + &text("crs:CorrectionName", local["name"].as_str().unwrap_or(""))
            + &text("crs:CorrectionActive", &local["enabled"].to_string())
            + &text(
                "crs:CorrectionAmount",
                &(local["amount"].as_f64().unwrap_or(100.0) / 100.0).to_string(),
            );
        for (name, path) in LOCAL_PARAMS {
            body += &text(&format!("crs:{name}"), &local["params"][*path].to_string());
        }
        let mut components = String::new();
        for c in local["components"]
            .as_array()
            .ok_or_else(|| error("mask components"))?
        {
            if c["kind"] != "linear" || c["combine"] != "add" {
                return Err(unsupported(CrsKey::MaskGroupBasedCorrections));
            }
            let mut b = text("crs:What", "Mask/Gradient")
                + &text("crs:MaskActive", "true")
                + &text("crs:MaskInverted", &c["invert"].to_string());
            for (key, point, idx) in [
                ("FullX", "start", 0),
                ("FullY", "start", 1),
                ("ZeroX", "end", 0),
                ("ZeroY", "end", 1),
            ] {
                b += &text(&format!("crs:{key}"), &c[point][idx].to_string());
            }
            components += &format!("<rdf:li rdf:parseType=\"Resource\">{b}</rdf:li>");
        }
        body +=
            &format!("<crs:CorrectionMasks><rdf:Seq>{components}</rdf:Seq></crs:CorrectionMasks>");
        items += &format!("<rdf:li rdf:parseType=\"Resource\">{body}</rdf:li>");
    }
    Ok(format!(
        "<crs:MaskGroupBasedCorrections><rdf:Seq>{items}</rdf:Seq></crs:MaskGroupBasedCorrections>"
    ))
}
