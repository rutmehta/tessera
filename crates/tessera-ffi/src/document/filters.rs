//! Filters, Image ▸ Adjustments and smart filters on a [`DocumentSession`]
//! (WP B5-05).
//!
//! # Calls
//!
//! - [`list_filters`]: the Filter menu, generated from `filters::registry`
//!   (id, group, name and a JSON parameter schema per filter).
//! - [`DocumentSession::preview_filter`]: renders the filter on the
//!   **viewport level** (and the visible region) on a worker thread and shows
//!   it in the presented surface. No history node; a newer preview cancels
//!   the running one (latest wins). [`DocumentSession::clear_preview`] ends it.
//! - [`DocumentSession::apply_filter`]: one history node. Destructive on
//!   pixel layers (inside the selection, if any); on smart objects the filter
//!   is appended to the layer's smart filter list (with the selection as its
//!   mask), and the pixels stay as they are.
//! - [`DocumentSession::set_smart_filter`] / `remove_smart_filter`: edit the
//!   list (enable, parameters, blend mode and opacity), one node each.
//! - [`DocumentSession::apply_adjustment`]: Image ▸ Adjustments on a pixel
//!   layer, with the same maths as the adjustment layer of that JSON.
//!
//! Filter JSON is `{"id":"gaussian_blur","params":{"radius":4}}`: `params`
//! uses the keys of the filter's schema, in its units; missing keys take the
//! schema defaults.
//!
//! # Presentation
//!
//! The compositor stores smart filters but does not render them, and a
//! preview is not part of the document. The session therefore renders a
//! *presented* document: the live one with (a) each smart object that has
//! enabled smart filters replaced by a baked pixel layer, (b) the previewed
//! layer replaced by a proxy pixel layer (the filtered viewport level, each
//! level pixel repeated `2^level` times, so the pyramid shows it exactly at
//! that level; outside the previewed region the layer's own tiles), and (c) for an
//! adjustment preview, the adjustment clipped directly above the layer. Bakes
//! run on the same worker at the viewport level and are refined at level 0
//! when the view is at 100 % or closer; export bakes at full resolution.

use super::{
    DocRect, DocumentSession, DocumentUpdate, Shared, blend_name, find, parse_blend,
    raster_from_rgba, tile_from_f32,
};
use crate::{Result, failure, surface::Surface};
use compositor::{
    Adjustment, Affine, BlendMode, Compositor, DocOp, DocState, Document, Layer, LayerId,
    LayerKind, PaintTarget, Raster, Rect, SmartFilter, SmartObject, TileDelta, blend::blend_pixel,
};
use engine_api::{
    jobs::CancellationToken,
    tile::{Extent, TILE_SIZE, TileCoord},
};
use filters::{
    Effect, Filter, FilterParams, Halo,
    registry::{self, ParamValue},
};
use std::{
    collections::{BTreeMap, HashMap},
    sync::{
        Arc, Condvar, Mutex, MutexGuard, Weak,
        atomic::{AtomicBool, Ordering},
    },
};

/// One request's live cancellation sources. Effects retain their existing atomic
/// API; native compositor work receives the same request's clonable token.
#[derive(Default)]
pub(super) struct RequestCancellation {
    effect: AtomicBool,
    native: CancellationToken,
}

impl RequestCancellation {
    pub(super) fn cancel(&self) {
        self.native.cancel();
        self.effect.store(true, Ordering::Release);
    }

    pub(super) fn is_cancelled(&self) -> bool {
        self.native.is_cancelled() || self.effect.load(Ordering::Acquire)
    }

    pub(super) fn native_token(&self) -> &CancellationToken {
        &self.native
    }
}

// ─────────────────────────────── records ───────────────────────────────

/// Full-resolution operations supplied by the compositor filter adapter.
#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum RasterFilterOperation {
    Remove,
    ContentAwareFill,
    ContentAwareMove,
    ContentAwareExtend,
    Liquify,
    CameraRaw,
    SkinSmoothing,
    Colorize,
    JpegArtifactRemoval,
    // M5-32: DRUNet denoise only; no face or scratch model.
    PhotoRestoration,
}

/// Retouch/neural request. Applied destructively to pixels or appended to an
/// existing smart object; use `convert_for_smart_filters` to opt into smart edits.
#[derive(Clone, Debug, uniffi::Record)]
pub struct RasterFilterRequest {
    pub operation: RasterFilterOperation,
    /// Strict adapter parameters in full-resolution canvas coordinates:
    /// Remove {mask,remove}; fill {mask,fill}; move/extend {mask,offset,fill,seam};
    /// liquify {mesh,interpolation}; skin {faces:[[x,y,w,h]],blur,smoothness};
    /// colorize {saturation}; JPEG {strength}; camera_raw uses its recipe schema.
    /// Masks are row-major floats, one per canvas pixel. No weights are downloaded.
    pub params_json: String,
}

/// Removal update plus detected coverage and detector limitations.
#[derive(Clone, Debug, uniffi::Record)]
pub struct DistractionRemovalResult {
    pub update: DocumentUpdate,
    pub report_json: String,
}

/// One Filter menu entry (`filters::registry`).
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct FilterInfo {
    /// Stable id used in filter JSON (`gaussian_blur`).
    pub id: String,
    /// `Blur`, `Sharpen`, `Noise`, `Distort`, `Stylize`, `Render` or `Other`.
    pub group: String,
    /// Menu title (`Gaussian Blur`).
    pub name: String,
    /// `{"params":[…]}`: controls with kind (`slider`, `angle`, `choice`,
    /// `point`, `toggle`), label, range, default, step, unit (see
    /// `filters::registry::FilterInfo::schema_json`).
    pub params_schema_json: String,
}

/// Every filter of the Filter menu, in menu order.
#[uniffi::export]
pub fn list_filters() -> Vec<FilterInfo> {
    registry::list()
        .into_iter()
        .map(|f| FilterInfo {
            id: f.id.into(),
            group: f.group.into(),
            name: f.name.into(),
            params_schema_json: f.schema_json(),
        })
        .collect()
}

/// One smart filter of a smart object, bottom (first applied) first.
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct SmartFilterRecord {
    pub index: u32,
    pub filter_id: String,
    /// Menu title of the filter.
    pub name: String,
    pub enabled: bool,
    /// `{"id":…,"params":{…}}` (the JSON `apply_filter` took).
    pub filter_json: String,
    /// Blending options: opacity 0…1 and a blend mode name.
    pub opacity: f32,
    pub blend_mode: String,
    /// A filter mask (from the selection when the filter was applied).
    pub has_mask: bool,
}

/// What [`DocumentSession::set_smart_filter`] changes.
#[derive(Clone, Debug, PartialEq, uniffi::Enum)]
pub enum SmartFilterEdit {
    Enabled {
        enabled: bool,
    },
    /// New filter JSON (the same filter id).
    Params {
        filter_json: String,
    },
    /// Blending options (mode name as `LayerNode::blend_mode`, opacity 0…1).
    Blending {
        mode: String,
        opacity: f32,
    },
}

/// A 1:1 filter detail crop written into an RGBA8 IOSurface.
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct FilterDetail {
    pub surface_id: u32,
    pub width: u32,
    pub height: u32,
    /// Pyramid level the crop was filtered at: 0 (true 1:1) except for
    /// whole-image filters on large layers, which are filtered on a coarser
    /// level and enlarged.
    pub level: u8,
}

// ─────────────────────────────── filter specs ───────────────────────────────

/// A parsed, validated filter JSON.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Spec {
    id: String,
    values: BTreeMap<String, ParamValue>,
    /// Canonical `{"id","params"}` JSON.
    json: String,
}

fn adapter_id(id: &str) -> bool {
    matches!(
        id,
        "remove"
            | "content_aware_fill"
            | "content_aware_move"
            | "content_aware_extend"
            | "liquify"
            | "camera_raw"
            | "neural/skin_smoothing"
            | "neural/colorize"
            | "neural/jpeg_artifact_removal"
            // M5-32: restoration follows the full-resolution neural adapter path.
            | "neural/photo_restoration"
    )
}

// B5-18b begin: the Camera Raw Filter is an adapter the renderer evaluates at
// any pyramid level and over any region (its input is the image it develops),
// so previews, the 1:1 detail pane, smart-filter checks and bakes render the
// view level over the visible region (plus the halo its local operators need)
// instead of the whole canvas at level 0. Apply stays full resolution.
const CAMERA_RAW: &str = "camera_raw";
const CAMERA_RAW_TITLE: &str = "Camera Raw Filter";

/// Adapters rendered at any level and over any region (see `camera_raw_halo`).
fn level_aware(id: &str) -> bool {
    id == CAMERA_RAW
}

/// Pixels the Camera Raw adapter developed (tests and benches: the work a
/// preview or detail render did).
static CAMERA_RAW_PIXELS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// Radius (pixels) the Texture / Clarity guided filters read around a pixel:
/// two box means of radius 8 plus the extrema clamp of radius 1, rounded up.
const PRESENCE_HALO: i64 = 24;

/// The halo (pixels of the level it runs on) a Camera Raw Filter needs around
/// a region, or `None` when its result depends on the whole image: lens
/// corrections (profile, CA analysis, manual distortion / vignetting,
/// defringe), Dehaze's global statistics, placed effects (vignette, grain,
/// lens blur), geometry, local masks, and any raw-only stage left non-default.
/// Such settings render over the whole canvas of the level, like any other
/// whole-image filter. Everything else is per-pixel (white balance, tone,
/// curves, colour, grading) or has a bounded support (Detail's sharpening and
/// noise reduction, Texture / Clarity).
fn camera_raw_halo(spec: &Spec) -> Result<Option<i64>> {
    let v: serde_json::Value = serde_json::from_str(&spec.json).map_err(failure)?;
    let p = filters::camera_raw::parse(&v["params"])?;
    let (s, d) = (&p.settings, engine_api::recipe::DevelopSettings::default());
    let lens = &s.lens;
    let lens_off = lens.profile == engine_api::recipe::settings::LensProfileSource::None
        && !lens.remove_chromatic_aberration
        && lens.manual_distortion == 0.0
        && lens.manual_vignetting == 0.0
        && lens.defringe_purple.amount == 0.0
        && lens.defringe_green.amount == 0.0
        && lens.softness_correction == 0.0;
    let local = lens_off
        && s.tone.dehaze == 0.0
        && s.effects.vignette.amount == 0.0
        && s.effects.grain.amount == 0.0
        && s.effects.lens_blur.is_none()
        && s.locals == d.locals
        && s.geometry == d.geometry
        && s.decode == d.decode
        && s.linearize == d.linearize
        && s.denoise == d.denoise
        && s.demosaic == d.demosaic
        && s.camera_profile == d.camera_profile;
    Ok(local.then(|| i64::from(pipeline_cpu::detail_halo(&s.detail)) + PRESENCE_HALO))
}

/// A Camera Raw Filter without its radius-dependent detail effects
/// (Sharpening, Noise Reduction, Texture, Clarity); other filters as they are.
/// Their radii are level-0 pixels, so a preview below 100 % (a smaller
/// pyramid level) would show them 2-4x too wide: like Camera Raw, the
/// zoomed-out preview omits them and the sheet says so. The 1:1 pane and
/// Apply always include them.
fn without_detail_effects(spec: &Spec) -> Result<Spec> {
    if spec.id != CAMERA_RAW {
        return Ok(spec.clone());
    }
    let v: serde_json::Value = serde_json::from_str(&spec.json).map_err(failure)?;
    let mut p = filters::camera_raw::parse(&v["params"])?;
    let s = &mut p.settings;
    s.detail.sharpening.amount = 0.0;
    s.detail.noise_reduction.luminance = 0.0;
    s.detail.noise_reduction.color = 0.0;
    s.tone.texture = 0.0;
    s.tone.clarity = 0.0;
    let params = serde_json::to_value(&p).map_err(failure)?;
    Spec::from_value(&serde_json::json!({"id": CAMERA_RAW, "params": params}))
}

impl StackEdit {
    /// The edit a preview at pyramid `level` evaluates: below level 0 the
    /// edited Camera Raw Filter omits its detail effects.
    fn at_preview_level(self, level: u8) -> Result<Self> {
        if level == 0 {
            return Ok(self);
        }
        Ok(match self {
            Self::Append(s) => Self::Append(without_detail_effects(&s)?),
            Self::Replace(i, s) => Self::Replace(i, without_detail_effects(&s)?),
        })
    }
}
// B5-18b end

/// Private evaluation errors remain typed until the native or legacy boundary.
#[derive(Debug)]
enum FilterEvalError {
    Cancelled,
    Failed(crate::BridgeError),
}
type FilterEvalResult<T> = std::result::Result<T, FilterEvalError>;
impl From<crate::BridgeError> for FilterEvalError {
    fn from(error: crate::BridgeError) -> Self {
        Self::Failed(error)
    }
}
impl From<engine_api::EngineError> for FilterEvalError {
    fn from(error: engine_api::EngineError) -> Self {
        match error {
            engine_api::EngineError::Cancelled => Self::Cancelled,
            other => Self::Failed(other.into()),
        }
    }
}
impl FilterEvalError {
    fn into_bridge(self) -> crate::BridgeError {
        match self {
            Self::Cancelled => failure("cancelled"),
            Self::Failed(error) => error,
        }
    }
    fn into_engine(self) -> engine_api::EngineError {
        match self {
            Self::Cancelled => engine_api::EngineError::Cancelled,
            Self::Failed(error) => {
                engine_api::EngineError::invalid("smart filter", error.to_string())
            }
        }
    }
}
fn effect_checkpoint(cancel: &AtomicBool) -> FilterEvalResult<()> {
    if cancel.load(Ordering::Relaxed) {
        Err(FilterEvalError::Cancelled)
    } else {
        Ok(())
    }
}

/// Drain every already-running result, even after cancellation. A real failure
/// from any worker wins cancellation; first received real failure is retained.
fn collect_effect_results<T>(
    results: impl IntoIterator<Item = FilterEvalResult<T>>,
    mut accept: impl FnMut(T),
) -> FilterEvalResult<()> {
    let mut failure = None;
    let mut cancelled = false;
    for result in results {
        match result {
            Err(FilterEvalError::Failed(error)) => {
                if failure.is_none() {
                    failure = Some(error);
                }
            }
            Err(FilterEvalError::Cancelled) => cancelled = true,
            Ok(value) if failure.is_none() && !cancelled => accept(value),
            Ok(_) => {}
        }
    }
    if let Some(error) = failure {
        Err(FilterEvalError::Failed(error))
    } else if cancelled {
        Err(FilterEvalError::Cancelled)
    } else {
        Ok(())
    }
}

impl Spec {
    fn run(
        &self,
        img: &Img,
        context: &compositor::render::smart_filters::FilterContext,
        cancel: &AtomicBool,
    ) -> FilterEvalResult<Img> {
        effect_checkpoint(cancel)?;
        if !adapter_id(&self.id) {
            let (effect, params) = self.at(context.level as u8)?;
            return run_effect(effect, &params, img, cancel);
        }
        // B5-18b: level-aware adapters develop the image they are given.
        let aware = level_aware(&self.id);
        if !aware && (context.level != 0 || img.rect != Rect::of_extent(context.canvas)) {
            return Err(failure("retouch filters require the complete level-0 canvas").into());
        }
        effect_checkpoint(cancel)?;
        let v: serde_json::Value = serde_json::from_str(&self.json).map_err(failure)?;
        let extent = Extent::new(img.w() as u32, img.h() as u32);
        if aware {
            CAMERA_RAW_PIXELS.fetch_add(extent.area(), Ordering::Relaxed);
        }
        let input = raster_from_rgba(extent, compositor::Depth::F32, &img.px, false)?;
        use compositor::render::smart_filters::SmartFilterEvaluator;
        let output = filters::CompositorFilters.evaluate(
            &input,
            &SmartFilter {
                name: self.id.clone(),
                params: v["params"].clone(),
                ..Default::default()
            },
            context,
        )?;
        effect_checkpoint(cancel)?;
        Ok(Img {
            rect: img.rect,
            px: raster_rgba(&output)?,
        })
    }

    fn parse(json: &str) -> Result<Self> {
        let v: serde_json::Value =
            serde_json::from_str(json).map_err(|e| failure(format!("filter JSON: {e}")))?;
        Self::from_value(&v)
    }

    fn from_value(v: &serde_json::Value) -> Result<Self> {
        let id = v
            .get("id")
            .and_then(|i| i.as_str())
            .ok_or_else(|| failure("filter JSON needs an \"id\""))?
            .to_owned();
        let mut values = BTreeMap::new();
        let params = v.get("params").cloned().unwrap_or(serde_json::json!({}));
        let obj = params
            .as_object()
            .ok_or_else(|| failure("filter \"params\" must be an object"))?;
        if adapter_id(&id) {
            return Ok(Self {
                id: id.clone(),
                values,
                json: serde_json::json!({"id":id,"params":params}).to_string(),
            });
        }
        for (k, p) in obj {
            let value = match p {
                serde_json::Value::Number(n) => ParamValue::Number(n.as_f64().unwrap_or(f64::NAN)),
                serde_json::Value::Bool(b) => ParamValue::Bool(*b),
                serde_json::Value::String(s) => ParamValue::Text(s.clone()),
                serde_json::Value::Array(a) if a.len() == 2 => ParamValue::Point([
                    a[0].as_f64().unwrap_or(f64::NAN),
                    a[1].as_f64().unwrap_or(f64::NAN),
                ]),
                other => {
                    return Err(failure(format!(
                        "filter parameter {k}: unsupported {other}"
                    )));
                }
            };
            values.insert(k.clone(), value);
        }
        registry::build(&id, &values, 1.0)?;
        let json = serde_json::json!({ "id": id, "params": params }).to_string();
        Ok(Self { id, values, json })
    }

    fn name(&self) -> String {
        // B5-18b: history labels and smart filter rows name the Camera Raw Filter.
        if self.id == CAMERA_RAW {
            return CAMERA_RAW_TITLE.to_owned();
        }
        registry::find(&self.id).map_or_else(|| self.id.clone(), |f| f.name.to_owned())
    }

    /// Effect and parameters at pyramid `level`.
    fn at(&self, level: u8) -> Result<(Effect, FilterParams)> {
        Ok(registry::build(
            &self.id,
            &self.values,
            (1u32 << level) as f32,
        )?)
    }
}

/// One smart filter as evaluated (the compositor stores it as JSON).
#[derive(Clone, Debug, PartialEq)]
struct Node {
    spec: Spec,
    enabled: bool,
    opacity: f32,
    blend: BlendMode,
    /// Canvas-sized 8-bit grey PNG, base64.
    mask_png: Option<String>,
    // B5-12 begin: a reserved compositor `transform` stage, kept verbatim
    // (never parsed by the Spec parser or re-baked into another schema).
    raw: Option<Arc<serde_json::Value>>,
    // B5-12 end
}

impl Node {
    fn new(spec: Spec) -> Self {
        Self {
            spec,
            enabled: true,
            opacity: 1.0,
            blend: BlendMode::Normal,
            mask_png: None,
            raw: None, // B5-12
        }
    }

    fn of(sf: &SmartFilter) -> Result<Self> {
        let p = &sf.params;
        // B5-12 begin: reserved transform stages pass through untouched.
        if reserved_transform(&sf.name) {
            return Ok(Self {
                spec: Spec {
                    id: TRANSFORM_STAGE.into(),
                    values: BTreeMap::new(),
                    json: serde_json::json!({ "id": TRANSFORM_STAGE, "params": stripped_transform(p) })
                        .to_string(),
                },
                enabled: sf.enabled,
                opacity: sf.blend.opacity,
                blend: sf.blend.mode,
                mask_png: None,
                raw: Some(Arc::new(p.clone())),
            });
        }
        // B5-12 end
        if adapter_id(&sf.name) && p.get("filter").is_none() {
            return Ok(Self {
                spec: Spec::from_value(&serde_json::json!({"id":sf.name,"params":p}))?,
                enabled: sf.enabled,
                opacity: sf.blend.opacity,
                blend: sf.blend.mode,
                mask_png: None,
                raw: None, // B5-12
            });
        }
        let spec = Spec::from_value(
            p.get("filter")
                .unwrap_or(&serde_json::json!({ "id": sf.name, "params": sf.params })),
        )?;
        Ok(Self {
            spec,
            enabled: sf.enabled,
            opacity: p
                .get("opacity")
                .and_then(|v| v.as_f64())
                .map_or(1.0, |v| v.clamp(0.0, 1.0) as f32),
            blend: p
                .get("blend")
                .and_then(|v| v.as_str())
                .map(parse_blend)
                .transpose()?
                .unwrap_or(BlendMode::Normal),
            mask_png: p
                .get("mask_png")
                .and_then(|v| v.as_str())
                .map(str::to_owned),
            raw: None, // B5-12
        })
    }

