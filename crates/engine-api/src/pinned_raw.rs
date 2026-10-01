//! Pure immutable declaration for a captured RAW input and its pinned recipe snapshot.
use crate::{
    error::{EngineError, EngineResult},
    id::{Digest, ImageId},
    recipe::{ProcessFamily, ProcessVersion, Recipe, RecipeHash, SourceKind},
    stage::canonical_json,
};
use serde::{
    de::{MapAccess, SeqAccess, Visitor},
    Deserialize, Deserializer, Serialize,
};
use serde_json::Value;
use std::collections::BTreeSet;

/// Supported closed RAW decoding route declaration.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PinnedRawDecoderRoute {
    /// LibRaw CFA decode path revision 1.
    LibRawCfaV1,
}

/// Untrusted pinned-input declarations. Construction does not inspect asset bytes or paths.
#[derive(Debug, Clone)]
pub struct PinnedRawInput {
    /// Declared captured asset digest.
    pub asset_digest: Digest,
    /// Declared positive byte length.
    pub asset_byte_len: u64,
    /// Owner named by the embedded recipe.
    pub recipe_image_id: ImageId,
    /// Exact UTF-8 recipe JSON payload.
    pub recipe_json: Vec<u8>,
    /// Closed decoder route.
    pub decoder_route: PinnedRawDecoderRoute,
    /// Normalized file extension hint.
    pub suffix_hint: String,
    /// Optional non-authoritative locator hint.
    pub locator_hint: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Wire {
    descriptor_version: u32,
    asset_digest: Digest,
    asset_byte_len: u64,
    recipe_image_id: ImageId,
    recipe_json: String,
    decoder_route: PinnedRawDecoderRoute,
    suffix_hint: String,
    #[serde(default)]
    locator_hint: Option<String>,
    recipe_hash: RecipeHash,
}

/// Validated immutable declaration; this proves structure only, not file contents or render eligibility.
#[derive(Debug, Clone)]
pub struct PinnedRawDescriptor {
    wire: Wire,
    recipe_json: Vec<u8>,
    input_identity: Digest,
}
impl PinnedRawDescriptor {
    /// Validate declarations and an explicit current-settings RAW recipe snapshot.
    pub fn new(mut input: PinnedRawInput) -> EngineResult<Self> {
        if input.asset_byte_len == 0 {
            return Err(bad("asset_byte_len", "must be positive"));
        }
        input.suffix_hint.make_ascii_lowercase();
        if input.suffix_hint.is_empty()
            || input.suffix_hint.len() > 16
            || !input
                .suffix_hint
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit())
        {
            return Err(bad(
                "suffix_hint",
                "expected 1–16 ASCII alphanumeric characters",
            ));
        }
        check_duplicates(&input.recipe_json)?;
        let value: Value = serde_json::from_slice(&input.recipe_json)
            .map_err(|e| bad("recipe_json", e.to_string()))?;
        let obj = value
            .as_object()
            .ok_or_else(|| bad("recipe_json", "expected object"))?;
        let writable = u64::from(crate::recipe::RECIPE_SCHEMA_VERSION)
            ..=u64::from(crate::recipe::max_writable_schema_version());
        if !obj
            .get("schema_version")
            .and_then(Value::as_u64)
            .is_some_and(|v| writable.contains(&v))
        {
            return Err(bad(
                "schema_version",
                "explicit writable raw recipe schema (3 or a supported conditional 4) required",
            ));
        }
        if obj
            .get("image_id")
            .and_then(Value::as_str)
            .and_then(|s| s.parse::<ImageId>().ok())
            != Some(input.recipe_image_id)
        {
            return Err(bad("image_id", "must explicitly match recipe owner"));
        }
        if obj.get("source_kind").and_then(Value::as_str) != Some("raw") {
            return Err(bad("source_kind", "explicit raw source required"));
        }
        let pv = obj
            .get("process_version")
            .and_then(Value::as_object)
            .ok_or_else(|| bad("process_version", "explicit process header required"))?;
        if pv.get("family").and_then(Value::as_str) != Some("native")
            || pv.get("revision").and_then(Value::as_u64) != Some(2)
        {
            return Err(bad("process_version", "native revision 2 required"));
        }
        let raw_settings = obj
            .get("settings")
            .ok_or_else(|| bad("settings", "explicit current settings required"))?;
        let settings: crate::recipe::DevelopSettings = serde_json::from_value(raw_settings.clone())
            .map_err(|e| bad("settings", e.to_string()))?;
        let typed = serde_json::to_value(&settings).map_err(|e| bad("settings", e.to_string()))?;
        check_shape(raw_settings, &typed, "settings")?;
        let mut recipe = Recipe::new(input.recipe_image_id);
        recipe.source_kind = SourceKind::Raw;
        recipe.process_version = ProcessVersion {
            family: ProcessFamily::Native,
            revision: 2,
        };
        recipe.settings = settings;
        if recipe.settings.geometry != Default::default() {
            return Err(bad(
                "settings.geometry",
                "pinned RAW requires default geometry",
            ));
        }
        let hash = recipe.recipe_hash();
        let recipe_json = input.recipe_json;
        let wire = Wire {
            descriptor_version: 1,
            asset_digest: input.asset_digest,
            asset_byte_len: input.asset_byte_len,
            recipe_image_id: input.recipe_image_id,
            recipe_json: String::from_utf8(recipe_json.clone())
                .map_err(|e| bad("recipe_json", e.to_string()))?,
            decoder_route: input.decoder_route,
            suffix_hint: input.suffix_hint,
            locator_hint: input.locator_hint,
            recipe_hash: hash,
        };
        let identity = identity(&wire, &recipe_json);
        Ok(Self {
            wire,
            recipe_json,
            input_identity: identity,
        })
    }
    /// Parse and validate the descriptor wire representation.
    pub fn from_json(bytes: &[u8]) -> EngineResult<Self> {
        check_duplicates(bytes)?;
        let wire: Wire =
            serde_json::from_slice(bytes).map_err(|e| bad("descriptor", e.to_string()))?;
        if wire.descriptor_version != 1 {
            return Err(bad("descriptor_version", "unsupported version"));
        }
        let input = PinnedRawInput {
            asset_digest: wire.asset_digest,
            asset_byte_len: wire.asset_byte_len,
            recipe_image_id: wire.recipe_image_id,
            recipe_json: wire.recipe_json.as_bytes().to_vec(),
            decoder_route: wire.decoder_route,
            suffix_hint: wire.suffix_hint.clone(),
            locator_hint: wire.locator_hint.clone(),
        };
        let mut parsed = Self::new(input)?;
        if parsed.wire.recipe_hash != wire.recipe_hash {
            return Err(bad("recipe_hash", "does not match current settings"));
        }
        parsed.wire = wire;
        parsed.wire.suffix_hint.make_ascii_lowercase();
        parsed.input_identity = identity(&parsed.wire, &parsed.recipe_json);
        Ok(parsed)
    }
    /// Serialize the validated descriptor.
    pub fn to_json(&self) -> EngineResult<Vec<u8>> {
        serde_json::to_vec_pretty(&self.wire).map_err(Into::into)
    }
    /// Exact supplied recipe JSON bytes.
    pub fn recipe_json(&self) -> &[u8] {
        &self.recipe_json
    }
    /// Domain-separated captured-input request identity; locator is excluded.
    pub fn input_identity(&self) -> Digest {
        self.input_identity
    }
}
fn identity(w: &Wire, raw: &[u8]) -> Digest {
    #[derive(Serialize)]
    struct Id<'a> {
        asset_digest: Digest,
        asset_byte_len: u64,
        recipe_image_id: ImageId,
        payload_digest: Digest,
        decoder_route: &'a PinnedRawDecoderRoute,
        suffix: &'a str,
    }
    let x = Id {
        asset_digest: w.asset_digest,
        asset_byte_len: w.asset_byte_len,
        recipe_image_id: w.recipe_image_id,
        payload_digest: Digest::derive("tessera pinned RAW recipe payload v1", raw),
        decoder_route: &w.decoder_route,
        suffix: &w.suffix_hint,
    };
    Digest::derive("tessera pinned RAW input identity v1", &canonical_json(&x))
}
fn bad(name: &str, reason: impl Into<String>) -> EngineError {
    EngineError::invalid(name, reason)
}
fn check_shape(raw: &Value, typed: &Value, path: &str) -> EngineResult<()> {
    match (raw, typed) {
        (Value::Object(a), Value::Object(b)) => {
            for (k, v) in a {
                let t = b
                    .get(k)
                    .ok_or_else(|| bad(path, format!("unknown current-settings key {k}")))?;
                check_shape(v, t, &format!("{path}.{k}"))?;
            }
        }
        (Value::Array(a), Value::Array(b)) => {
            if a.len() != b.len() {
                return Err(bad(path, "current-settings array value was not preserved"));
            }
            for (i, (v, t)) in a.iter().zip(b).enumerate() {
                check_shape(v, t, &format!("{path}[{i}]"))?;
            }
        }
        // These two profile references intentionally accept their legacy schema-1 bare-name form.
        (Value::String(_), Value::Object(_))
            if path == "settings.camera_profile.profile"
                || path == "settings.lens.profile.profile" => {}
        (Value::Number(_), Value::Number(_))
        | (Value::String(_), Value::String(_))
        | (Value::Bool(_), Value::Bool(_))
        | (Value::Null, Value::Null) => {}
        (Value::Number(_), Value::Null) => {
            return Err(bad(
                path,
                "numeric setting cannot be represented by the current settings type",
            ))
        }
        _ => return Err(bad(path, "current-settings value shape was not preserved")),
    }
    Ok(())
}