    fn store(&self) -> SmartFilter {
        // B5-12 begin
        if let Some(raw) = &self.raw {
            return SmartFilter {
                name: TRANSFORM_STAGE.into(),
                enabled: self.enabled,
                blend: compositor::render::smart_filters::FilterBlend {
                    mode: self.blend,
                    opacity: self.opacity.clamp(0.0, 1.0),
                },
                params: (**raw).clone(),
            };
        }
        // B5-12 end
        let filter: serde_json::Value =
            serde_json::from_str(&self.spec.json).unwrap_or(serde_json::Value::Null);
        if adapter_id(&self.spec.id) && self.mask_png.is_none() {
            return SmartFilter {
                name: self.spec.id.clone(),
                enabled: self.enabled,
                blend: compositor::render::smart_filters::FilterBlend {
                    mode: self.blend,
                    opacity: self.opacity,
                },
                params: filter["params"].clone(),
            };
        }
        let mut params = serde_json::json!({
            "filter": filter,
            "opacity": self.opacity,
            "blend": blend_name(self.blend),
        });
        if let Some(m) = &self.mask_png {
            params["mask_png"] = serde_json::Value::String(m.clone());
        }
        SmartFilter {
            blend: compositor::render::smart_filters::FilterBlend {
                mode: self.blend,
                opacity: self.opacity.clamp(0.0, 1.0),
            },
            name: self.spec.id.clone(),
            enabled: self.enabled,
            params,
        }
    }

    /// Everything that changes the result (the key of bakes).
    fn key(&self) -> String {
        // B5-12 begin: a transform's geometry by digest (protect masks can be large).
        if let Some(raw) = &self.raw {
            return format!(
                "transform|{}|{}|{:?}|{}",
                self.enabled,
                self.opacity,
                self.blend,
                transform_digest(&self.spec.json, raw)
            );
        }
        // B5-12 end
        format!(
            "{}|{}|{}|{:?}|{}",
            self.spec.json,
            self.enabled,
            self.opacity,
            self.blend,
            self.mask_png.as_ref().map_or(0, |m| {
                use std::hash::{Hash, Hasher};
                let mut h = std::collections::hash_map::DefaultHasher::new();
                m.hash(&mut h);
                h.finish()
            })
        )
    }
}

/// Whether the stack has an adapter that needs the whole level-0 canvas
/// (B5-18b: not the level-aware Camera Raw Filter).
fn full_resolution(nodes: &[Node]) -> bool {
    nodes
        .iter()
        .any(|n| n.enabled && adapter_id(&n.spec.id) && !level_aware(&n.spec.id))
}

fn nodes_of(so: &SmartObject) -> Result<Vec<Node>> {
    so.filters.iter().map(Node::of).collect()
}

// B5-12 begin: reserved compositor `transform` stages (document/transform.rs).
//
// Transform stages are the compositor's own nodes (`SmartFilter::transform`):
// they are never fed through the Spec parser or baker, never rewritten by an
// edit of another node, and never dropped when unrelated filters change.
// Stacks made only of geometric stages (Free / Warp / Perspective / Puppet /
// Displacement) are not baked here at all: the session renderer evaluates
// them itself (the resident GPU route of M5-23, or the CPU compositor).
// Stacks mixing them with menu filters, and content-aware scale (a CPU-only,
// potentially slow stage), are baked on this worker through the compositor
// with the menu filters bridged by `NativeFilterEvaluator`.

pub(super) const TRANSFORM_STAGE: &str = "transform";

fn reserved_transform(id: &str) -> bool {
    id == TRANSFORM_STAGE
}

/// `op` without a content-aware protect mask (records and keys stay small).
pub(super) fn stripped_transform(op: &serde_json::Value) -> serde_json::Value {
    let Some(cas) = op
        .get("operation")
        .and_then(|o| o.get("ContentAwareScale"))
        .and_then(|c| c.as_object())
    else {
        return op.clone();
    };
    let mut c = serde_json::Map::new();
    for (k, v) in cas {
        c.insert(
            k.clone(),
            if k == "protect" {
                serde_json::Value::Null
            } else {
                v.clone()
            },
        );
    }
    let mut out = serde_json::Map::new();
    if let Some(o) = op.as_object() {
        for (k, v) in o {
            if k != "operation" {
                out.insert(k.clone(), v.clone());
            }
        }
    }
    out.insert(
        "operation".into(),
        serde_json::json!({ "ContentAwareScale": serde_json::Value::Object(c) }),
    );
    serde_json::Value::Object(out)
}

/// A content-aware protect mask of `op`, if any.
pub(super) fn transform_protect(op: &serde_json::Value) -> Option<&Vec<serde_json::Value>> {
    op.get("operation")?
        .get("ContentAwareScale")?
        .get("protect")?
        .as_array()
}

/// Geometry digest: the stripped JSON plus the protect mask's length and a
/// strided sample of it (cheap enough to recompute for every frame).
fn transform_digest(stripped: &str, raw: &serde_json::Value) -> String {
    let mut h = blake3::Hasher::new();
    h.update(stripped.as_bytes());
    if let Some(p) = transform_protect(raw) {
        h.update(&(p.len() as u64).to_le_bytes());
        let step = (p.len() / 4096).max(1);
        for v in p.iter().step_by(step) {
            h.update(&v.as_f64().unwrap_or(f64::NAN).to_le_bytes());
        }
    }
    h.finalize().to_hex()[..32].to_owned()
}

/// Menu title of a transform stage (`Warp`, `Perspective Warp`, …).
pub(super) fn transform_title(op: &serde_json::Value) -> &'static str {
    let kind = op
        .get("operation")
        .and_then(|o| o.as_object())
        .and_then(|o| o.keys().next().map(String::as_str));
    match kind {
        Some("Warp") => "Warp",
        Some("Perspective") => "Perspective Warp",
        Some("Puppet") => "Puppet Warp",
        Some("ContentAwareScale") => "Content-Aware Scale",
        Some("Free") => "Transform",
        Some("Displacement") => "Displacement",
        _ => "Transform",
    }
}

/// Whether the session renderer evaluates `l`'s stack itself (no bake): a
/// smart object whose enabled stages are all geometric transform stages.
pub(super) fn compositor_stack(l: &Layer) -> bool {
    let LayerKind::SmartObject(so) = &l.kind else {
        return false;
    };
    let mut enabled = so.filters.iter().filter(|f| f.enabled).peekable();
    enabled.peek().is_some()
        && enabled.all(|f| {
            reserved_transform(&f.name)
                && f.params
                    .get("operation")
                    .and_then(|o| o.get("ContentAwareScale"))
                    .is_none()
        })
}

/// The layer rendered alone at `level` through the compositor, its stack
/// `nodes` evaluated natively (transform stages by the compositor, menu
/// filters through `NativeFilterEvaluator`), placed in the parent canvas.
fn native_stack(
    base: &DocState,
    layer: &Layer,
    nodes: &[Node],
    level: u8,
    cancel: Option<&Arc<RequestCancellation>>,
) -> Result<Img> {
    let cancel = cancel.cloned().unwrap_or_default();
    cancel.native.check()?;
    Ok(native_stack_with_evaluator(
        base,
        layer,
        nodes,
        level,
        &cancel,
        Arc::new(NativeFilterEvaluator {
            cancel: cancel.clone(),
        }),
    )?)
}

fn native_stack_with_evaluator(
    base: &DocState,
    layer: &Layer,
    nodes: &[Node],
    level: u8,
    cancel: &Arc<RequestCancellation>,
    evaluator: Arc<dyn compositor::render::smart_filters::SmartFilterEvaluator>,
) -> engine_api::EngineResult<Img> {
    cancel.native.check()?;
    let neutral = solo(base, layer);
    let mut l = (*neutral.state().root[0]).clone();
    l.kind = layer.kind.clone();
    let LayerKind::SmartObject(so) = &mut l.kind else {
        return Err(engine_api::EngineError::invalid(
            "smart filter",
            "transform stages require a smart object",
        ));
    };
    so.filters = nodes.iter().map(Node::store).collect();
    let mut state = DocState::new(base.canvas, compositor::Depth::F32);
    state.profile = base.profile.clone();
    state.next_id = base.next_id;
    state.root.push(Arc::new(l));
    let mut comp = super::fonts::compositor(64 << 20); // B5-10b
    comp.set_filter_evaluator(evaluator);
    let doc = Document::new(state);
    // RES-01 serializes first touch and retains results for the full-level pass.
    // A direct-tile prewarm would bypass both that pass and caller cancellation.
    let (e, px) = comp.render_level_rgba_with_cancel(&doc, level, &cancel.native)?;
    Ok(Img {
        rect: Rect::of_extent(e),
        px,
    })
}

/// A smart object's whole stack rasterized at level 0 into a document-sized
/// raster at `depth` (the explicit rasterized-PSD path).
pub(super) fn rasterize_smart_stack_with_cancel(
    base: &DocState,
    layer: &Layer,
    cancel: &Arc<RequestCancellation>,
) -> super::psd_copy::CopyResult<Raster> {
    let check = || {
        cancel
            .native
            .check()
            .map_err(super::psd_copy::CopyError::from)
    };
    check()?;
    let LayerKind::SmartObject(so) = &layer.kind else {
        return Err(failure("not a smart object").into());
    };
    let img = native_stack_with_evaluator(
        base,
        layer,
        &nodes_of(so)?,
        0,
        cancel,
        Arc::new(NativeFilterEvaluator {
            cancel: cancel.clone(),
        }),
    )?;
    super::raster_from_rgba_checked(base.canvas, base.depth, &img.px, true, check)
}

/// What a whole-stack edit may not change under a position lock: every
/// transform stage with its stack index, enabled state, blending and geometry.
fn transform_signature(
    nodes: &[Node],
) -> Vec<(usize, bool, u32, BlendMode, Arc<serde_json::Value>)> {
    nodes
        .iter()
        .enumerate()
        .filter_map(|(i, n)| {
            n.raw
                .clone()
                .map(|r| (i, n.enabled, n.opacity.to_bits(), n.blend, r))
        })
        .collect()
}

/// `l` wrapped in a smart object holding it at identity (same id, name,
/// properties, layer styles, raster and vector masks: each exactly once, on
/// the wrapper; the child keeps the pixels / live source with default
/// properties). Shared by Filter ▸ Convert for Smart Filters and the B5-12
/// transform tools' explicit conversion.
pub(super) fn smart_wrapper(s: &DocState, l: &Layer) -> Result<Layer> {
    if matches!(l.kind, LayerKind::SmartObject(_)) {
        return Err(failure("the layer is already a smart object"));
    }
    if matches!(l.kind, LayerKind::Adjustment(_)) {
        return Err(failure("adjustment layers cannot become smart objects"));
    }
    let mut child = DocState::new(s.canvas, s.depth);
    child.profile = s.profile.clone();
    let mut inner = l.clone();
    inner.props = compositor::LayerProps {
        name: l.props.name.clone(),
        ..Default::default()
    };
    inner.mask = None;
    inner.vector_mask = None;
    inner.id = LayerId(1);
    child.next_id = 2;
    if let LayerKind::Group { children, .. } = &mut inner.kind {
        // Children keep their ids inside the child document.
        let max = children.iter().map(|c| max_id(c)).max().unwrap_or(1);
        inner.id = LayerId(max + 1);
        child.next_id = max + 2;
    }
    child.root = vec![Arc::new(inner)];
    let mut outer = Layer::new(
        l.props.name.clone(),
        LayerKind::SmartObject(SmartObject::new(child, Affine::IDENTITY)),
    );
    outer.id = l.id;
    outer.props = l.props.clone();
    outer.props.background = false;
    outer.mask = l.mask.clone();
    outer.vector_mask = l.vector_mask.clone();
    Ok(outer)
}
// B5-12 end

// ─────────────────────────────── images ───────────────────────────────

/// Straight RGBA f32 pixels of `rect` (in the coordinates of one level).
#[derive(Clone, Debug)]
struct Img {
    rect: Rect,
    px: Vec<f32>,
}

/// Retained source/prefix images only; excludes active results and other caches.
const FILTER_IMAGE_CACHE_BYTES: usize = 512 * 1024 * 1024;
const FILTER_IMAGE_CACHE_ENTRIES: usize = 4;

struct CachedImg {
    key: String,
    image: Arc<Img>,
    bytes: usize,
}

struct ImgCache {
    entries: Vec<CachedImg>,
    retained_bytes: usize,
    budget: usize,
}

impl Default for ImgCache {
    fn default() -> Self {
        Self::new(FILTER_IMAGE_CACHE_BYTES)
    }
}

impl ImgCache {
    fn new(budget: usize) -> Self {
        Self {
            entries: Vec::new(),
            retained_bytes: 0,
            budget,
        }
    }

    fn capacity_bytes(capacity: usize) -> Option<usize> {
        capacity.checked_mul(std::mem::size_of::<f32>())
    }

    fn can_retain(&self, image: &Img) -> bool {
        self.budget != 0
            && Self::capacity_bytes(image.px.capacity()).is_some_and(|bytes| bytes <= self.budget)
    }

    fn remove(&mut self, key: &str) {
        if let Some(index) = self.entries.iter().position(|entry| entry.key == key) {
            self.retained_bytes -= self.entries.remove(index).bytes;
        }
    }

    fn insert(&mut self, key: String, image: Arc<Img>) {
        // Replacement is newest; an oversized replacement removes the old key.
        self.remove(&key);
        if !self.can_retain(&image) {
            return;
        }
        let bytes = Self::capacity_bytes(image.px.capacity()).expect("preflight checked capacity");
        while self.entries.len() >= FILTER_IMAGE_CACHE_ENTRIES
            || self
                .retained_bytes
                .checked_add(bytes)
                .is_none_or(|total| total > self.budget)
        {
            self.retained_bytes -= self.entries.remove(0).bytes;
        }
        self.retained_bytes += bytes;
        self.entries.push(CachedImg { key, image, bytes });
    }
}

impl Img {
    fn w(&self) -> usize {
        self.rect.width() as usize
    }
    fn h(&self) -> usize {
        self.rect.height() as usize
    }
    fn at(&self, x: usize, y: usize) -> &[f32] {
        let i = (y * self.w() + x) * 4;
        &self.px[i..i + 4]
    }
    /// The part of the image inside `r` (clamped to it).
    fn crop(&self, r: Rect) -> Img {
        let r = r.intersect(&self.rect);
        let (w, ox, oy) = (
            r.width() as usize,
            (r.x0 - self.rect.x0) as usize,
            (r.y0 - self.rect.y0) as usize,
        );
        let mut px = Vec::with_capacity(w * r.height() as usize * 4);
        for y in 0..r.height() as usize {
            let i = ((oy + y) * self.w() + ox) * 4;
            px.extend_from_slice(&self.px[i..i + w * 4]);
        }
        Img { rect: r, px }
    }
}

/// Normalized samples of `r` (level 0) of a raster, interleaved per pixel
/// (`channels` per pixel).
fn read_raster(raster: &Raster, r: Rect) -> Result<Vec<f32>> {
    let ch = raster.channels() as usize;
    let (w, h) = (r.width() as usize, r.height() as usize);
    let mut out = vec![raster.default_value(); w * h * ch];
    let ts = i64::from(TILE_SIZE);
    let full = Rect::of_extent(raster.extent());
    let r = r.intersect(&full);
    if r.is_empty() {
        return Ok(out);
    }
    let mut buf = Vec::new();
    for ty in r.y0 / ts..=(r.y1 - 1) / ts {
        for tx in r.x0 / ts..=(r.x1 - 1) / ts {
            let (tx32, ty32) = (tx as u32, ty as u32);
            raster.read_tile(tx32, ty32, &mut buf)?;
            let l = raster.layout(tx32, ty32);
            let (n, stride) = (l.plane_len(), l.stride());
            let tr = Rect::new(
                tx * ts,
                ty * ts,
                tx * ts + i64::from(l.extent.width),
                ty * ts + i64::from(l.extent.height),
            )
            .intersect(&r);
            for y in tr.y0..tr.y1 {
                for x in tr.x0..tr.x1 {
                    let src = (y - ty * ts) as usize * stride + (x - tx * ts) as usize;
                    let dst = ((y - r.y0) as usize * w + (x - r.x0) as usize) * ch;
                    for c in 0..ch {
                        out[dst + c] = buf[c * n + src];
                    }
                }
            }
        }
    }
    Ok(out)
}

/// Every tile of a raster as interleaved RGBA (raster channels ≥ 4).
fn raster_rgba(raster: &Raster) -> Result<Vec<f32>> {
    read_raster(raster, Rect::of_extent(raster.extent()))
}

/// A one-layer document with `layer`'s own content (Normal, opaque,
/// unmasked, unclipped), like the layer thumbnails.
/// `layer` with its smart filters removed. This session evaluates smart
/// filters itself (bakes); the compositor must only see the unfiltered child.
pub(crate) fn unfiltered(layer: &Layer) -> Layer {
    let mut l = layer.clone();
    if let LayerKind::SmartObject(so) = &mut l.kind {
        so.filters.clear();
        so.filter_mask = None;
    }
    l
}

fn solo(state: &DocState, layer: &Layer) -> Document {
    let mut l = unfiltered(layer);
    l.props.visible = true;
    l.props.opacity = 1.0;
    l.props.fill_opacity = 1.0;
    l.props.blend_mode = BlendMode::Normal;
    l.props.clipped = false;
    l.props.knockout = compositor::Knockout::None;
    l.props.background = false;
    l.props.blend_if = Default::default();
    l.mask = None;
    let mut s = DocState::new(state.canvas, state.depth);
    s.next_id = state.next_id;
    s.profile = state.profile.clone();
    s.root = vec![Arc::new(l)];
    Document::new(s)
}

fn threads() -> usize {
    std::thread::available_parallelism()
        .map_or(4, |n| n.get())
        .min(16)
}

/// The composite of `doc` inside `r` (coordinates of `level`), rendered
/// tile by tile in parallel through the CPU compositor.
fn render_region(comp: &Compositor, doc: &Document, level: u8, r: Rect) -> Result<Img> {
    let le = doc.state().canvas.at_level(level);
    let r = r.intersect(&Rect::of_extent(le));
    let (w, h) = (r.width().max(0) as usize, r.height().max(0) as usize);
    let mut px = vec![0.0f32; w * h * 4];
    if r.is_empty() {
        return Ok(Img { rect: r, px });
    }
    let ts = i64::from(TILE_SIZE);
    let coords: Vec<TileCoord> = (r.y0 / ts..=(r.y1 - 1) / ts)
        .flat_map(|ty| {
            (r.x0 / ts..=(r.x1 - 1) / ts).map(move |tx| TileCoord::new(level, tx as u32, ty as u32))
        })
        .collect();
    let next = std::sync::atomic::AtomicUsize::new(0);
    let tiles = Mutex::new(Vec::with_capacity(coords.len()));
    let err = Mutex::new(None);
    std::thread::scope(|s| {
        for _ in 0..threads().min(coords.len()) {
            s.spawn(|| {
                loop {
                    let i = next.fetch_add(1, Ordering::Relaxed);
                    let Some(c) = coords.get(i) else { break };
                    match comp.render_tile(doc, *c) {
                        Ok(t) => tiles.lock().unwrap_or_else(|e| e.into_inner()).push(t),
                        Err(e) => {
                            *err.lock().unwrap_or_else(|e| e.into_inner()) = Some(e);
                            break;
                        }
                    }
                }
            });
        }
    });
    if let Some(e) = err.into_inner().unwrap_or_else(|e| e.into_inner()) {
        return Err(e.into());
    }
    for t in tiles.into_inner().unwrap_or_else(|e| e.into_inner()) {
        let (ox, oy) = t.coord().pixel_origin(TILE_SIZE);
        let l = t.layout();
        let n = l.plane_len();
        let s = t.samples::<f32>()?;
        for y in 0..l.extent.height as i64 {
            let gy = i64::from(oy) + y;
            if gy < r.y0 || gy >= r.y1 {
                continue;
            }
            for x in 0..l.extent.width as i64 {
                let gx = i64::from(ox) + x;
                if gx < r.x0 || gx >= r.x1 {
                    continue;
                }
                let i = y as usize * l.stride() + x as usize;
                let o = ((gy - r.y0) as usize * w + (gx - r.x0) as usize) * 4;
                for c in 0..4 {
                    px[o + c] = s[c * n + i];
                }
            }
        }
    }
    Ok(Img { rect: r, px })
}

// ─────────────────────────────── running filters ───────────────────────────────

/// Resampling filters average colours: they run on premultiplied pixels so
/// transparent neighbours do not darken edges.
fn premultiplied(e: Effect) -> bool {
    matches!(
        e,
        Effect::Gaussian
            | Effect::Box
            | Effect::Motion
            | Effect::RadialSpin
            | Effect::RadialZoom
            | Effect::Distort(_)
    )
}

/// `effect` over `img`. Neighbourhood filters run in parallel blocks with
/// real-neighbour halos (clamped at the image edge, as the crate clamps at
/// the canvas); whole-image filters run once over the image.
fn run_effect(
    effect: Effect,
    p: &FilterParams,
    img: &Img,
    cancel: &AtomicBool,
) -> FilterEvalResult<Img> {
    effect_checkpoint(cancel)?;
    let (w, h) = (img.w(), img.h());
    if w == 0 || h == 0 {
        return Ok(img.clone());
    }
    let pre = premultiplied(effect);
    let prep = |px: &mut [f32]| {
        if pre {
            for q in px.as_chunks_mut::<4>().0 {
                for c in 0..3 {
                    q[c] *= q[3];
                }
            }
        }
    };
    let unprep = |px: &mut [f32]| {
        if pre {
            for q in px.as_chunks_mut::<4>().0 {
                if q[3] > 1e-6 {
                    for c in 0..3 {
                        q[c] /= q[3];
                    }
                } else {
                    q[..3].fill(0.0);
                }
            }
        }
    };
    let filter_block = |block: Rect| -> FilterEvalResult<Vec<f32>> {
        effect_checkpoint(cancel)?;
        let mut src = img.crop(block);
        prep(&mut src.px);
        let e = Extent::new(src.w() as u32, src.h() as u32);
        effect_checkpoint(cancel)?;
        let raster = raster_from_rgba(e, compositor::Depth::F32, &src.px, false)?;
        let out = effect.apply(&raster, p, cancel)?;
        effect_checkpoint(cancel)?;
        let mut px = raster_rgba(&out)?;
        unprep(&mut px);
        effect_checkpoint(cancel)?;
        Ok(px)
    };
    let halo = match effect.halo(p) {
        Halo::WholeImage => None,
        Halo::Radius(r) => Some(i64::from(r)),
    };
    let Some(halo) = halo else {
        let px = filter_block(img.rect)?;
        return Ok(Img { rect: img.rect, px });
    };
    let block = (halo * 2).clamp(256, 1024);
    let mut blocks = Vec::new();
    let mut y = img.rect.y0;
    while y < img.rect.y1 {
        let mut x = img.rect.x0;
        while x < img.rect.x1 {
            blocks.push(Rect::new(
                x,
                y,
                (x + block).min(img.rect.x1),
                (y + block).min(img.rect.y1),
            ));
            x += block;
        }
        y += block;
    }
    effect_checkpoint(cancel)?;
    let mut out = img.clone();
    let next = std::sync::atomic::AtomicUsize::new(0);
    let (tx, rx) = std::sync::mpsc::channel::<FilterEvalResult<(Rect, Rect, Vec<f32>)>>();
    std::thread::scope(|s| {
        for _ in 0..threads().min(blocks.len()) {
            let tx = tx.clone();
            let (next, blocks, filter_block) = (&next, &blocks, &filter_block);
            s.spawn(move || {
                loop {
                    let i = next.fetch_add(1, Ordering::Relaxed);
                    let Some(b) = blocks.get(i) else { break };
                    let ext = b.inflate(halo).intersect(&img.rect);
                    let r = filter_block(ext).map(|px| (*b, ext, px));
                    let failed = r.is_err();
                    if tx.send(r).is_err() || failed {
                        break;
                    }
                }
            });
        }
        drop(tx);
        let w = out.w();
        collect_effect_results(rx, |(b, ext, px)| {
            let ew = ext.width() as usize;
            for y in b.y0..b.y1 {
                let src = ((y - ext.y0) as usize * ew + (b.x0 - ext.x0) as usize) * 4;
                let dst = ((y - out.rect.y0) as usize * w + (b.x0 - out.rect.x0) as usize) * 4;
                let n = b.width() as usize * 4;
                out.px[dst..dst + n].copy_from_slice(&px[src..src + n]);
            }
        })
    })?;
    effect_checkpoint(cancel)?;
    Ok(out)
}

/// Halo (level pixels) the enabled nodes need around a region; `None` when
/// any is a whole-image operator.
fn stack_halo(nodes: &[Node], level: u8) -> Result<Option<i64>> {
    let mut sum = 0i64;
    for n in nodes.iter().filter(|n| n.enabled) {
        // B5-18b begin
        if level_aware(&n.spec.id) {
            match camera_raw_halo(&n.spec)? {
                Some(h) => sum += h,
                None => return Ok(None),
            }
            continue;
        }
        // B5-18b end
        if adapter_id(&n.spec.id) {
            return Ok(None);
        }
        let (e, p) = n.spec.at(level)?;
        match e.halo(&p) {
            Halo::WholeImage => return Ok(None),
            Halo::Radius(r) => sum += i64::from(r),
        }
    }
    Ok(Some(sum))
}

/// The mask of a node over `r` at `level` (box-averaged), 0…1.
fn mask_region(png_b64: &str, canvas: Extent, level: u8, r: Rect) -> Result<Vec<f32>> {
    let bytes = base64_decode(png_b64).ok_or_else(|| failure("smart filter mask: bad base64"))?;
    let dec = png::Decoder::new(std::io::Cursor::new(bytes));
    let mut reader = dec.read_info().map_err(failure)?;
    let mut buf = vec![0u8; reader.output_buffer_size()];
    let info = reader.next_frame(&mut buf).map_err(failure)?;
    if info.width != canvas.width
        || info.height != canvas.height
        || info.color_type != png::ColorType::Grayscale
        || info.bit_depth != png::BitDepth::Eight
    {
        return Err(failure("smart filter mask does not match the canvas"));
    }
    let (w, h) = (r.width() as usize, r.height() as usize);
    let d = 1i64 << level;
    let mut out = vec![1.0f32; w * h];
    for y in 0..h {
        for x in 0..w {
            let (x0, y0) = ((r.x0 + x as i64) * d, (r.y0 + y as i64) * d);
            let (mut sum, mut n) = (0u32, 0u32);
            for yy in y0..(y0 + d).min(i64::from(canvas.height)) {
                for xx in x0..(x0 + d).min(i64::from(canvas.width)) {
                    sum += u32::from(buf[yy as usize * info.line_size + xx as usize]);
                    n += 1;
                }
            }
            if n > 0 {
                out[y * w + x] = sum as f32 / (n as f32 * 255.0);
            }
        }
    }
    Ok(out)
}

/// The enabled nodes over `src`, each blended onto its input with its
/// opacity, mode and mask.
fn eval_stack(
    src: Img,
    nodes: &[Node],
    level: u8,
    canvas: Extent,
    profile: Option<compositor::document::ColorProfile>,
    cancel: &AtomicBool,
) -> FilterEvalResult<Img> {
    effect_checkpoint(cancel)?;
    let mut cur = src;
    for n in nodes.iter().filter(|n| n.enabled) {
        let f = n.spec.run(
            &cur,
            &compositor::render::smart_filters::FilterContext {
                profile: profile.clone(),
                level: u32::from(level),
                canvas,
            },
            cancel,
        )?;
        effect_checkpoint(cancel)?;
        let mask = n
            .mask_png
            .as_deref()
            .map(|m| mask_region(m, canvas, level, cur.rect))
            .transpose()?;
        if n.opacity >= 1.0 && n.blend == BlendMode::Normal && mask.is_none() {
            cur = f;
            continue;
        }
        for (i, (b, s)) in cur
            .px
            .as_chunks_mut::<4>()
            .0
            .iter_mut()
            .zip(f.px.as_chunks::<4>().0)
            .enumerate()
        {
            let k = n.opacity * mask.as_ref().map_or(1.0, |m| m[i]);
            let c = blend_pixel(n.blend, [b[0], b[1], b[2]], [s[0], s[1], s[2]]);
            for ch in 0..3 {
                b[ch] += k * (c[ch] - b[ch]);
            }
            b[3] += k * (s[3] - b[3]);
        }
    }
    effect_checkpoint(cancel)?;
    Ok(cur)
}

/// A canvas-sized raster showing `img` (pixels of `level`), each level pixel
/// repeated `2^level` times; tiles outside `img` come from `under`. Tiles
/// are built in parallel.
fn upsampled(
    img: &Img,
    level: u8,
    canvas: Extent,
    depth: compositor::Depth,
    under: Option<&Raster>,
) -> Result<Raster> {
    let mut r = under
        .cloned()
        .unwrap_or_else(|| Raster::new(canvas, 4, depth, 0.0));
    let rev = r.max_rev() + 1;
    let area = img
        .rect
        .to_level0(level)
        .intersect(&Rect::of_extent(canvas));
    if area.is_empty() {
        return Ok(r);
    }
    let ts = i64::from(TILE_SIZE);
    let coords: Vec<(i64, i64)> = (area.y0 / ts..=(area.y1 - 1) / ts)
        .flat_map(|ty| (area.x0 / ts..=(area.x1 - 1) / ts).map(move |tx| (tx, ty)))
        .collect();
    let base = &r;
    let next = std::sync::atomic::AtomicUsize::new(0);
    let (tx_, rx) =
        std::sync::mpsc::channel::<Result<(u32, u32, Option<engine_api::tile::Tile>)>>();
    let results: Vec<_> = std::thread::scope(|s| {
        for _ in 0..threads().min(coords.len()) {
            let tx_ = tx_.clone();
            let (next, coords) = (&next, &coords);
            s.spawn(move || {
                let mut buf = Vec::new();
                loop {
                    let i = next.fetch_add(1, Ordering::Relaxed);
                    let Some(&(tx, ty)) = coords.get(i) else {
                        break;
                    };
                    let r = (|| -> Result<(u32, u32, Option<engine_api::tile::Tile>)> {
                        let (tx32, ty32) = (tx as u32, ty as u32);
                        let l = base.layout(tx32, ty32);
                        base.read_tile(tx32, ty32, &mut buf)?;
                        let n = l.plane_len();
                        let mut any = false;
                        for y in 0..l.extent.height as i64 {
                            let gy = ty * ts + y;
                            let inside_y = gy >= area.y0 && gy < area.y1;
                            let row = ((gy >> level) - img.rect.y0) as usize;
                            for x in 0..l.extent.width as i64 {
                                let gx = tx * ts + x;
                                let i = y as usize * l.stride() + x as usize;
                                if inside_y && gx >= area.x0 && gx < area.x1 {
                                    let p = img.at(((gx >> level) - img.rect.x0) as usize, row);
                                    for c in 0..4 {
                                        buf[c * n + i] = p[c];
                                    }
                                }
                                any |= buf[3 * n + i] != 0.0;
                            }
                        }
                        let tile = if any {
                            Some(tile_from_f32(
                                TileCoord::new(0, tx32, ty32),
                                l,
                                depth,
                                buf.clone(),
                            )?)
                        } else {
                            None
                        };
                        Ok((tx32, ty32, tile))
                    })();
                    if tx_.send(r).is_err() {
                        break;
                    }
                }
            });
        }
        drop(tx_);
        rx.iter().collect()
    });
    for res in results {
        let (tx, ty, tile) = res?;
        r.set_slot(tx, ty, tile, rev)?;
    }
    Ok(r)
}

// ─────────────────────────────── base64 ───────────────────────────────

const B64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

fn base64_encode(data: &[u8]) -> String {
    let mut o = String::with_capacity(data.len().div_ceil(3) * 4);
    for c in data.chunks(3) {
        let n = (u32::from(c[0]) << 16)
            | (u32::from(*c.get(1).unwrap_or(&0)) << 8)
            | u32::from(*c.get(2).unwrap_or(&0));
        for i in 0..4 {
            if i <= c.len() {
                o.push(B64[(n >> (18 - 6 * i) & 63) as usize] as char);
            } else {
                o.push('=');
            }
        }
    }
    o
}

fn base64_decode(s: &str) -> Option<Vec<u8>> {
    let mut o = Vec::with_capacity(s.len() / 4 * 3);
    let (mut acc, mut bits) = (0u32, 0u32);
    for b in s.bytes() {
        if b == b'=' {
            break;
        }
        let v = B64.iter().position(|c| *c == b)? as u32;
        acc = acc << 6 | v;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            o.push((acc >> bits & 0xff) as u8);
        }
    }
    Some(o)
}

/// The selection as a canvas-sized 8-bit grey PNG, base64.
fn mask_from_selection(sel: &Raster) -> Result<String> {
    let e = sel.extent();
    let v = read_raster(sel, Rect::of_extent(e))?;
    let bytes: Vec<u8> = v
        .iter()
        .map(|v| (v.clamp(0.0, 1.0) * 255.0 + 0.5) as u8)
        .collect();
    let mut png_bytes = Vec::new();
    {
        let mut enc = png::Encoder::new(&mut png_bytes, e.width, e.height);
        enc.set_color(png::ColorType::Grayscale);
        enc.set_depth(png::BitDepth::Eight);
        enc.set_compression(png::Compression::Fast);
        let mut w = enc.write_header().map_err(failure)?;
        w.write_image_data(&bytes).map_err(failure)?;
    }
    Ok(base64_encode(&png_bytes))
}

// ─────────────────────────────── worker state ───────────────────────────────

/// What a preview changes in the stack of the previewed layer.
#[derive(Clone, Debug)]
enum StackEdit {
    /// A new filter (on top of a smart object's list, or alone on pixels).
    Append(Spec),
    /// Re-editing smart filter `index`.
    Replace(usize, Spec),
}

struct PreviewJob {
    generation: u64,
    base: Arc<DocState>,
    layer: u64,
    edit: StackEdit,
    level: u8,
    /// Region of `level` to show.
    region: Rect,
}

#[derive(Clone)]
enum PreviewShown {
    /// The layer replaced by this proxy.
    Replace(u64, Arc<Raster>),
    /// An adjustment clipped directly above the layer.
    ClippedAdjustment(u64, Adjustment),
    /// B5-15 (P19): the smart object with the edited stack in engine form,
    /// evaluated by the resident renderer on the GPU.
    Stack(u64, Arc<Layer>),
}

struct BakeJob {
    key: String,
    base: Arc<DocState>,
    layer: u64,
    level: u8,
    /// Level-0 region to refine (level 0 only).
    region: Option<Rect>,
}

struct ActiveBake {
    layer: u64,
    key: String,
    level: u8,
    region: Option<Rect>,
    cancel: Arc<RequestCancellation>,
}

impl ActiveBake {
    fn matches(&self, job: &BakeJob) -> bool {
        self.layer == job.layer
            && self.key == job.key
            && self.level == job.level
            && self.region == job.region
            && !self.cancel.is_cancelled()
    }
}

/// A smart object's filtered pixels.
struct Baked {
    key: String,
    raster: Raster,
    /// Finest level computed over the whole canvas.
    level: u8,
    /// Level-0 regions computed at full resolution since.
    fine: Vec<Rect>,
}

#[derive(Default)]
struct Inner {
    stop: bool,
    busy: bool,
    generation: u64,
    preview_job: Option<PreviewJob>,
    running: Option<Arc<RequestCancellation>>,
    running_bake: Option<ActiveBake>,
    preview: Option<(u64, PreviewShown)>,
    bake_jobs: BTreeMap<u64, BakeJob>,
    bakes: HashMap<u64, Baked>,
    /// Bake generation (part of the presented key).
    bake_serial: u64,
    /// Sources and stack prefixes, newest last.
    imgs: ImgCache,
    presented: Option<(String, Arc<Document>)>,
    last_error: Option<String>,
    /// B5-15: CPU bakes and previews run by the worker (trace).
    cpu_bakes: u64,
    cpu_previews: u64,
}

/// B5-15: which route smart filters took (tests and benches).
#[doc(hidden)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SmartFilterTrace {
    /// GPU stack stages the resident renderer executed.
    pub gpu_stages: u64,
    /// Stacks the resident renderer evaluated on the CPU instead.
    pub cpu_fallbacks: u64,
    /// Bytes of GPU stage results it keeps (bounded by its budget).
    pub stage_cache_bytes: u64,
    /// CPU bakes and previews run by the session's filter worker.
    pub cpu_bakes: u64,
    pub cpu_previews: u64,
}

impl Inner {
    fn cancel_active(&self) {
        if let Some(cancel) = &self.running {
            cancel.cancel();
        }
        if let Some(active) = &self.running_bake {
            active.cancel.cancel();
        }
    }

    fn finish_preview(&mut self, generation: u64, cancel: &Arc<RequestCancellation>) -> bool {
        let owns = self
            .running
            .as_ref()
            .is_some_and(|c| Arc::ptr_eq(c, cancel));
        if owns {
            self.running = None;
        }
        owns && !self.stop && self.generation == generation && !cancel.is_cancelled()
    }

    fn finish_bake(&mut self, job: &BakeJob, cancel: &Arc<RequestCancellation>) -> bool {
        let owns = self
            .running_bake
            .as_ref()
            .is_some_and(|active| Arc::ptr_eq(&active.cancel, cancel));
        let current = self
            .running_bake
            .as_ref()
            .is_some_and(|active| active.matches(job));
        if owns {
            self.running_bake = None;
        }
        owns && current && !self.stop && !cancel.is_cancelled()
    }
}

struct Queue {
    m: Mutex<Inner>,
    cv: Condvar,
}

impl Queue {
    fn lock(&self) -> MutexGuard<'_, Inner> {
        self.m.lock().unwrap_or_else(|e| e.into_inner())
    }
}

/// `(layer, index, max_px)` → `(mask key, surface)`.
type MaskThumbs = HashMap<(u64, usize, u32), (String, Arc<Surface>)>;

/// Per-session filter state (a field of [`Shared`]).
pub(crate) struct FilterState {
    q: Arc<Queue>,
    worker: Mutex<Option<std::thread::JoinHandle<()>>>,
    comp: Arc<Compositor>,
    detail: Mutex<Option<Arc<Surface>>>,
    mask_thumbs: Mutex<MaskThumbs>,
    apply_cancel: Mutex<Arc<AtomicBool>>,
    /// B5-15 (P19): the session renders on the resident (GPU) renderer with
    /// the filters crate installed, so eligible stacks take the GPU route.
    gpu: AtomicBool,
    /// B5-15: whether smart-object children are opaque, by child layer.
    opaque: Mutex<Vec<(Weak<Layer>, u64, bool)>>,
}