struct StrictValue(Value);
impl<'de> Deserialize<'de> for StrictValue {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        struct V;
        impl<'de> Visitor<'de> for V {
            type Value = StrictValue;
            fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                f.write_str("JSON value with unique object keys")
            }
            fn visit_bool<E: serde::de::Error>(self, v: bool) -> Result<Self::Value, E> {
                Ok(StrictValue(Value::Bool(v)))
            }
            fn visit_i64<E: serde::de::Error>(self, v: i64) -> Result<Self::Value, E> {
                Ok(StrictValue(Value::from(v)))
            }
            fn visit_u64<E: serde::de::Error>(self, v: u64) -> Result<Self::Value, E> {
                Ok(StrictValue(Value::from(v)))
            }
            fn visit_f64<E: serde::de::Error>(self, v: f64) -> Result<Self::Value, E> {
                serde_json::Number::from_f64(v)
                    .map(|n| StrictValue(Value::Number(n)))
                    .ok_or_else(|| E::custom("non-finite JSON number"))
            }
            fn visit_str<E: serde::de::Error>(self, v: &str) -> Result<Self::Value, E> {
                Ok(StrictValue(Value::String(v.into())))
            }
            fn visit_string<E: serde::de::Error>(self, v: String) -> Result<Self::Value, E> {
                Ok(StrictValue(Value::String(v)))
            }
            fn visit_none<E: serde::de::Error>(self) -> Result<Self::Value, E> {
                Ok(StrictValue(Value::Null))
            }
            fn visit_unit<E: serde::de::Error>(self) -> Result<Self::Value, E> {
                Ok(StrictValue(Value::Null))
            }
            fn visit_seq<A: SeqAccess<'de>>(self, mut a: A) -> Result<Self::Value, A::Error> {
                let mut v = vec![];
                while let Some(x) = a.next_element::<StrictValue>()? {
                    v.push(x.0)
                }
                Ok(StrictValue(Value::Array(v)))
            }
            fn visit_map<A: MapAccess<'de>>(self, mut a: A) -> Result<Self::Value, A::Error> {
                let mut m = serde_json::Map::new();
                let mut seen = BTreeSet::new();
                while let Some((k, v)) = a.next_entry::<String, StrictValue>()? {
                    if !seen.insert(k.clone()) {
                        return Err(serde::de::Error::custom(format!(
                            "duplicate decoded JSON key {k}"
                        )));
                    }
                    m.insert(k, v.0);
                }
                Ok(StrictValue(Value::Object(m)))
            }
        }
        d.deserialize_any(V)
    }
}
fn check_duplicates(bytes: &[u8]) -> EngineResult<()> {
    let _: StrictValue = serde_json::from_slice(bytes).map_err(|e| bad("json", e.to_string()))?;
    Ok(())
}