impl Default for FilterState {
    fn default() -> Self {
        Self {
            q: Arc::new(Queue {
                m: Mutex::new(Inner::default()),
                cv: Condvar::new(),
            }),
            worker: Mutex::new(None),
            comp: Arc::new(super::fonts::compositor(256 << 20)), // B5-10b
            detail: Mutex::new(None),
            mask_thumbs: Mutex::new(HashMap::new()),
            apply_cancel: Mutex::new(Arc::new(AtomicBool::new(false))),
            gpu: AtomicBool::new(false),    // B5-15
            opaque: Mutex::new(Vec::new()), // B5-15
        }
    }
}

impl Drop for FilterState {
    fn drop(&mut self) {
        self.stop();
    }
}

impl FilterState {
    pub(crate) fn stop(&self) {
        {
            let mut i = self.q.lock();
            i.stop = true;
            i.cancel_active();
            i.preview_job = None;
            i.bake_jobs.clear();
        }
        self.q.cv.notify_all();
        if let Some(w) = self.worker.lock().unwrap_or_else(|e| e.into_inner()).take()
            && w.thread().id() != std::thread::current().id()
        {
            let _ = w.join();
        }
    }

    fn ensure_worker(&self, shared: &Arc<Shared>) {
        let mut w = self.worker.lock().unwrap_or_else(|e| e.into_inner());
        if w.is_some() || self.q.lock().stop {
            return;
        }
        let (q, comp, weak) = (self.q.clone(), self.comp.clone(), Arc::downgrade(shared));
        *w = std::thread::Builder::new()
            .name("document-filters".into())
            .spawn(move || worker_loop(q, comp, weak))
            .ok();
    }

    fn wait_idle(&self) {
        let mut i = self.q.lock();
        while !i.stop && (i.busy || i.preview_job.is_some() || !i.bake_jobs.is_empty()) {
            i = self.q.cv.wait(i).unwrap_or_else(|e| e.into_inner());
        }
    }
}

fn cached_img(q: &Queue, key: &str) -> Option<Arc<Img>> {
    q.lock()
        .imgs
        .entries
        .iter()
        .find(|entry| entry.key == key)
        .map(|entry| entry.image.clone())
}

fn store_img(q: &Queue, key: String, img: Arc<Img>) {
    q.lock().imgs.insert(key, img);
}

/// Check retention before a deep prefix copy; copying never holds the queue lock.
/// Insertion rechecks capacity/budget because copied Vec capacity may differ.
fn store_prefix_with(q: &Queue, key: String, image: &Img, copy: impl FnOnce(&Img) -> Img) {
    let retain = {
        let mut state = q.lock();
        state.imgs.remove(&key);
        state.imgs.can_retain(image)
    };
    if retain {
        let cloned = Arc::new(copy(image));
        store_img(q, key, cloned);
    }
}

/// What identifies a layer's own content for the source cache: pixels, text
/// proxy or the smart object's child and transform (not its filters).
fn content_key(l: &Layer) -> String {
    match &l.kind {
        LayerKind::SmartObject(so) => format!(
            "so{}:{:?}:{:?}",
            l.id.0,
            so.state
                .root
                .iter()
                .map(|c| (
                    Arc::as_ptr(c) as usize,
                    super::layer_revision(c),
                    c.props_rev
                ))
                .collect::<Vec<_>>(),
            so.transform.m
        ),
        _ => format!("l{}:{}", l.id.0, super::layer_revision(l)),
    }
}

/// The layer's own pixels (no filters) over `need` at `level`, cached.
fn source(
    q: &Queue,
    comp: &Compositor,
    base: &DocState,
    layer: &Layer,
    level: u8,
    need: Rect,
) -> Result<Arc<Img>> {
    let key = format!("src:{}:{level}:{need:?}", content_key(layer));
    if let Some(i) = cached_img(q, &key) {
        return Ok(i);
    }
    let img = Arc::new(render_region(comp, &solo(base, layer), level, need)?);
    store_img(q, key, img.clone());
    Ok(img)
}

/// Bridge legacy menu JSON while letting the compositor own child coordinates,
/// placement, shared filter masks and per-node blending for native stacks.
struct NativeFilterEvaluator {
    cancel: Arc<RequestCancellation>,
}
impl compositor::render::smart_filters::SmartFilterEvaluator for NativeFilterEvaluator {
    fn evaluate(
        &self,
        input: &Raster,
        filter: &SmartFilter,
        context: &compositor::render::smart_filters::FilterContext,
    ) -> engine_api::EngineResult<Raster> {
        self.cancel.native.check()?;
        if adapter_id(&filter.name) && filter.params.get("filter").is_none() {
            let result = filters::CompositorFilters.evaluate(input, filter, context)?;
            self.cancel.native.check()?;
            return Ok(result);
        }
        // Parsing/raster bridge failures stay failures; typed evaluation below
        // preserves cancellation without inferring it from a later flag.
        let convert =
            |e: crate::BridgeError| engine_api::EngineError::invalid("smart filter", e.to_string());
        let mut node = Node::of(filter).map_err(convert)?;
        node.opacity = 1.0;
        node.blend = BlendMode::Normal;
        let img = Img {
            rect: Rect::of_extent(input.extent()),
            px: raster_rgba(input).map_err(convert)?,
        };
        let out = eval_stack(
            img,
            &[node],
            context.level as u8,
            context.canvas,
            context.profile.clone(),
            &self.cancel.effect,
        )
        .map_err(FilterEvalError::into_engine)?;
        self.cancel.native.check()?;
        raster_from_rgba(input.extent(), compositor::Depth::F32, &out.px, false).map_err(convert)
    }
}

fn native_filtered(
    base: &DocState,
    layer: &Layer,
    nodes: &[Node],
    unplaced: bool,
    cancel: Option<&Arc<RequestCancellation>>,
) -> Result<Img> {
    let cancel = cancel.cloned().unwrap_or_default();
    cancel.native.check()?;
    let neutral = solo(base, layer);
    let mut l = (*neutral.state().root[0]).clone();
    l.kind = layer.kind.clone();
    let LayerKind::SmartObject(so) = &mut l.kind else {
        return Err(failure("native stack requires a smart object"));
    };
    so.filters = nodes.iter().map(Node::store).collect();
    let (canvas, profile) = if unplaced {
        so.transform = Affine::IDENTITY;
        so.filter_mask = None;
        (so.state.canvas, so.state.profile.clone())
    } else {
        (base.canvas, base.profile.clone())
    };
    let mut state = DocState::new(canvas, compositor::Depth::F32);
    state.profile = profile;
    state.next_id = base.next_id;
    state.root.push(Arc::new(l));
    let mut comp = super::fonts::compositor(64 << 20); // B5-10b
    comp.set_filter_evaluator(Arc::new(NativeFilterEvaluator {
        cancel: cancel.clone(),
    }));
    let (_, px) = comp.render_level_rgba_with_cancel(&Document::new(state), 0, &cancel.native)?;
    Ok(Img {
        rect: Rect::of_extent(canvas),
        px,
    })
}

/// Runs `nodes` over the layer's pixels for `region` (with the halo the
/// stack needs), cropped to `region`. Stack prefixes are cached so a
/// re-edited or appended filter does not recompute the filters below it.
#[allow(clippy::too_many_arguments)]
fn filtered(
    q: &Queue,
    comp: &Compositor,
    base: &DocState,
    layer: &Layer,
    nodes: &[Node],
    level: u8,
    region: Rect,
    cancel: &AtomicBool,
) -> Result<Img> {
    filtered_with_cancel(q, comp, base, layer, nodes, level, region, cancel, None)
}

#[allow(clippy::too_many_arguments)]
fn filtered_with_cancel(
    q: &Queue,
    comp: &Compositor,
    base: &DocState,
    layer: &Layer,
    nodes: &[Node],
    level: u8,
    region: Rect,
    cancel: &AtomicBool,
    request: Option<&Arc<RequestCancellation>>,
) -> Result<Img> {
    if full_resolution(nodes) && matches!(layer.kind, LayerKind::SmartObject(_)) {
        if level != 0 {
            return Err(failure("native retouch stacks require level 0"));
        }
        if cancel.load(Ordering::Relaxed) {
            return Err(failure("cancelled"));
        }
        let image = native_filtered(base, layer, nodes, false, request)?;
        if cancel.load(Ordering::Relaxed) {
            return Err(failure("cancelled"));
        }
        return Ok(image.crop(region));
    }
    // B5-12 begin: stacks with transform stages render through the compositor.
    if nodes.iter().any(|n| n.enabled && n.raw.is_some())
        && matches!(layer.kind, LayerKind::SmartObject(_))
    {
        if cancel.load(Ordering::Relaxed) {
            return Err(failure("cancelled"));
        }
        let image = native_stack(base, layer, nodes, level, request)?;
        if cancel.load(Ordering::Relaxed) {
            return Err(failure("cancelled"));
        }
        return Ok(image.crop(region));
    }
    // B5-12 end
    let full = Rect::of_extent(base.canvas.at_level(level));
    let region = region.intersect(&full);
    let need = match stack_halo(nodes, level)? {
        None => full,
        Some(h) => region.inflate(h).intersect(&full),
    };
    // The longest cached prefix of the stack.
    let prefix_key = |k: usize| {
        format!(
            "stack:{}:{level}:{need:?}:{}",
            content_key(layer),
            nodes[..k]
                .iter()
                .map(Node::key)
                .collect::<Vec<_>>()
                .join(";")
        )
    };
    let mut start = 0;
    let mut cur = None;
    for k in (1..=nodes.len()).rev() {
        if let Some(i) = cached_img(q, &prefix_key(k)) {
            start = k;
            cur = Some(i);
            break;
        }
    }
    let mut cur = match cur {
        Some(i) => (*i).clone(),
        None => (*source(q, comp, base, layer, level, need)?).clone(),
    };
    for k in start..nodes.len() {
        cur = eval_stack(
            cur,
            &nodes[k..=k],
            level,
            base.canvas,
            base.profile.clone(),
            cancel,
        )
        .map_err(FilterEvalError::into_bridge)?;
        // Cache the prefix below the top filter (what previews re-run on).
        if k + 2 == nodes.len() {
            store_prefix_with(q, prefix_key(k + 1), &cur, Img::clone);
        }
    }
    Ok(cur.crop(region))
}

/// A linear 0…1 sample, sRGB-encoded to 8 bits.
fn srgb_u8(v: f32) -> u8 {
    let v = if v.is_finite() {
        v.clamp(0.0, 1.0)
    } else {
        0.0
    };
    let e = if v <= 0.003_130_8 {
        12.92 * v
    } else {
        1.055 * v.powf(1.0 / 2.4) - 0.055
    };
    (e * 255.0 + 0.5) as u8
}

/// The stack a preview evaluates.
fn edited_stack(layer: &Layer, edit: &StackEdit) -> Result<Vec<Node>> {
    let mut nodes = match &layer.kind {
        LayerKind::SmartObject(so) => nodes_of(so)?,
        LayerKind::Pixel(_) => Vec::new(),
        _ => return Err(failure("filters apply to pixel layers and smart objects")),
    };
    match edit {
        StackEdit::Append(s) => nodes.push(Node::new(s.clone())),
        StackEdit::Replace(i, s) => {
            let n = nodes
                .get_mut(*i)
                .ok_or_else(|| failure(format!("no smart filter {i}")))?;
            n.spec = s.clone();
            n.enabled = true;
            n.raw = None; // B5-12: a previewed menu filter is not a transform stage
        }
    }
    Ok(nodes)
}

fn worker_loop(q: Arc<Queue>, comp: Arc<Compositor>, shared: Weak<Shared>) {
    loop {
        enum Job {
            Preview(PreviewJob, Arc<RequestCancellation>),
            Bake(BakeJob, Arc<RequestCancellation>),
        }
        let job = {
            let mut i = q.lock();
            loop {
                if i.stop {
                    return;
                }
                if let Some(p) = i.preview_job.take() {
                    let cancel = Arc::new(RequestCancellation::default());
                    i.running = Some(cancel.clone());
                    i.busy = true;
                    break Job::Preview(p, cancel);
                }
                if let Some(id) = i.bake_jobs.keys().next().copied() {
                    let job = i.bake_jobs.remove(&id).expect("bake job");
                    let cancel = Arc::new(RequestCancellation::default());
                    i.running_bake = Some(ActiveBake {
                        layer: job.layer,
                        key: job.key.clone(),
                        level: job.level,
                        region: job.region,
                        cancel: cancel.clone(),
                    });
                    i.busy = true;
                    break Job::Bake(job, cancel);
                }
                let (g, _) =
                    q.cv.wait_timeout(i, std::time::Duration::from_millis(500))
                        .unwrap_or_else(|e| e.into_inner());
                i = g;
                if shared.strong_count() == 0 {
                    return;
                }
            }
        };
        // B5-14 (P17): previews and bakes count as interactive pressure, so
        // photo export yields to them (bounded by its maximum yield).
        let _pressure = super::render::Pressure::begin(super::render::PressureKind::Filters);
        match &job {
            Job::Preview(..) => q.lock().cpu_previews += 1, // B5-15
            Job::Bake(..) => q.lock().cpu_bakes += 1,       // B5-15
        }
        match job {
            Job::Preview(p, cancel) => {
                let result = find(&p.base, p.layer)
                    .map_err(|e| e.to_string())
                    .and_then(|layer| {
                        let nodes = edited_stack(layer, &p.edit).map_err(|e| e.to_string())?;
                        let img = filtered_with_cancel(
                            &q,
                            &comp,
                            &p.base,
                            layer,
                            &nodes,
                            p.level,
                            p.region,
                            &cancel.effect,
                            Some(&cancel),
                        )
                        .map_err(|e| e.to_string())?;
                        cancel.native.check().map_err(|e| e.to_string())?;
                        // The layer's own pixels outside the region; the preview inside.
                        let under = match &layer.kind {
                            LayerKind::Pixel(r) => Some(r),
                            _ => None,
                        };
                        upsampled(&img, p.level, p.base.canvas, p.base.depth, under)
                            .map_err(|e| e.to_string())
                    });
                let mut i = q.lock();
                if i.finish_preview(p.generation, &cancel) {
                    match result {
                        Ok(raster) => {
                            i.preview = Some((
                                p.generation,
                                PreviewShown::Replace(p.layer, Arc::new(raster)),
                            ));
                            i.last_error = None;
                        }
                        Err(e) => i.last_error = Some(e),
                    }
                }
            }
            Job::Bake(b, cancel) => {
                let result = bake(&q, &comp, &b, &cancel);
                let mut i = q.lock();
                if i.finish_bake(&b, &cancel) {
                    match result {
                        Ok(Some(baked)) => {
                            i.bakes.insert(b.layer, baked);
                            i.bake_serial += 1;
                        }
                        Ok(None) => {}
                        Err(e) => i.last_error = Some(e.to_string()),
                    }
                }
            }
        }
        {
            let mut i = q.lock();
            i.busy = false;
        }
        q.cv.notify_all();
        if let Some(s) = shared.upgrade() {
            s.render.request(Vec::new(), false, 0);
        }
    }
}

fn bake(
    q: &Queue,
    comp: &Compositor,
    b: &BakeJob,
    cancel: &Arc<RequestCancellation>,
) -> Result<Option<Baked>> {
    cancel.native.check()?;
    let layer = find(&b.base, b.layer)?;
    let LayerKind::SmartObject(so) = &layer.kind else {
        return Ok(None);
    };
    let nodes = nodes_of(so)?;
    let canvas = b.base.canvas;
    let previous = {
        let i = q.lock();
        i.bakes
            .get(&b.layer)
            .filter(|p| p.key == b.key)
            .map(|p| (p.raster.clone(), p.level, p.fine.clone()))
    };
    match b.region {
        Some(r0) => {
            let img = filtered_with_cancel(
                q,
                comp,
                &b.base,
                layer,
                &nodes,
                0,
                r0,
                &cancel.effect,
                Some(cancel),
            )?;
            let (under, level, mut fine) =
                previous.map_or((None, 8, Vec::new()), |(r, l, f)| (Some(r), l, f));
            let raster = upsampled(&img, 0, canvas, b.base.depth, under.as_ref())?;
            fine.push(img.rect);
            Ok(Some(Baked {
                key: b.key.clone(),
                raster,
                level,
                fine,
            }))
        }
        None => {
            let full = Rect::of_extent(canvas.at_level(b.level));
            let img = filtered_with_cancel(
                q,
                comp,
                &b.base,
                layer,
                &nodes,
                b.level,
                full,
                &cancel.effect,
                Some(cancel),
            )?;
            Ok(Some(Baked {
                key: b.key.clone(),
                raster: upsampled(&img, b.level, canvas, b.base.depth, None)?,
                level: b.level,
                fine: Vec::new(),
            }))
        }
    }
}

// ─────────────────────────── GPU smart filters (B5-15) ───────────────────────────
//
// Perf audit P19. The most common smart filter, Gaussian Blur, is evaluated by
// the resident renderer (compositor M5-23: `ResidentRenderer::filtered_buffer`
// with `filters::CompositorFilters` as its `ResidentFilterEvaluator`) instead
// of being baked on the CPU by the worker above. The presented document then
// keeps the smart object, with its stack in engine form; the resident renderer
// caches the child composite and every stack prefix by content hash, so a
// drag of one filter re-runs that filter (and those above it) only, on the GPU,
// and nothing crosses to the CPU.
//
// Taken only where the two routes agree within the operator contract
// (docs/11 §1.3), so the CPU bake stays the fallback and the export path:
//
// * every enabled filter is `gaussian_blur` with sigma ≤ `GPU_MAX_RADIUS`
//   (larger sigmas use the area-reduction path, whose grid follows the CPU
//   route's evaluation blocks), Normal blending, no per-filter mask;
// * the smart object sits at identity over a child canvas of the document's
//   size, without a shared filter mask or layer styles, and its child is one
//   opaque pixel layer (the CPU route blurs premultiplied colour, the engine
//   route straight colour: they agree where alpha is 1);
// * the session renders on the resident renderer and the document has no
//   layer styles (styled documents composite on the CPU, which bakes).

/// Largest Gaussian sigma (px) the GPU route takes.
const GPU_MAX_RADIUS: f32 = 32.0;

/// Installs the filters crate as the resident renderer's smart-filter
/// evaluator (GPU stages and the CPU fallback of unsupported stages).
pub(crate) fn install_resident(resident: &mut compositor::resident::ResidentRenderer) {
    if let Err(e) = resident.set_filter_evaluator(Arc::new(filters::CompositorFilters)) {
        eprintln!("document: resident smart filters unavailable: {e}");
    }
}

/// `nodes` in engine form when the GPU route applies to them.
fn gpu_filters(nodes: &[Node]) -> Option<Vec<SmartFilter>> {
    let enabled: Vec<&Node> = nodes.iter().filter(|n| n.enabled).collect();
    if enabled.is_empty() {
        return None;
    }
    enabled
        .into_iter()
        .map(|n| {
            if n.spec.id != "gaussian_blur"
                || n.mask_png.is_some()
                || n.blend != BlendMode::Normal
                || !(0.0..=1.0).contains(&n.opacity)
            {
                return None;
            }
            let (effect, p) = n.spec.at(0).ok()?;
            if !matches!(effect, Effect::Gaussian)
                || p.amount != 1.0
                || !(p.radius > 0.0 && p.radius <= GPU_MAX_RADIUS)
            {
                return None;
            }
            Some(SmartFilter {
                name: "gaussian_blur".into(),
                enabled: true,
                blend: compositor::render::smart_filters::FilterBlend {
                    mode: BlendMode::Normal,
                    opacity: n.opacity,
                },
                params: serde_json::json!({ "radius": p.radius }),
            })
        })
        .collect()
}

/// Every alpha sample of `r` is 1 (straight RGBA, all tiles present).
fn raster_opaque(r: &Raster) -> bool {
    if r.channels() != 4 {
        return false;
    }
    let (cols, rows) = r.grid();
    let coords: Vec<(u32, u32)> = (0..rows)
        .flat_map(|y| (0..cols).map(move |x| (x, y)))
        .collect();
    let next = std::sync::atomic::AtomicUsize::new(0);
    let transparent = AtomicBool::new(false);
    std::thread::scope(|s| {
        for _ in 0..threads().min(coords.len()) {
            s.spawn(|| {
                while !transparent.load(Ordering::Relaxed) {
                    let i = next.fetch_add(1, Ordering::Relaxed);
                    let Some(&(tx, ty)) = coords.get(i) else {
                        break;
                    };
                    let opaque = match r.tile(tx, ty) {
                        None => r.default_value() >= 1.0,
                        Some(t) => {
                            let l = t.layout();
                            let (w, h, stride, n) = (
                                l.extent.width as usize,
                                l.extent.height as usize,
                                l.stride(),
                                l.plane_len(),
                            );
                            let rows = |f: &dyn Fn(usize) -> bool| {
                                (0..h).all(|y| (0..w).all(|x| f(3 * n + y * stride + x)))
                            };
                            if let Ok(v) = t.samples::<u8>() {
                                rows(&|i| v[i] == u8::MAX)
                            } else if let Ok(v) = t.samples::<u16>() {
                                rows(&|i| v[i] == u16::MAX)
                            } else if let Ok(v) = t.samples::<f32>() {
                                rows(&|i| v[i] >= 1.0)
                            } else {
                                false
                            }
                        }
                    };
                    if !opaque {
                        transparent.store(true, Ordering::Relaxed);
                    }
                }
            });
        }
    });
    !transparent.load(Ordering::Relaxed)
}

/// The child of `so` composites to an opaque canvas: one plain, visible,
/// full-opacity pixel layer whose every pixel is opaque (cached per child
/// layer and revision).
fn child_opaque(fs: &FilterState, so: &SmartObject) -> bool {
    let [child] = so.state.root.as_slice() else {
        return false;
    };
    let p = &child.props;
    let LayerKind::Pixel(raster) = &child.kind else {
        return false;
    };
    if !p.visible
        || p.opacity < 1.0
        || p.fill_opacity < 1.0
        || p.blend_mode != BlendMode::Normal
        || p.clipped
        || child.mask.is_some()
        || !p.styles.effects.is_empty()
        || raster.extent() != so.state.canvas
    {
        return false;
    }
    let rev = super::layer_revision(child);
    let mut cache = fs.opaque.lock().unwrap_or_else(|e| e.into_inner());
    cache.retain(|(w, _, _)| w.strong_count() > 0);
    if let Some((_, _, o)) = cache
        .iter()
        .find(|(w, r, _)| *r == rev && w.upgrade().is_some_and(|c| Arc::ptr_eq(&c, child)))
    {
        return *o;
    }
    drop(cache);
    let opaque = raster_opaque(raster);
    let mut cache = fs.opaque.lock().unwrap_or_else(|e| e.into_inner());
    if cache.len() >= 32 {
        cache.remove(0);
    }
    cache.push((Arc::downgrade(child), rev, opaque));
    opaque
}

/// `layer` (a smart object of a document with `canvas`) with `nodes` in
/// engine form, when the GPU route applies; `None` keeps the CPU bake.
fn gpu_layer(fs: &FilterState, state: &DocState, layer: &Layer, nodes: &[Node]) -> Option<Layer> {
    if !fs.gpu.load(Ordering::Relaxed) || state.has_layer_styles() {
        return None;
    }
    let LayerKind::SmartObject(so) = &layer.kind else {
        return None;
    };
    if so.transform != Affine::IDENTITY
        || so.state.canvas != state.canvas
        || so.filter_mask.is_some()
        || so.state.has_layer_styles()
    {
        return None;
    }
    let filters = gpu_filters(nodes)?;
    if !child_opaque(fs, so) {
        return None;
    }
    let mut l = layer.clone();
    if let LayerKind::SmartObject(so) = &mut l.kind {
        so.filters = filters;
    }
    Some(l)
}

impl FilterState {
    /// B5-15: enables the GPU route (the session renders on the resident
    /// renderer, which has the filters installed); tests switch it off to
    /// compare with the CPU bake.
    pub(crate) fn set_gpu(&self, on: bool) {
        self.gpu.store(on, Ordering::Relaxed);
        self.q.lock().presented = None;
    }
}

/// B5-15: whether `for_output` has smart filters to bake.
pub(crate) fn has_filtered_smart_objects(state: &DocState) -> bool {
    !filtered_smart_objects(state).is_empty()
}

/// Smart objects with at least one enabled smart filter, anywhere in the tree.
fn filtered_smart_objects(state: &DocState) -> Vec<&Layer> {
    fn go<'a>(v: &'a [Arc<Layer>], out: &mut Vec<&'a Layer>) {
        for l in v {
            match &l.kind {
                LayerKind::SmartObject(so) if so.filters.iter().any(|f| f.enabled) => out.push(l),
                LayerKind::Group { children, .. } => go(children, out),
                _ => {}
            }
        }
    }
    let mut out = Vec::new();
    go(&state.root, &mut out);
    out
}

fn bake_key(l: &Layer) -> String {
    match &l.kind {
        LayerKind::SmartObject(so) => format!(
            "{}|{}",
            content_key(l),
            so.filters
                .iter()
                .map(|f| Node::of(f).map(|n| n.key()).unwrap_or_default())
                .collect::<Vec<_>>()
                .join(";")
        ),
        _ => String::new(),
    }
}

/// The level-0 region a level-0 bake refines for a view of `view` (level 0):
/// the view plus a margin, snapped to 512-pixel blocks so small pans reuse it.
fn fine_region(view: Rect, canvas: Extent) -> Rect {
    let b = 512;
    let v = view.inflate(b);
    Rect::new(
        v.x0.div_euclid(b) * b,
        v.y0.div_euclid(b) * b,
        (v.x1 + b - 1).div_euclid(b) * b,
        (v.y1 + b - 1).div_euclid(b) * b,
    )
    .intersect(&Rect::of_extent(canvas))
}

/// A copy of `state` with layers replaced and an adjustment inserted.
fn substitute(
    state: &DocState,
    subs: &[(u64, Layer)],
    insert_above: Option<(u64, Layer)>,
) -> DocState {
    let mut s = state.clone();
    for (id, l) in subs {
        s.layer_mut(LayerId(*id), |x| *x = l.clone());
    }
    if let Some((above, adj)) = insert_above
        && let Some((parent, i)) = s.locate(LayerId(above))
    {
        let adj = Arc::new(adj);
        match parent {
            None => s.root.insert(i + 1, adj),
            Some(p) => {
                s.layer_mut(p, |g| {
                    if let LayerKind::Group { children, .. } = &mut g.kind {
                        children.insert(i + 1, adj);
                    }
                });
            }
        }
    }
    s
}

/// The document to present for `live` at `level` showing `view` (level
/// coordinates): smart filters baked, the preview shown. `None` when that is
/// the live document itself. Schedules missing bakes on the worker.
pub(crate) fn presented(
    shared: &Arc<Shared>,
    live: &Document,
    level: u8,
    view: Rect,
) -> Option<Arc<Document>> {
    let fs = &shared.filters;
    let state = live.state();
    let sos = filtered_smart_objects(state);
    // B5-12: geometric transform stacks are rendered by the session renderer.
    let sos: Vec<&Layer> = sos.into_iter().filter(|l| !compositor_stack(l)).collect();
    let mut i = fs.q.lock();
    i.bake_jobs
        .retain(|id, job| sos.iter().any(|l| l.id.0 == *id && bake_key(l) == job.key));
    if let Some(active) = &i.running_bake
        && !sos
            .iter()
            .any(|l| l.id.0 == active.layer && bake_key(l) == active.key)
    {
        active.cancel.cancel();
    }
    if sos.is_empty() && i.preview.is_none() {
        i.presented = None;
        return None;
    }
    let mut subs = Vec::new();
    let mut key = format!("{:p}|{}|", Arc::as_ptr(state), i.bake_serial);
    let mut spawn = false;
    for l in &sos {
        let id = l.id.0;
        // B5-15 (P19): the resident renderer evaluates the stack on the GPU.
        if let LayerKind::SmartObject(so) = &l.kind
            && let Ok(nodes) = nodes_of(so)
            && let Some(nl) = gpu_layer(fs, state, l, &nodes)
        {
            i.bakes.remove(&id);
            i.bake_jobs.remove(&id);
            key.push_str(&format!(
                "{id}:g:{:p}:{};",
                Arc::as_ptr(&so_arc(l)),
                bake_key(l)
            ));
            subs.push((id, nl));
            continue;
        }
        let bk = bake_key(l);
        let wanted_region = (level == 0).then(|| fine_region(view.to_level0(level), state.canvas));
        let entry = i.bakes.get(&id);
        let fresh = entry.is_some_and(|b| b.key == bk);
        let good = entry.is_some_and(|b| {
            b.key == bk
                && (b.level <= level
                    || wanted_region.is_some_and(|w| b.fine.iter().any(|f| f.intersect(&w) == w)))
        });
        if !good {
            // A full bake at this level first; level 0 then refines the view.
            let exact = match &l.kind {
                LayerKind::SmartObject(so) => nodes_of(so).is_ok_and(|n| full_resolution(&n)),
                _ => false,
            };
            let job_level = if exact || (level == 0 && fresh) {
                0
            } else {
                level.max(1)
            };
            let job = BakeJob {
                key: bk.clone(),
                base: state.clone(),
                layer: id,
                level: job_level,
                region: if exact {
                    None
                } else {
                    (job_level == 0).then_some(wanted_region).flatten()
                },
            };
            let queued = i.bake_jobs.get(&id).is_some_and(|j| {
                j.key == job.key && j.level == job.level && j.region == job.region
            });
            let running = i
                .running_bake
                .as_ref()
                .is_some_and(|active| active.matches(&job));
            if !running {
                if let Some(active) = &i.running_bake
                    && active.layer == id
                {
                    active.cancel.cancel();
                }
                if !queued {
                    i.bake_jobs.insert(id, job);
                    spawn = true;
                }
            }
        }
        if let Some(b) = i.bakes.get(&id) {
            let mut nl = (*l).clone();
            nl.kind = LayerKind::Pixel(b.raster.clone());
            key.push_str(&format!(
                "{id}:{:p}:{}:{};",
                Arc::as_ptr(&so_arc(l)),
                b.level,
                b.fine.len()
            ));
            subs.push((id, nl));
        } else {
            // Not baked yet: show the unfiltered contents until the bake lands.
            key.push_str(&format!("{id}:u;"));
            subs.push((id, unfiltered(l)));
        }
    }
    let mut insert = None;
    if let Some((generation, shown)) = &i.preview {
        match shown {
            PreviewShown::Replace(id, so) => {
                if let Some(l) = state.find(LayerId(*id)) {
                    let mut nl = l.clone();
                    nl.kind = LayerKind::Pixel((**so).clone());
                    subs.retain(|(s, _)| s != id);
                    subs.push((*id, nl));
                    key.push_str(&format!("p{generation}:{:p}", Arc::as_ptr(so)));
                }
            }
            PreviewShown::Stack(id, nl) => {
                if state.find(LayerId(*id)).is_some() {
                    subs.retain(|(s, _)| s != id);
                    subs.push((*id, (**nl).clone()));
                    key.push_str(&format!("s{generation}"));
                }
            }
            PreviewShown::ClippedAdjustment(id, adj) => {
                if state.find(LayerId(*id)).is_some() {
                    let mut l = Layer::new("preview", LayerKind::Adjustment(adj.clone()));
                    l.id = LayerId(state.next_id);
                    l.props.clipped = true;
                    insert = Some((*id, l));
                    key.push_str(&format!("a{generation}"));
                }
            }
        }
    }
    if spawn {
        drop(i);
        fs.q.cv.notify_all();
        fs.ensure_worker(shared);
        i = fs.q.lock();
    }
    if subs.is_empty() && insert.is_none() {
        i.presented = None;
        return None;
    }
    if let Some((k, d)) = &i.presented
        && *k == key
    {
        return Some(d.clone());
    }
    let mut s = substitute(state, &subs, insert);
    s.rev = state.rev;
    let doc = Arc::new(Document::new(s));
    i.presented = Some((key, doc.clone()));
    Some(doc)
}

/// `state` with every smart filter removed (quick previews such as thumbnails).
pub(crate) fn unfiltered_state(state: &DocState) -> DocState {
    fn go(v: &[Arc<Layer>]) -> Vec<Arc<Layer>> {
        v.iter()
            .map(|l| match &l.kind {
                LayerKind::SmartObject(so) if !so.filters.is_empty() => Arc::new(unfiltered(l)),
                LayerKind::Group { children, mode } => {
                    let mut g = (**l).clone();
                    g.kind = LayerKind::Group {
                        mode: *mode,
                        children: go(children),
                    };
                    Arc::new(g)
                }
                _ => l.clone(),
            })
            .collect()
    }
    let mut s = state.clone();
    s.root = go(&state.root);
    s
}

/// Identity of a layer's smart object state (presented-cache key part).
fn so_arc(l: &Layer) -> Arc<DocState> {
    match &l.kind {
        LayerKind::SmartObject(so) => so.state.clone(),
        _ => Arc::new(DocState::new(Extent::new(1, 1), compositor::Depth::U8)),
    }
}

/// `doc` with every smart filter baked at full resolution (export, flatten).
pub(crate) fn for_output(doc: Document) -> Result<Document> {
    for_output_with(doc, &AtomicBool::new(false), &|_| {})
}

/// B5-15 (P16): [`for_output`] that stops when `cancel` is set and reports
/// the fraction of smart objects baked.
pub(crate) fn for_output_with(
    doc: Document,
    cancel: &AtomicBool,
    progress: &dyn Fn(f32),
) -> Result<Document> {
    let state = doc.state().clone();
    let sos = filtered_smart_objects(&state);
    if sos.is_empty() {
        return Ok(doc);
    }
    let comp = super::fonts::compositor(128 << 20); // B5-10b
    let q = Queue {
        m: Mutex::new(Inner::default()),
        cv: Condvar::new(),
    };
    let total = sos.len();
    let mut subs = Vec::new();
    for (i, l) in sos.into_iter().enumerate() {
        progress(i as f32 / total as f32);
        if cancel.load(Ordering::Relaxed) {
            return Err(failure("export cancelled"));
        }
        let LayerKind::SmartObject(so) = &l.kind else {
            continue;
        };
        let nodes = nodes_of(so)?;
        let img = filtered(
            &q,
            &comp,
            &state,
            l,
            &nodes,
            0,
            Rect::of_extent(state.canvas),
            cancel,
        )?;
        let mut nl = l.clone();
        nl.kind = LayerKind::Pixel(upsampled(&img, 0, state.canvas, state.depth, None)?);
        subs.push((l.id.0, nl));
    }
    progress(1.0);
    let mut s = substitute(&state, &subs, None);
    s.rev = state.rev;
    Ok(Document::new(s))
}

// ─────────────────────────────── session calls ───────────────────────────────

/// The level and level region a preview renders for the current view.
fn preview_view(st: &super::State, region: Option<DocRect>) -> (u8, Rect) {
    let canvas = st.live().state().canvas;
    let level = match (st.view.viewport, st.view.surfaces.first()) {
        (Some(v), _) => v.level,
        (None, Some(s)) => (0..super::render::MAX_VIEW_LEVEL)
            .find(|&l| {
                let e = canvas.at_level(l);
                e.width <= s.width() && e.height <= s.height()
            })
            .unwrap_or(super::render::MAX_VIEW_LEVEL - 1),
        // No viewport: a level of at most ~2 MP.
        (None, None) => (0..super::render::MAX_VIEW_LEVEL)
            .find(|&l| canvas.at_level(l).area() <= 2_000_000)
            .unwrap_or(super::render::MAX_VIEW_LEVEL - 1),
    };
    let full = Rect::of_extent(canvas.at_level(level));
    let r = match region {
        Some(r) => Rect::new(r.x, r.y, r.x + r.width, r.y + r.height)
            .to_level(level)
            .intersect(&full),
        None => full,
    };
    (level, if r.is_empty() { full } else { r })
}

/// The level and level region a preview of the edited stack `nodes` renders:
/// the view's (`preview_view`), or the whole level-0 canvas when an adapter
/// in the stack needs it. `submit_preview` and `filter_preview_level` both
/// use it, so the sheet's note and the engine cannot disagree.
fn preview_plan(st: &super::State, nodes: &[Node], region: Option<DocRect>) -> (u8, Rect) {
    if full_resolution(nodes) {
        (0, Rect::of_extent(st.live().state().canvas))
    } else {
        preview_view(st, region)
    }
}

impl DocumentSession {
    fn filter_target(&self, layer: u64) -> Result<(Arc<DocState>, Option<Arc<Raster>>)> {
        let st = self.shared.lock()?;
        st.open()?;
        let s = st.live().state().clone();
        let l = find(&s, layer)?;
        if !matches!(l.kind, LayerKind::Pixel(_) | LayerKind::SmartObject(_)) {
            return Err(failure("filters apply to pixel layers and smart objects"));
        }
        let sel = s.selection.clone();
        Ok((s, sel))
    }

    fn submit_preview(&self, layer: u64, edit: StackEdit, region: Option<DocRect>) -> Result<()> {
        let (base, level, region, nodes) = {
            let st = self.shared.lock()?;
            st.open()?;
            let s = st.live().state().clone();
            let l = find(&s, layer)?;
            let nodes = edited_stack(l, &edit)?;
            let (level, r) = preview_plan(&st, &nodes, region);
            (s, level, r, nodes)
        };
        // B5-18b: below 100 % a Camera Raw preview omits its detail effects.
        let (edit, nodes) = if level > 0 {
            let edit = edit.at_preview_level(level)?;
            let nodes = edited_stack(find(&base, layer)?, &edit)?;
            (edit, nodes)
        } else {
            (edit, nodes)
        };
        let fs = &self.shared.filters;
        // B5-15 (P19): shown by the resident renderer, on the render thread's
        // next frame (no worker job, no CPU pixels).
        if let Some(nl) = gpu_layer(fs, &base, find(&base, layer)?, &nodes) {
            {
                let mut i = fs.q.lock();
                i.generation += 1;
                i.preview_job = None;
                if let Some(c) = &i.running {
                    c.cancel();
                }
                let g = i.generation;
                i.preview = Some((g, PreviewShown::Stack(layer, Arc::new(nl))));
                i.last_error = None;
            }
            let epoch = self.shared.lock()?.epoch;
            self.shared.render.request(Vec::new(), false, epoch);
            return Ok(());
        }
        {
            let mut i = fs.q.lock();
            i.generation += 1;
            if let Some(c) = &i.running {
                c.cancel();
            }
            let generation = i.generation;
            i.preview_job = Some(PreviewJob {
                generation,
                base,
                layer,
                edit,
                level,
                region,
            });
        }
        fs.q.cv.notify_all();
        fs.ensure_worker(&self.shared);
        Ok(())
    }

    /// Writes pixels of `img` (level 0) into the pixel layer as one paint
    /// node, blended by the selection and keeping alpha under a
    /// transparency lock. Fails when the layer changed since `base`.
    fn write_pixels(
        &self,
        base: &DocState,
        layer: u64,
        img: &Img,
        label: &str,
    ) -> Result<DocumentUpdate> {
        let l = find(base, layer)?;
        let LayerKind::Pixel(raster) = &l.kind else {
            return Err(failure("not a pixel layer"));
        };
        let depth = base.depth;
        let r = img.rect;
        let src = read_raster(raster, r)?;
        let sel = base
            .selection
            .as_ref()
            .map(|s| read_raster(s, r))
            .transpose()?;
        let keep_alpha = l.props.locks.transparency;
        let ts = i64::from(TILE_SIZE);
        let mut tiles = Vec::new();
        let mut buf = Vec::new();
        let w = r.width() as usize;
        for ty in r.y0 / ts..=(r.y1 - 1) / ts {
            for tx in r.x0 / ts..=(r.x1 - 1) / ts {
                let (tx32, ty32) = (tx as u32, ty as u32);
                let lay = raster.layout(tx32, ty32);
                raster.read_tile(tx32, ty32, &mut buf)?;
                let n = lay.plane_len();
                let mut any = false;
                for y in 0..lay.extent.height as i64 {
                    let gy = ty * ts + y;
                    for x in 0..lay.extent.width as i64 {
                        let gx = tx * ts + x;
                        let i = y as usize * lay.stride() + x as usize;
                        if gx >= r.x0 && gx < r.x1 && gy >= r.y0 && gy < r.y1 {
                            let j = (gy - r.y0) as usize * w + (gx - r.x0) as usize;
                            let k = sel.as_ref().map_or(1.0, |s| s[j].clamp(0.0, 1.0));
                            for c in 0..4 {
                                let (a, b) = (src[j * 4 + c], img.px[j * 4 + c]);
                                buf[c * n + i] = if c == 3 && keep_alpha {
                                    a
                                } else {
                                    a + k * (b - a)
                                };
                            }
                        }
                        any |= buf[3 * n + i] != 0.0;
                    }
                }
                let tile = if any || raster.tile(tx32, ty32).is_some() {
                    Some(tile_from_f32(
                        TileCoord::new(0, tx32, ty32),
                        lay,
                        depth,
                        buf.clone(),
                    )?)
                } else {
                    None
                };
                tiles.push(TileDelta {
                    tx: tx32,
                    ty: ty32,
                    tile,
                });
            }
        }
        let op = DocOp::PaintTiles {
            id: LayerId(layer),
            target: PaintTarget::Content,
            tiles,
            dirty: r,
        };
        self.edit_checked(layer, super::layer_revision(l), op, label)
    }

    /// One history node, provided the layer's content is still `revision`.
    fn edit_checked(
        &self,
        layer: u64,
        revision: u64,
        op: DocOp,
        label: &str,
    ) -> Result<DocumentUpdate> {
        self.edit_checked_until(layer, revision, op, label, &AtomicBool::new(false))
    }

    /// `edit_checked`, unless `cancel` is set by the time the node would be
    /// applied (checked under the document lock, just before the write).
    fn edit_checked_until(
        &self,
        layer: u64,
        revision: u64,
        op: DocOp,
        label: &str,
        cancel: &AtomicBool,
    ) -> Result<DocumentUpdate> {
        self.clear_preview_state();
        let mut st = self.shared.lock()?;
        st.open()?;
        let before = st.live().state().clone();
        self.commit_pending(&mut st, None)?;
        let now = super::layer_revision(find(st.doc.state(), layer)?);
        if now != revision {
            return Err(failure(
                "the layer changed while the filter ran; apply it again",
            ));
        }
        super::liquify::apply_checkpoint(&self.shared, "write"); // B5-13
        if cancel.load(Ordering::SeqCst) {
            return Err(failure("cancelled"));
        }
        let applied = st.doc.apply(op)?;
        st.labels.insert(applied.node, label.to_owned());
        Ok(self.update(&mut st, &before, Some(&applied), true))
    }

    fn clear_preview_state(&self) -> bool {
        let fs = &self.shared.filters;
        let mut i = fs.q.lock();
        i.generation += 1;
        i.preview_job = None;
        if let Some(c) = &i.running {
            c.cancel();
        }
        i.preview.take().is_some()
    }

    /// Replaces a smart object's filter list (one node).
    fn set_nodes(
        &self,
        layer: u64,
        label: &str,
        f: impl FnOnce(&mut Vec<Node>) -> Result<()>,
    ) -> Result<DocumentUpdate> {
        self.set_nodes_checked(layer, label, None, f)
    }

    fn set_nodes_checked(
        &self,
        layer: u64,
        label: &str,
        expected_revision: Option<u64>,
        f: impl FnOnce(&mut Vec<Node>) -> Result<()>,
    ) -> Result<DocumentUpdate> {
        self.set_nodes_checked_until(layer, label, expected_revision, &AtomicBool::new(false), f)
    }

    /// `set_nodes_checked`, with `cancel` stopping the validation render and
    /// checked again under the document lock just before the write.
    fn set_nodes_checked_until(
        &self,
        layer: u64,
        label: &str,
        expected_revision: Option<u64>,
        cancel: &AtomicBool,
        f: impl FnOnce(&mut Vec<Node>) -> Result<()>,
    ) -> Result<DocumentUpdate> {
        self.clear_preview_state();
        let mut st = self.shared.lock()?;
        st.open()?;
        let before = st.live().state().clone();
        self.commit_pending(&mut st, None)?;
        let s = st.doc.state().clone();
        let l = find(&s, layer)?;
        if expected_revision.is_some_and(|r| r != super::layer_revision(l)) {
            return Err(failure(
                "the layer changed while the filter ran; apply it again",
            ));
        }
        let LayerKind::SmartObject(so) = &l.kind else {
            return Err(failure(format!("layer {layer} is not a smart object")));
        };
        let mut nodes = nodes_of(so)?;
        // B5-12 begin: transform stages keep the engine's position-lock rule.
        let transforms_before = transform_signature(&nodes);
        // B5-12 end
        f(&mut nodes)?;
        // B5-12 begin
        if (l.props.locks.position || l.props.locks.all)
            && transform_signature(&nodes) != transforms_before
        {
            return Err(failure("transform is locked"));
        }
        // B5-12 end
        // Validate with actual pixels/context before changing history. In particular,
        // unavailable neural weights must never leave an unrenderable smart node.
        if full_resolution(&nodes) {
            filtered(
                &self.shared.filters.q,
                &self.shared.filters.comp,
                &s,
                l,
                &nodes,
                0,
                Rect::of_extent(s.canvas),
                cancel,
            )?;
        } else if nodes.iter().any(|n| n.enabled && level_aware(&n.spec.id)) {
            // B5-18b: the Camera Raw Filter (settings, document profile and
            // renderer) is checked on a small level, not the whole canvas.
            let level = (0..super::render::MAX_VIEW_LEVEL)
                .find(|&lv| s.canvas.at_level(lv).area() <= 262_144)
                .unwrap_or(super::render::MAX_VIEW_LEVEL - 1);
            filtered(
                &self.shared.filters.q,
                &self.shared.filters.comp,
                &s,
                l,
                &nodes,
                level,
                Rect::of_extent(s.canvas.at_level(level)),
                cancel,
            )?;
        }
        let mut nl = l.clone();
        if let LayerKind::SmartObject(so) = &mut nl.kind {
            so.filters = nodes.iter().map(Node::store).collect();
        }
        let (parent, index) = s
            .locate(LayerId(layer))
            .ok_or_else(|| failure("layer not found"))?;
        super::liquify::apply_checkpoint(&self.shared, "write"); // B5-13
        if cancel.load(Ordering::SeqCst) {
            return Err(failure("cancelled"));
        }
        let applied = st.doc.apply(DocOp::Batch(vec![
            DocOp::RemoveLayer { id: LayerId(layer) },
            DocOp::AddLayer {
                parent,
                index,
                layer: nl,
            },
        ]))?;
        st.labels.insert(applied.node, label.to_owned());
        let mut u = self.update(&mut st, &before, Some(&applied), true);
        // The layer was replaced in place, not created.
        u.created.clear();
        // B5-15: the replaced smart object is a new instance to the resident
        // stage cache, whose entries for the old one are dead.
        self.shared.render.trim_smart_filter_cache();
        Ok(u)
    }
}

#[uniffi::export]
impl DocumentSession {
    /// Shows `filter_json` applied to `layer` in the viewport, rendered on
    /// the viewport's pyramid level over `region` (level-0 canvas pixels;
    /// `None`: the whole canvas) on a worker thread. No history node. A newer
    /// preview cancels this one; the frame arrives through the listener.
    /// On a smart object the filter is previewed on top of its smart filters.
    pub fn preview_filter(
        &self,
        layer: u64,
        filter_json: String,
        region: Option<DocRect>,
    ) -> Result<()> {
        let spec = Spec::parse(&filter_json)?;
        self.submit_preview(layer, StackEdit::Append(spec), region)
    }

    /// Like `preview_filter`, re-editing smart filter `index` of a smart object.
    pub fn preview_smart_filter(
        &self,
        layer: u64,
        index: u32,
        filter_json: String,
        region: Option<DocRect>,
    ) -> Result<()> {
        let spec = Spec::parse(&filter_json)?;
        self.submit_preview(layer, StackEdit::Replace(index as usize, spec), region)
    }

    /// The pyramid level a `preview_filter` (or, with `smart_index`,
    /// `preview_smart_filter`) of `filter_json` on `layer` would render at
    /// with the current viewport. B5-18b: above 0 a Camera Raw preview omits
    /// its detail effects (the sheet says so).
    pub fn filter_preview_level(
        &self,
        layer: u64,
        smart_index: Option<u32>,
        filter_json: String,
    ) -> Result<u8> {
        let spec = Spec::parse(&filter_json)?;
        let edit = match smart_index {
            Some(i) => StackEdit::Replace(i as usize, spec),
            None => StackEdit::Append(spec),
        };
        let st = self.shared.lock()?;
        st.open()?;
        let s = st.live().state().clone();
        let nodes = edited_stack(find(&s, layer)?, &edit)?;
        Ok(preview_plan(&st, &nodes, None).0)
    }

    /// Shows an Image ▸ Adjustments result (`compositor::Adjustment` JSON)
    /// on a pixel layer, live on the GPU (the adjustment clipped to the
    /// layer). No history node.
    pub fn preview_adjustment(&self, layer: u64, adjustment_json: String) -> Result<()> {
        let adj: Adjustment = serde_json::from_str(&adjustment_json)
            .map_err(|e| failure(format!("adjustment JSON: {e}")))?;
        {
            let st = self.shared.lock()?;
            st.open()?;
            if !matches!(find(st.live().state(), layer)?.kind, LayerKind::Pixel(_)) {
                return Err(failure("Image ▸ Adjustments apply to pixel layers"));
            }
        }
        let fs = &self.shared.filters;
        {
            let mut i = fs.q.lock();
            i.generation += 1;
            i.preview_job = None;
            if let Some(c) = &i.running {
                c.cancel();
            }
            let g = i.generation;
            i.preview = Some((g, PreviewShown::ClippedAdjustment(layer, adj)));
        }
        let st = self.shared.lock()?;
        self.shared.render.request(Vec::new(), false, st.epoch);
        Ok(())
    }

    /// Ends a filter or adjustment preview (the viewport shows the document).
    pub fn clear_preview(&self) -> Result<()> {
        self.shared.render.trim_smart_filter_cache(); // B5-15
        if self.clear_preview_state() {
            let st = self.shared.lock()?;
            self.shared.render.request(Vec::new(), false, st.epoch);
        }
        Ok(())
    }

    /// The last preview or smart filter render error (cleared by a good one).
    pub fn filter_error(&self) -> Option<String> {
        self.shared.filters.q.lock().last_error.clone()
    }

    /// Cancels a running `apply_filter` / `apply_adjustment` and the preview.
    pub fn cancel_filter(&self) {
        let fs = &self.shared.filters;
        fs.apply_cancel
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .store(true, Ordering::Relaxed);
        if let Some(c) = &fs.q.lock().running {
            c.cancel();
        }
    }

    /// `filter_json` on the layer's own pixels (on a smart object: after its
    /// smart filters) over `width × height` level-0 pixels at `(x, y)`,
    /// written into an RGBA8 IOSurface (straight alpha, sRGB-encoded like the
    /// canvas shows the document's linear samples) the session retains
    /// until the next call: the filter dialog's 1:1 detail pane. Blocking.
    pub fn filter_detail(
        &self,
        layer: u64,
        filter_json: String,
        x: i64,
        y: i64,
        width: u32,
        height: u32,
    ) -> Result<FilterDetail> {
        let spec = Spec::parse(&filter_json)?;
        self.detail(layer, StackEdit::Append(spec), x, y, width, height)
    }

    /// B5-18b: like `filter_detail`, re-editing smart filter `index` of a
    /// smart object (the edited filter replaces it instead of stacking on top).
    #[allow(clippy::too_many_arguments)]
    pub fn smart_filter_detail(
        &self,
        layer: u64,
        index: u32,
        filter_json: String,
        x: i64,
        y: i64,
        width: u32,
        height: u32,
    ) -> Result<FilterDetail> {
        let spec = Spec::parse(&filter_json)?;
        self.detail(
            layer,
            StackEdit::Replace(index as usize, spec),
            x,
            y,
            width,
            height,
        )
    }
}

impl DocumentSession {
    fn detail(
        &self,
        layer: u64,
        edit: StackEdit,
        x: i64,
        y: i64,
        width: u32,
        height: u32,
    ) -> Result<FilterDetail> {
        if width == 0 || height == 0 || width > 4096 || height > 4096 {
            return Err(failure("detail size must be 1…4096 pixels"));
        }
        let (base, _) = self.filter_target(layer)?;
        let l = find(&base, layer)?;
        let nodes = edited_stack(l, &edit)?;
        let r0 = Rect::new(x, y, x + i64::from(width), y + i64::from(height));
        // Whole-image filters are filtered on a level of at most ~4 MP.
        let level = if !full_resolution(&nodes) && stack_halo(&nodes, 0)?.is_none() {
            (0..super::render::MAX_VIEW_LEVEL)
                .find(|&lv| base.canvas.at_level(lv).area() <= 4_000_000)
                .unwrap_or(0)
        } else {
            0
        };
        let fs = &self.shared.filters;
        let cancel = AtomicBool::new(false);
        let img = filtered(
            &fs.q,
            &fs.comp,
            &base,
            l,
            &nodes,
            level,
            r0.to_level(level),
            &cancel,
        )?;
        let surface = Surface::create_rgba8(width, height).map_err(failure)?;
        surface
            .with_pixels(|px, stride| {
                for yy in 0..height as i64 {
                    for xx in 0..width as i64 {
                        let o = yy as usize * stride + xx as usize * 4;
                        let (gx, gy) = ((x + xx) >> level, (y + yy) >> level);
                        let p = if img.rect.x0 <= gx
                            && gx < img.rect.x1
                            && img.rect.y0 <= gy
                            && gy < img.rect.y1
                        {
                            let q =
                                img.at((gx - img.rect.x0) as usize, (gy - img.rect.y0) as usize);
                            [q[0], q[1], q[2], q[3]]
                        } else {
                            [0.0; 4]
                        };
                        // B5-18b: sRGB-encoded (the pane is an sRGB image
                        // of the linear samples the canvas shows).
                        for c in 0..3 {
                            px[o + c] = srgb_u8(p[c]);
                        }
                        px[o + 3] = (p[3].clamp(0.0, 1.0) * 255.0 + 0.5) as u8;
                    }
                }
            })
            .map_err(failure)?;
        let id = surface.id();
        *fs.detail.lock().unwrap_or_else(|e| e.into_inner()) = Some(Arc::new(surface));
        Ok(FilterDetail {
            surface_id: id,
            width,
            height,
            level,
        })
    }
}

#[uniffi::export]
impl DocumentSession {
    /// Applies `filter_json` to `layer` as one history node (labelled with
    /// the filter's name). Pixel layers are filtered at full resolution
    /// inside the selection (all of it without one); smart objects get the
    /// filter appended to their smart filters, masked by the selection.
    /// Blocking (seconds on large layers): call off the main thread;
    /// `cancel_filter` stops it.
    pub fn apply_raster_filter(
        &self,
        layer: u64,
        request: RasterFilterRequest,
    ) -> Result<DocumentUpdate> {
        let id = match request.operation {
            RasterFilterOperation::Remove => "remove",
            RasterFilterOperation::ContentAwareFill => "content_aware_fill",
            RasterFilterOperation::ContentAwareMove => "content_aware_move",
            RasterFilterOperation::ContentAwareExtend => "content_aware_extend",
            RasterFilterOperation::Liquify => "liquify",
            RasterFilterOperation::CameraRaw => "camera_raw",
            RasterFilterOperation::SkinSmoothing => "neural/skin_smoothing",
            RasterFilterOperation::Colorize => "neural/colorize",
            RasterFilterOperation::JpegArtifactRemoval => "neural/jpeg_artifact_removal",
            // M5-32: preserve the restoration ID for native smart-filter replay.
            RasterFilterOperation::PhotoRestoration => "neural/photo_restoration",
        };
        let params: serde_json::Value =
            serde_json::from_str(&request.params_json).map_err(failure)?;
        self.apply_filter(
            layer,
            serde_json::json!({"id":id,"params":params}).to_string(),
        )
    }

    /// Explicit removal of geometric suggestions, not semantic segmentation.
    /// Detected masks are frozen in smart nodes, never rerun during rendering.
    pub fn remove_distractions(
        &self,
        layer: u64,
        params_json: String,
    ) -> Result<DistractionRemovalResult> {
        let params: serde_json::Value = serde_json::from_str(&params_json).map_err(failure)?;
        let (base, selection) = self.filter_target(layer)?;
        let l = find(&base, layer)?;
        if l.props.locks.pixels || l.props.locks.all {
            return Err(failure("the layer's pixels are locked"));
        }
        if selection.is_some() {
            return Err(failure(
                "remove_distractions requires no active selection; use Remove for selected coverage",
            ));
        }
        let full = Rect::of_extent(base.canvas);
        let nodes = match &l.kind {
            LayerKind::Pixel(_) => Vec::new(),
            LayerKind::SmartObject(so) => nodes_of(so)?,
            _ => return Err(failure("filters apply to pixel layers and smart objects")),
        };
        let cancel = AtomicBool::new(false);
        let img = if matches!(l.kind, LayerKind::SmartObject(_)) {
            native_filtered(&base, l, &nodes, true, None)?
        } else {
            filtered(
                &self.shared.filters.q,
                &self.shared.filters.comp,
                &base,
                l,
                &nodes,
                0,
                full,
                &cancel,
            )?
        };
        let extent = Extent::new(img.rect.width() as u32, img.rect.height() as u32);
        let input = raster_from_rgba(extent, compositor::Depth::F32, &img.px, false)?;
        let (node, report) = filters::detect_distractions(&input, &params)?;
        let spec = Spec::from_value(&serde_json::json!({"id":node.name,"params":node.params}))?;
        let update = if matches!(l.kind, LayerKind::SmartObject(_)) {
            self.set_nodes_checked(
                layer,
                "Remove Distractions",
                Some(super::layer_revision(l)),
                move |nodes| {
                    nodes.push(Node::new(spec));
                    Ok(())
                },
            )?
        } else {
            let output = spec
                .run(
                    &img,
                    &compositor::render::smart_filters::FilterContext {
                        profile: base.profile.clone(),
                        canvas: base.canvas,
                        level: 0,
                    },
                    &cancel,
                )
                .map_err(FilterEvalError::into_bridge)?;
            self.write_pixels(&base, layer, &output, "Remove Distractions")?
        };
        Ok(DistractionRemovalResult {
            update,
            report_json: report.to_string(),
        })
    }

    /// Existing JSON API, also accepting all `RasterFilterOperation` adapter IDs.
    pub fn apply_filter(&self, layer: u64, filter_json: String) -> Result<DocumentUpdate> {
        let spec = Spec::parse(&filter_json)?;
        let (base, sel) = self.filter_target(layer)?;
        let l = find(&base, layer)?;
        if l.props.locks.pixels || l.props.locks.all {
            return Err(failure("the layer's pixels are locked"));
        }
        let name = spec.name();
        if let LayerKind::SmartObject(_) = l.kind {
            if adapter_id(&spec.id) && sel.is_some() {
                return Err(failure(
                    "retouch smart filters require no active selection; supply an explicit operation mask",
                ));
            }
            let mask = sel.as_deref().map(mask_from_selection).transpose()?;
            return self.set_nodes(layer, &name, move |nodes| {
                let mut n = Node::new(spec);
                n.mask_png = mask;
                nodes.push(n);
                Ok(())
            });
        }
        let LayerKind::Pixel(raster) = &l.kind else {
            return Err(failure("filters apply to pixel layers and smart objects"));
        };
        let cancel = {
            let c = Arc::new(AtomicBool::new(false));
            *self
                .shared
                .filters
                .apply_cancel
                .lock()
                .unwrap_or_else(|e| e.into_inner()) = c.clone();
            c
        };
        let full = Rect::of_extent(base.canvas);
        let region = sel
            .as_deref()
            .and_then(super::io::selection_bounds)
            .unwrap_or(full)
            .intersect(&full);
        if region.is_empty() {
            return Err(failure("the selection is empty"));
        }
        let halo = if adapter_id(&spec.id) {
            Halo::WholeImage
        } else {
            let (effect, params) = spec.at(0)?;
            effect.halo(&params)
        };
        let need = match halo {
            Halo::WholeImage => full,
            Halo::Radius(h) => region.inflate(i64::from(h)).intersect(&full),
        };
        let src = Img {
            rect: need,
            px: read_raster(raster, need)?,
        };
        let out = spec
            .run(
                &src,
                &compositor::render::smart_filters::FilterContext {
                    profile: base.profile.clone(),
                    level: 0,
                    canvas: base.canvas,
                },
                &cancel,
            )
            .map_err(FilterEvalError::into_bridge)?
            .crop(region);
        self.write_pixels(&base, layer, &out, &name)
    }

    /// Image ▸ Adjustments: `adjustment_json` (`compositor::Adjustment`, the
    /// JSON of the adjustment layer of the same kind) applied to a pixel
    /// layer's pixels inside the selection, as one history node.
    pub fn apply_adjustment(&self, layer: u64, adjustment_json: String) -> Result<DocumentUpdate> {
        let adj: Adjustment = serde_json::from_str(&adjustment_json)
            .map_err(|e| failure(format!("adjustment JSON: {e}")))?;
        let label = super::adjustment_title(&adj);
        let (base, sel) = self.filter_target(layer)?;
        let l = find(&base, layer)?;
        if !matches!(l.kind, LayerKind::Pixel(_)) {
            return Err(failure("Image ▸ Adjustments apply to pixel layers"));
        }
        if l.props.locks.pixels || l.props.locks.all {
            return Err(failure("the layer's pixels are locked"));
        }
        let full = Rect::of_extent(base.canvas);
        let region = sel
            .as_deref()
            .and_then(super::io::selection_bounds)
            .unwrap_or(full)
            .intersect(&full);
        if region.is_empty() {
            return Err(failure("the selection is empty"));
        }
        // The layer alone with the adjustment clipped to it: the adjustment
        // layer's own maths, alpha kept.
        let mut doc = solo(&base, l);
        let mut s = (**doc.state()).clone();
        let mut a = Layer::new(label, LayerKind::Adjustment(adj));
        a.id = LayerId(s.next_id);
        s.next_id += 1;
        a.props.clipped = true;
        s.root.push(Arc::new(a));
        doc = Document::new(s);
        let img = render_region(&self.shared.filters.comp, &doc, 0, region)?;
        self.write_pixels(&base, layer, &img, label)
    }

    /// The smart filters of a smart object, first applied first.
    pub fn smart_filters(&self, layer: u64) -> Result<Vec<SmartFilterRecord>> {
        let st = self.shared.lock()?;
        let s = st.live().state().clone();
        drop(st);
        let l = find(&s, layer)?;
        let LayerKind::SmartObject(so) = &l.kind else {
            return Ok(Vec::new());
        };
        nodes_of(so)?
            .into_iter()
            .enumerate()
            .map(|(i, n)| {
                Ok(SmartFilterRecord {
                    index: i as u32,
                    filter_id: n.spec.id.clone(),
                    // B5-12: transform stages by operation.
                    name: n
                        .raw
                        .as_ref()
                        .map_or_else(|| n.spec.name(), |r| transform_title(r).to_owned()),
                    enabled: n.enabled,
                    filter_json: n.spec.json.clone(),
                    opacity: n.opacity,
                    blend_mode: blend_name(n.blend),
                    has_mask: n.mask_png.is_some(),
                })
            })
            .collect()
    }

    /// Edits smart filter `index` of a smart object (one history node).
    pub fn set_smart_filter(
        &self,
        layer: u64,
        index: u32,
        edit: SmartFilterEdit,
    ) -> Result<DocumentUpdate> {
        let i = index as usize;
        let label = match &edit {
            SmartFilterEdit::Enabled { enabled: true } => "Enable Smart Filter",
            SmartFilterEdit::Enabled { enabled: false } => "Disable Smart Filter",
            SmartFilterEdit::Params { .. } => "Edit Smart Filter",
            SmartFilterEdit::Blending { .. } => "Smart Filter Blending Options",
        };
        self.set_nodes(layer, label, move |nodes| {
            let n = nodes
                .get_mut(i)
                .ok_or_else(|| failure(format!("no smart filter {i}")))?;
            match edit {
                SmartFilterEdit::Enabled { enabled } => n.enabled = enabled,
                SmartFilterEdit::Params { filter_json } => {
                    // B5-12 begin
                    if n.raw.is_some() {
                        return Err(failure(
                            "edit a transform stage with Edit ▸ Transform (Warp, Perspective Warp, Puppet Warp or Content-Aware Scale)",
                        ));
                    }
                    // B5-12 end
                    let spec = Spec::parse(&filter_json)?;
                    if spec.id != n.spec.id {
                        return Err(failure(
                            "a smart filter keeps its filter; add a new one instead",
                        ));
                    }
                    n.spec = spec;
                }
                SmartFilterEdit::Blending { mode, opacity } => {
                    if !opacity.is_finite() {
                        return Err(failure("opacity must be finite"));
                    }
                    n.blend = parse_blend(&mode)?;
                    n.opacity = opacity.clamp(0.0, 1.0);
                }
            }
            Ok(())
        })
    }

    /// Deletes smart filter `index` (one history node).
    pub fn remove_smart_filter(&self, layer: u64, index: u32) -> Result<DocumentUpdate> {
        let i = index as usize;
        self.set_nodes(layer, "Delete Smart Filter", move |nodes| {
            if i >= nodes.len() {
                return Err(failure(format!("no smart filter {i}")));
            }
            nodes.remove(i);
            Ok(())
        })
    }

    /// The mask of smart filter `index` as a grey RGBA8 IOSurface (white =
    /// filtered; all white without a mask), cached per mask.
    pub fn smart_filter_mask_thumbnail(&self, layer: u64, index: u32, max_px: u32) -> Result<u32> {
        if max_px == 0 || max_px > 1024 {
            return Err(failure("max_px must be 1…1024"));
        }
        let (canvas, node) = {
            let st = self.shared.lock()?;
            let s = st.live().state();
            let l = find(s, layer)?;
            let LayerKind::SmartObject(so) = &l.kind else {
                return Err(failure(format!("layer {layer} is not a smart object")));
            };
            let sf = so
                .filters
                .get(index as usize)
                .ok_or_else(|| failure(format!("no smart filter {index}")))?;
            (s.canvas, Node::of(sf)?)
        };
        let key = node.mask_png.as_deref().unwrap_or("").to_owned();
        let slot = (layer, index as usize, max_px);
        let fs = &self.shared.filters;
        if let Some((k, s)) = fs
            .mask_thumbs
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(&slot)
            && *k == key
        {
            return Ok(s.id());
        }
        let level = (0..super::render::MAX_VIEW_LEVEL)
            .find(|&l| {
                let e = canvas.at_level(l);
                e.width.max(e.height) <= max_px
            })
            .unwrap_or(super::render::MAX_VIEW_LEVEL - 1);
        let e = canvas.at_level(level);
        let r = Rect::of_extent(e);
        let m = match &node.mask_png {
            Some(png) => mask_region(png, canvas, level, r)?,
            None => vec![1.0; (e.width * e.height) as usize],
        };
        let surface = Surface::create_rgba8(e.width, e.height).map_err(failure)?;
        surface
            .with_pixels(|px, stride| {
                for y in 0..e.height as usize {
                    for x in 0..e.width as usize {
                        let v = (m[y * e.width as usize + x].clamp(0.0, 1.0) * 255.0 + 0.5) as u8;
                        let o = y * stride + x * 4;
                        px[o..o + 4].copy_from_slice(&[v, v, v, 255]);
                    }
                }
            })
            .map_err(failure)?;
        let id = surface.id();
        fs.mask_thumbs
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(slot, (key, Arc::new(surface)));
        Ok(id)
    }

    /// Filter ▸ Convert for Smart Filters: the layer becomes a smart object
    /// holding it (same id, name, opacity, blend mode and mask; the contents
    /// keep their pixels at identity transform). One history node.
    pub fn convert_for_smart_filters(&self, layer: u64) -> Result<DocumentUpdate> {
        self.clear_preview_state();
        let mut st = self.shared.lock()?;
        st.open()?;
        let before = st.live().state().clone();
        self.commit_pending(&mut st, None)?;
        let s = st.doc.state().clone();
        let l = find(&s, layer)?;
        let outer = smart_wrapper(&s, l)?; // B5-12: the shared wrapper (masks once)
        let (parent, index) = s
            .locate(LayerId(layer))
            .ok_or_else(|| failure("layer not found"))?;
        let applied = st.doc.apply(DocOp::Batch(vec![
            DocOp::RemoveLayer { id: LayerId(layer) },
            DocOp::AddLayer {
                parent,
                index,
                layer: outer,
            },
        ]))?;
        st.labels
            .insert(applied.node, "Convert to Smart Object".into());
        let mut u = self.update(&mut st, &before, Some(&applied), true);
        u.created.clear();
        Ok(u)
    }
}

// B5-13 begin: narrow adapter helpers for the Liquify workspace and
// Content-Aware Move (document/liquify.rs, document/content_aware.rs). They
// reuse this module's preview slot, checked writes and smart-filter lists;
// no filter algorithm lives here.

/// Tiles of `raster` over `region` replaced by `result` (both canvas-sized,
/// level 0), blended by `clip` coverage when given (never otherwise), alpha
/// kept when `keep_alpha`. Tiles outside `region` are not touched.
pub(crate) fn blended_tiles(
    raster: &Raster,
    result: &Raster,
    region: Rect,
    clip: Option<&Raster>,
    keep_alpha: bool,
    depth: compositor::Depth,
) -> Result<Vec<TileDelta>> {
    if raster.extent() != result.extent() || clip.is_some_and(|c| c.extent() != raster.extent()) {
        return Err(failure("result and layer sizes differ"));
    }
    let region = region.intersect(&Rect::of_extent(raster.extent()));
    if region.is_empty() {
        return Ok(Vec::new());
    }
    let ts = i64::from(TILE_SIZE);
    let mut tiles = Vec::new();
    let (mut a, mut b, mut k) = (Vec::new(), Vec::new(), Vec::new());
    for ty in region.y0 / ts..=(region.y1 - 1) / ts {
        for tx in region.x0 / ts..=(region.x1 - 1) / ts {
            let (tx32, ty32) = (tx as u32, ty as u32);
            let lay = raster.layout(tx32, ty32);
            raster.read_tile(tx32, ty32, &mut a)?;
            result.read_tile(tx32, ty32, &mut b)?;
            if let Some(c) = clip {
                c.read_tile(tx32, ty32, &mut k)?;
            }
            let n = lay.plane_len();
            let rc = result.channels() as usize;
            let mut any = false;
            for y in 0..lay.extent.height as i64 {
                let gy = ty * ts + y;
                for x in 0..lay.extent.width as i64 {
                    let gx = tx * ts + x;
                    let i = y as usize * lay.stride() + x as usize;
                    if gx >= region.x0 && gx < region.x1 && gy >= region.y0 && gy < region.y1 {
                        let m = clip.map_or(1.0, |_| k[i].clamp(0.0, 1.0));
                        for c in 0..4 {
                            let src = a[c * n + i];
                            let dst = if c < rc {
                                b[c * n + i]
                            } else if c == 3 {
                                1.0
                            } else {
                                b[i]
                            };
                            if !dst.is_finite() {
                                return Err(failure("nonfinite result"));
                            }
                            a[c * n + i] = if c == 3 && keep_alpha {
                                src
                            } else if m == 1.0 {
                                dst
                            } else {
                                src + m * (dst - src)
                            };
                        }
                    }
                    any |= a[3 * n + i] != 0.0;
                }
            }
            let tile = if any || raster.tile(tx32, ty32).is_some() {
                Some(tile_from_f32(
                    TileCoord::new(0, tx32, ty32),
                    lay,
                    depth,
                    a.clone(),
                )?)
            } else {
                None
            };
            tiles.push(TileDelta {
                tx: tx32,
                ty: ty32,
                tile,
            });
        }
    }
    Ok(tiles)
}

impl DocumentSession {
    /// Shows `raster` (canvas-sized, level 0) in place of `layer`'s content
    /// in the viewport: the same preview slot `preview_filter` fills (a newer
    /// preview or `clear_preview` replaces it). No history node. `cancel` is
    /// checked under the preview-slot lock, so a cancel that clears the slot
    /// (`clear_preview`) after setting it is never followed by this preview.
    pub(crate) fn show_layer_preview(
        &self,
        layer: u64,
        raster: Raster,
        cancel: &AtomicBool,
    ) -> Result<()> {
        {
            let mut i = self.shared.filters.q.lock();
            if cancel.load(Ordering::SeqCst) {
                return Err(failure("cancelled"));
            }
            i.generation += 1;
            i.preview_job = None;
            if let Some(c) = &i.running {
                c.cancel();
            }
            let generation = i.generation;
            i.preview = Some((generation, PreviewShown::Replace(layer, Arc::new(raster))));
            i.last_error = None;
        }
        let epoch = self.shared.lock()?.epoch;
        self.shared.render.request(Vec::new(), false, epoch);
        Ok(())
    }

    /// What a smart filter at `stage` of smart object `layer` receives: the
    /// child unplaced (its own canvas, identity transform) after smart
    /// filters `..stage`, straight RGBA f32 — the space adapter masks and
    /// Liquify meshes live in (as `remove_distractions`).
    pub(crate) fn smart_stage_source(
        base: &DocState,
        layer: u64,
        stage: usize,
    ) -> Result<(Extent, Vec<f32>)> {
        let l = find(base, layer)?;
        let LayerKind::SmartObject(so) = &l.kind else {
            return Err(failure(format!("layer {layer} is not a smart object")));
        };
        let nodes = nodes_of(so)?;
        if stage > nodes.len() {
            return Err(failure(format!("no smart filter {stage}")));
        }
        let img = native_filtered(base, l, &nodes[..stage], true, None)?;
        Ok((
            Extent::new(img.rect.width() as u32, img.rect.height() as u32),
            img.px,
        ))
    }

    /// Appends (`index` None) or re-edits smart filter `index` of adapter
    /// filter `id` with `params` (keeping its enable state and blending), as
    /// one checked history node. Re-editing never appends a duplicate.
    /// `cancel` stops the validation render and is checked again under the
    /// document lock just before the write.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn set_adapter_smart_filter(
        &self,
        layer: u64,
        label: &str,
        expected_revision: u64,
        index: Option<usize>,
        id: &str,
        params: serde_json::Value,
        cancel: &AtomicBool,
    ) -> Result<DocumentUpdate> {
        if !adapter_id(id) {
            return Err(failure(format!("{id} is not an adapter filter")));
        }
        let spec = Spec::from_value(&serde_json::json!({"id": id, "params": params}))?;
        self.set_nodes_checked_until(
            layer,
            label,
            Some(expected_revision),
            cancel,
            move |nodes| {
                match index {
                    None => nodes.push(Node::new(spec)),
                    Some(i) => {
                        let n = nodes
                            .get_mut(i)
                            .ok_or_else(|| failure(format!("no smart filter {i}")))?;
                        if n.spec.id != spec.id {
                            return Err(failure(format!(
                                "smart filter {i} is {}, not {}",
                                n.spec.id, spec.id
                            )));
                        }
                        n.spec = spec;
                    }
                }
                Ok(())
            },
        )
    }

    /// `op` as one history node, provided `layer`'s content is still
    /// `revision` and `cancel` is not set when the node would be applied
    /// (checked under the document lock, just before the write).
    pub(crate) fn edit_layer_checked(
        &self,
        layer: u64,
        revision: u64,
        op: DocOp,
        label: &str,
        cancel: &AtomicBool,
    ) -> Result<DocumentUpdate> {
        self.edit_checked_until(layer, revision, op, label, cancel)
    }
}
// B5-13 end

fn max_id(l: &Layer) -> u64 {
    l.children()
        .map_or(0, |c| c.iter().map(|x| max_id(x)).max().unwrap_or(0))
        .max(l.id.0)
}

/// Test and bench support (not exported over UniFFI).
impl DocumentSession {
    /// B5-18b: pixels the Camera Raw adapter has developed in this process
    /// (previews, detail panes, checks, bakes and applies).
    #[doc(hidden)]
    pub fn camera_raw_pixels_developed(&self) -> u64 {
        CAMERA_RAW_PIXELS.load(Ordering::Relaxed)
    }

    /// B5-15: `false` bakes every smart filter on the CPU (the route before
    /// P19), `true` restores the GPU route where it applies.
    #[doc(hidden)]
    pub fn set_gpu_smart_filters(&self, enabled: bool) {
        self.shared
            .filters
            .set_gpu(enabled && self.shared.render.backend_name() != "CPU");
        if let Ok(st) = self.shared.lock() {
            self.shared.render.request(Vec::new(), false, st.epoch);
        }
    }

    /// B5-15: `(GPU stages, CPU fallbacks, stage cache bytes)` of the
    /// resident renderer's smart-filter runtime and the CPU bakes and
    /// previews the filter worker ran; `None` without Metal.
    #[doc(hidden)]
    pub fn smart_filter_trace(&self) -> Option<SmartFilterTrace> {
        let (gpu_stages, cpu_fallbacks, stage_cache_bytes) =
            self.shared.render.smart_filter_stats()?;
        let i = self.shared.filters.q.lock();
        Some(SmartFilterTrace {
            gpu_stages,
            cpu_fallbacks,
            stage_cache_bytes,
            cpu_bakes: i.cpu_bakes,
            cpu_previews: i.cpu_previews,
        })
    }

    /// Blocks until previews and smart filter bakes are done.
    #[doc(hidden)]
    pub fn wait_filters_idle(&self) {
        self.shared.filters.wait_idle();
    }

    /// Renders `level` of the *presented* document (smart filters baked at
    /// that level, the preview shown), waiting for the worker, and reads it
    /// back as straight f32 RGBA.
    #[doc(hidden)]
    pub fn read_presented_level(&self, level: u8) -> Result<(u32, u32, Vec<f32>)> {
        // B5-15: until presenting schedules nothing and the worker is idle
        // (a bake at the view level can be followed by a level-0 refinement),
        // with a generous deadline that fails instead of reading a stale bake.
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(300);
        loop {
            let pending = {
                let st = self.shared.lock()?;
                let full = Rect::of_extent(st.live().state().canvas.at_level(level));
                presented(&self.shared, st.live(), level, full);
                let i = self.shared.filters.q.lock();
                i.busy || i.preview_job.is_some() || !i.bake_jobs.is_empty()
            };
            if !pending {
                break;
            }
            if std::time::Instant::now() > deadline {
                return Err(failure(
                    "smart filter bakes and previews did not finish within 300 s",
                ));
            }
            self.wait_filters_idle();
        }
        let st = self.shared.lock()?;
        let full = Rect::of_extent(st.live().state().canvas.at_level(level));
        let doc = presented(&self.shared, st.live(), level, full);
        let d: &Document = doc.as_deref().unwrap_or(st.live());
        self.shared.render.read_level(d, level)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_round_trips() {
        for n in 0..40 {
            let v: Vec<u8> = (0..n).map(|i| (i * 37 % 256) as u8).collect();
            assert_eq!(base64_decode(&base64_encode(&v)).unwrap(), v);
        }
        assert_eq!(base64_encode(b"Man"), "TWFu");
        assert_eq!(base64_encode(b"Ma"), "TWE=");
    }

    #[test]
    fn filter_json_parses_and_rejects() {
        let s = Spec::parse(r#"{"id":"gaussian_blur","params":{"radius":4}}"#).unwrap();
        assert_eq!(s.id, "gaussian_blur");
        assert_eq!(s.name(), "Gaussian Blur");
        assert_eq!(s.at(2).unwrap().1.radius, 1.0);
        assert!(Spec::parse(r#"{"id":"gaussian_blur","params":{"radius":-4}}"#).is_err());
        assert!(Spec::parse(r#"{"params":{}}"#).is_err());
        assert!(Spec::parse(r#"{"id":"twirl","params":{"center":[0.2,0.7]}}"#).is_ok());
        let n = Node::new(s);
        assert_eq!(Node::of(&n.store()).unwrap(), n);
    }
}

/// SOURCE-ONLY regressions: UNRUN on B. Tiny rasters, explicit rendezvous,
/// bounded channel waits; no sleeps, GPU work or image fixtures.
#[cfg(test)]
mod request_cancellation_tests {
    use super::*;
    use compositor::render::smart_filters::{FilterContext, SmartFilterEvaluator};
    use std::sync::{atomic::AtomicUsize, mpsc};
    use std::time::Duration;

    struct GatedEvaluator {
        entered: mpsc::Sender<()>,
        release: Mutex<mpsc::Receiver<()>>,
        calls: AtomicUsize,
    }

    impl SmartFilterEvaluator for GatedEvaluator {
        fn evaluate(
            &self,
            input: &Raster,
            _: &SmartFilter,
            _: &FilterContext,
        ) -> engine_api::EngineResult<Raster> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            self.entered.send(()).expect("notify first stage");
            self.release
                .lock()
                .unwrap()
                .recv_timeout(Duration::from_secs(5))
                .expect("release first stage within watchdog");
            Ok(input.clone())
        }
    }

    fn fixture() -> (DocState, Layer, Vec<Node>) {
        let extent = Extent::new(3, 2);
        let mut child = DocState::new(extent, compositor::Depth::F32);
        child.root.push(Arc::new(Layer::new(
            "fill",
            LayerKind::Fill(compositor::document::Fill::Solid {
                color: [0.2, 0.4, 0.6],
            }),
        )));
        let layer = Layer::new(
            "smart",
            LayerKind::SmartObject(SmartObject::new(child, Affine::IDENTITY)),
        );
        let mut base = DocState::new(extent, compositor::Depth::F32);
        base.root.push(Arc::new(layer.clone()));
        let node =
            Node::new(Spec::parse(r#"{"id":"gaussian_blur","params":{"radius":1}}"#).unwrap());
        (base, layer, vec![node.clone(), node])
    }

    #[test]
    fn native_request_cancel_during_first_stage_stops_next_stage_and_publication() {
        for closing in [false, true] {
            let (base, layer, nodes) = fixture();
            let cancel = Arc::new(RequestCancellation::default());
            let (entered_tx, entered_rx) = mpsc::channel();
            let (release_tx, release_rx) = mpsc::channel();
            let evaluator = Arc::new(GatedEvaluator {
                entered: entered_tx,
                release: Mutex::new(release_rx),
                calls: AtomicUsize::new(0),
            });
            let worker_cancel = cancel.clone();
            let worker_evaluator = evaluator.clone();
            let worker = std::thread::spawn(move || {
                native_stack_with_evaluator(
                    &base,
                    &layer,
                    &nodes,
                    0,
                    &worker_cancel,
                    worker_evaluator,
                )
                .is_err()
            });
            entered_rx
                .recv_timeout(Duration::from_secs(5))
                .expect("first stage entered");
            let fresh = Arc::new(RequestCancellation::default());
            let mut inner = Inner {
                generation: 1,
                running: Some(cancel.clone()),
                ..Default::default()
            };
            if closing {
                inner.stop = true;
            }
            inner.cancel_active();
            if !closing {
                inner.generation = 2;
                inner.running = Some(fresh.clone());
            }
            release_tx.send(()).unwrap();
            assert!(worker.join().unwrap());
            assert_eq!(evaluator.calls.load(Ordering::SeqCst), 1);
            assert!(cancel.effect.load(Ordering::Acquire));
            assert!(!inner.finish_preview(1, &cancel));
            assert!(inner.preview.is_none());
            assert!(inner.last_error.is_none());
            if !closing {
                assert!(inner.finish_preview(2, &fresh));
            }
            assert!(!fresh.is_cancelled());
        }
    }

    #[test]
    fn native_precancel_skips_work_and_fresh_request_survives() {
        let (base, layer, nodes) = fixture();
        let cancelled = Arc::new(RequestCancellation::default());
        cancelled.cancel();
        assert!(native_stack(&base, &layer, &nodes, 0, Some(&cancelled)).is_err());
        let fresh = Arc::new(RequestCancellation::default());
        assert!(!fresh.is_cancelled());
        assert!(native_stack(&base, &layer, &nodes, 0, Some(&fresh)).is_ok());
        assert!(native_filtered(&base, &layer, &nodes, false, Some(&cancelled)).is_err());
        assert!(native_filtered(&base, &layer, &nodes, false, Some(&fresh)).is_ok());
    }

    #[test]
    fn preview_old_completion_cannot_clear_fresh_owner() {
        let old = Arc::new(RequestCancellation::default());
        let fresh = Arc::new(RequestCancellation::default());
        let mut inner = Inner {
            generation: 1,
            running: Some(old.clone()),
            ..Default::default()
        };
        inner.cancel_active();
        inner.generation = 2;
        inner.running = Some(fresh.clone());
        assert!(!inner.finish_preview(1, &old));
        assert!(Arc::ptr_eq(inner.running.as_ref().unwrap(), &fresh));
        assert!(inner.finish_preview(2, &fresh));
        assert!(!fresh.is_cancelled());
    }

    fn bake_job(key: &str) -> BakeJob {
        BakeJob {
            key: key.into(),
            base: Arc::new(DocState::new(Extent::new(3, 2), compositor::Depth::F32)),
            layer: 1,
            level: 0,
            region: None,
        }
    }

    fn active(job: &BakeJob, cancel: Arc<RequestCancellation>) -> ActiveBake {
        ActiveBake {
            layer: job.layer,
            key: job.key.clone(),
            level: job.level,
            region: job.region,
            cancel,
        }
    }

    #[test]
    fn bake_supersession_preserves_new_owner_and_rejects_stale_key() {
        let old_job = bake_job("old");
        let fresh_job = bake_job("new");
        let old = Arc::new(RequestCancellation::default());
        let fresh = Arc::new(RequestCancellation::default());
        let mut inner = Inner {
            running_bake: Some(active(&old_job, old.clone())),
            ..Default::default()
        };
        inner.cancel_active();
        inner.running_bake = Some(active(&fresh_job, fresh.clone()));
        assert!(!inner.finish_bake(&old_job, &old));
        assert!(Arc::ptr_eq(
            &inner.running_bake.as_ref().unwrap().cancel,
            &fresh
        ));
        assert!(inner.finish_bake(&fresh_job, &fresh));
        assert!(!fresh.is_cancelled());
    }

    #[test]
    fn shutdown_cancels_both_request_channels_and_rejects_completion() {
        let preview = Arc::new(RequestCancellation::default());
        let bake = Arc::new(RequestCancellation::default());
        let job = bake_job("current");
        let mut inner = Inner {
            generation: 1,
            running: Some(preview.clone()),
            running_bake: Some(active(&job, bake.clone())),
            ..Default::default()
        };
        inner.stop = true;
        inner.cancel_active();
        for source in [&preview, &bake] {
            assert!(source.native.is_cancelled());
            assert!(source.effect.load(Ordering::Acquire));
        }
        assert!(!inner.finish_preview(1, &preview));
        assert!(!inner.finish_bake(&job, &bake));
        assert!(inner.last_error.is_none());
    }
}

/// SOURCE-ONLY cache policy regressions: UNRUN on B; budgets are bytes, not MB.
#[cfg(test)]
mod image_cache_tests {
    use super::*;
    use std::cell::Cell;

    fn image(pixels: usize) -> Arc<Img> {
        Arc::new(Img {
            rect: Rect::new(0, 0, pixels as i64, 1),
            px: vec![0.25; pixels * 4],
        })
    }

    fn queue(budget: usize) -> Queue {
        Queue {
            m: Mutex::new(Inner {
                imgs: ImgCache::new(budget),
                ..Default::default()
            }),
            cv: Condvar::new(),
        }
    }

    #[test]
    fn byte_budget_evicts_oldest_without_mutating_current_result() {
        let q = queue(64);
        let first = image(4);
        let second = image(4);
        store_img(&q, "first".into(), first.clone());
        assert!(Arc::ptr_eq(&cached_img(&q, "first").unwrap(), &first));
        store_img(&q, "second".into(), second.clone());
        assert!(cached_img(&q, "first").is_none());
        assert!(Arc::ptr_eq(&cached_img(&q, "second").unwrap(), &second));
        assert_eq!(q.lock().imgs.retained_bytes, 64);
        assert_eq!(first.px, vec![0.25; 16]);
    }

    #[test]
    fn replacement_is_charged_once_and_oversize_removes_old_key() {
        let q = queue(64);
        store_img(&q, "same".into(), image(4));
        store_img(&q, "same".into(), image(2));
        assert_eq!(q.lock().imgs.retained_bytes, 32);
        assert_eq!(q.lock().imgs.entries.len(), 1);
        let oversized = image(5);
        store_img(&q, "same".into(), oversized.clone());
        assert!(cached_img(&q, "same").is_none());
        assert_eq!(q.lock().imgs.retained_bytes, 0);
        assert_eq!(oversized.px, vec![0.25; 20]);
    }

    #[test]
    fn capacity_not_length_is_charged_and_overflow_is_rejected() {
        let q = queue(64);
        let mut px = Vec::with_capacity(20);
        px.extend([0.25; 4]);
        let img = Arc::new(Img {
            rect: Rect::new(0, 0, 1, 1),
            px,
        });
        store_img(&q, "spare-capacity".into(), img);
        assert!(cached_img(&q, "spare-capacity").is_none());
        assert_eq!(q.lock().imgs.retained_bytes, 0);
        assert!(ImgCache::capacity_bytes(usize::MAX).is_none());
    }

    #[test]
    fn entry_cap_and_zero_budget_both_apply() {
        let q = queue(1024);
        for n in 0..5 {
            store_img(&q, n.to_string(), image(1));
        }
        assert_eq!(q.lock().imgs.entries.len(), 4);
        assert_eq!(q.lock().imgs.retained_bytes, 64);
        assert!(cached_img(&q, "0").is_none());
        let disabled = queue(0);
        store_img(&disabled, "empty".into(), image(0));
        store_img(&disabled, "nonempty".into(), image(1));
        assert!(disabled.lock().imgs.entries.is_empty());
    }

    #[test]
    fn oversized_prefix_skips_deep_copy_and_keeps_pixels() {
        for budget in [0, 64] {
            let q = queue(budget);
            let img = image(5);
            let copies = Cell::new(0);
            store_prefix_with(&q, "prefix".into(), &img, |value| {
                copies.set(copies.get() + 1);
                value.clone()
            });
            assert_eq!(copies.get(), 0);
            assert!(cached_img(&q, "prefix").is_none());
            assert_eq!(img.px, vec![0.25; 20]);
        }
    }

    #[test]
    fn prefix_copy_is_unlocked_and_insertion_rechecks_policy() {
        let q = queue(64);
        let img = image(4);
        let copies = Cell::new(0);
        store_prefix_with(&q, "prefix".into(), &img, |value| {
            copies.set(copies.get() + 1);
            q.m.try_lock()
                .expect("cache lock must not span image copy")
                .imgs
                .budget = 0;
            value.clone()
        });
        assert_eq!(copies.get(), 1);
        assert!(cached_img(&q, "prefix").is_none());
        assert_eq!(q.lock().imgs.retained_bytes, 0);
    }

    #[test]
    fn source_and_prefix_cache_policy_preserves_tiny_filter_pixels() {
        let extent = Extent::new(2, 2);
        let raster = raster_from_rgba(
            extent,
            compositor::Depth::F32,
            &[0.2, 0.4, 0.6, 1.0].repeat(4),
            false,
        )
        .unwrap();
        let layer = Layer::new("pixels", LayerKind::Pixel(raster));
        let mut base = DocState::new(extent, compositor::Depth::F32);
        base.root.push(Arc::new(layer.clone()));
        let comp = Compositor::new(1 << 20);
        let region = Rect::of_extent(extent);
        let retained = queue(64);
        let disabled = queue(0);
        let first = source(&retained, &comp, &base, &layer, 0, region).unwrap();
        let hit = source(&retained, &comp, &base, &layer, 0, region).unwrap();
        assert!(Arc::ptr_eq(&first, &hit));
        let node =
            Node::new(Spec::parse(r#"{"id":"gaussian_blur","params":{"radius":1}}"#).unwrap());
        let nodes = [node.clone(), node];
        let cancel = AtomicBool::new(false);
        let expected =
            filtered(&disabled, &comp, &base, &layer, &nodes, 0, region, &cancel).unwrap();
        let actual = filtered(&retained, &comp, &base, &layer, &nodes, 0, region, &cancel).unwrap();
        let again = filtered(&retained, &comp, &base, &layer, &nodes, 0, region, &cancel).unwrap();
        assert_eq!(actual.px, expected.px);
        assert_eq!(again.px, expected.px);
        let cache = retained.lock();
        assert!(cache.imgs.retained_bytes <= 64);
        assert!(
            cache
                .imgs
                .entries
                .iter()
                .any(|entry| entry.key.starts_with("stack:"))
        );
        assert!(disabled.lock().imgs.entries.is_empty());
    }
}

/// Source-only candidate: UNRUN on B; A owns bounded compilation and execution.
#[cfg(test)]
mod legacy_effect_cancellation_tests {
    use super::*;
    use std::cell::Cell;
    use std::time::Duration;

    fn tiny() -> Img {
        Img {
            rect: Rect::of_extent(Extent::new(2, 2)),
            px: vec![0.25; 16],
        }
    }

    #[test]
    fn tiny_gaussian_keeps_typed_precancel_and_fresh_success() {
        let image = tiny();
        let cancelled = AtomicBool::new(true);
        assert!(matches!(
            run_effect(
                Effect::Gaussian,
                &FilterParams::default(),
                &image,
                &cancelled
            ),
            Err(FilterEvalError::Cancelled)
        ));
        let fresh = AtomicBool::new(false);
        assert!(run_effect(Effect::Gaussian, &FilterParams::default(), &image, &fresh).is_ok());
        let empty = Img {
            rect: Rect::new(0, 0, 0, 0),
            px: Vec::new(),
        };
        assert!(run_effect(Effect::Gaussian, &FilterParams::default(), &empty, &fresh).is_ok());
        assert!(matches!(
            run_effect(
                Effect::Gaussian,
                &FilterParams::default(),
                &empty,
                &cancelled
            ),
            Err(FilterEvalError::Cancelled)
        ));
    }

    #[test]
    fn native_evaluator_receives_typed_legacy_effect_cancellation() {
        let cancel = Arc::new(RequestCancellation::default());
        // Deliberately exercise the effect path, not the native entry checkpoint.
        cancel.effect.store(true, Ordering::Release);
        let evaluator = NativeFilterEvaluator { cancel };
        let input = Raster::new(Extent::new(2, 2), 4, compositor::Depth::F32, 0.25);
        let filter =
            Node::new(Spec::parse(r#"{"id":"gaussian_blur","params":{"radius":1}}"#).unwrap())
                .store();
        let context = compositor::render::smart_filters::FilterContext {
            profile: None,
            level: 0,
            canvas: input.extent(),
        };
        use compositor::render::smart_filters::SmartFilterEvaluator;
        assert!(matches!(
            evaluator.evaluate(&input, &filter, &context),
            Err(engine_api::EngineError::Cancelled)
        ));
    }

    #[test]
    fn collector_drains_and_real_failure_wins_both_arrival_orders() {
        for cancel_first in [false, true] {
            let cancellation = Err(FilterEvalError::Cancelled);
            let genuine = Err(FilterEvalError::from(failure("genuine")));
            let mut results = if cancel_first {
                vec![cancellation, genuine]
            } else {
                vec![genuine, cancellation]
            };
            results.push(Ok(7));
            results.push(Err(FilterEvalError::from(failure("later"))));
            let seen = Cell::new(0);
            let accepted = Cell::new(0);
            let result = collect_effect_results(
                results.into_iter().inspect(|_| seen.set(seen.get() + 1)),
                |_| accepted.set(accepted.get() + 1),
            );
            assert_eq!(seen.get(), 4, "must drain after either error");
            assert_eq!(accepted.get(), 0, "no copying after an error");
            match result {
                Err(FilterEvalError::Failed(error)) => assert_eq!(error.to_string(), "genuine"),
                _ => panic!("first real error must win"),
            }
        }
        let mut scratch = Vec::new();
        let result =
            collect_effect_results([Ok(1), Err(FilterEvalError::Cancelled), Ok(2)], |value| {
                scratch.push(value)
            });
        assert_eq!(scratch, [1]); // Private scratch only; Err prevents image return/publication.
        assert!(matches!(result, Err(FilterEvalError::Cancelled)));
    }

    #[test]
    fn gated_real_engine_error_is_not_reclassified_after_cancel() {
        let cancel = Arc::new(RequestCancellation::default());
        let (entered_tx, entered_rx) = std::sync::mpsc::channel();
        let (release_tx, release_rx) = std::sync::mpsc::channel();
        let request = cancel.clone();
        let worker = std::thread::spawn(move || {
            let error = engine_api::EngineError::invalid("effect test", "genuine");
            entered_tx.send(()).unwrap();
            release_rx.recv_timeout(Duration::from_secs(5)).unwrap();
            assert!(request.is_cancelled());
            let result = collect_effect_results::<()>(
                [
                    Err(FilterEvalError::from(error)),
                    effect_checkpoint(&request.effect),
                ],
                |_| {},
            );
            result.map_err(FilterEvalError::into_engine)
        });
        entered_rx.recv_timeout(Duration::from_secs(5)).unwrap();
        cancel.cancel();
        release_tx.send(()).unwrap();
        let result = worker.join().unwrap();
        assert!(result.is_err());
        assert!(!matches!(result, Err(engine_api::EngineError::Cancelled)));
    }

    #[test]
    fn malformed_mask_failure_survives_later_cancel() {
        let mut node =
            Node::new(Spec::parse(r#"{"id":"gaussian_blur","params":{"radius":1}}"#).unwrap());
        node.mask_png = Some("not-base64".into());
        let cancel = AtomicBool::new(false);
        let result = eval_stack(tiny(), &[node], 0, Extent::new(2, 2), None, &cancel);
        assert!(matches!(result, Err(FilterEvalError::Failed(_))));
        cancel.store(true, Ordering::Release);
        assert!(matches!(result.map_err(FilterEvalError::into_engine),
            Err(error) if !matches!(error, engine_api::EngineError::Cancelled)));
    }

    #[test]
    fn compatibility_preserves_failures_without_string_or_flag_inference() {
        let error = FilterEvalError::from(failure("cancelled"));
        assert!(matches!(error, FilterEvalError::Failed(_)));
        assert_eq!(error.into_bridge().to_string(), "cancelled");
        assert!(matches!(
            FilterEvalError::from(engine_api::EngineError::Cancelled).into_engine(),
            engine_api::EngineError::Cancelled
        ));
        assert_eq!(
            FilterEvalError::Cancelled.into_bridge().to_string(),
            "cancelled"
        );
        let cancel = AtomicBool::new(false);
        let invalid = FilterParams {
            radius: f32::NAN,
            ..Default::default()
        };
        let result = run_effect(Effect::Gaussian, &invalid, &tiny(), &cancel);
        assert!(matches!(result, Err(FilterEvalError::Failed(_))));
        // Setting the flag after a genuine result cannot change its variant.
        cancel.store(true, Ordering::Release);
        assert!(matches!(result, Err(FilterEvalError::Failed(_))));
    }
}
