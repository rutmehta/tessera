//! Pure advanced-selection computation. Callers commit the returned F32
//! mask as one history operation; computation never mutates a document.
use compositor::{Depth, Document, Raster};
use engine_api::{EngineError, EngineResult};
use schemars::JsonSchema;
use selection::{Image, Mask};
use serde::{Deserialize, Serialize};

/// Combination of the current selection and a computed mask.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum BooleanOperation {
    /// Replace the current mask.
    #[default]
    Replace,
    /// Per-pixel maximum.
    Add,
    /// Minimum of current and inverted operand.
    Subtract,
    /// Per-pixel minimum.
    Intersect,
    /// Absolute difference.
    Xor,
}

/// Image-driven selection request. Coordinates are canvas pixels.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SelectAdvanced {
    /// Open-document identifier, resolved by the executor.
    pub document: u64,
    /// Selection algorithm and its options.
    pub operation: AdvancedOperation,
    /// How to combine with the current selection (absent means all selected).
    #[serde(default)]
    pub mode: BooleanOperation,
    /// Gaussian feather radius (2 sigma), pixels, 0..=4096.
    #[serde(default)]
    pub feather: f32,
}

/// Supported image-driven algorithms.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum AdvancedOperation {
    /// Magic-wand RGBA tolerance flood fill, on the visible composite.
    Wand {
        /// Seed [x, y].
        seed: [u32; 2],
        /// Per-channel tolerance in 8-bit levels (0..=255).
        tolerance: f32,
        /// Restrict to the 4-connected region.
        #[serde(default = "yes")]
        contiguous: bool,
        /// Soften boundaries by one pixel.
        #[serde(default = "yes")]
        antialias: bool,
        /// Odd seed averaging window, 1..=255.
        #[serde(default = "one")]
        sample_size: u32,
    },
    /// Seeded colour/texture region growth from a brush polyline.
    Quick {
        /// Brush centres in canvas pixels.
        stroke: Vec<[f32; 2]>,
        /// Brush radius in pixels, 0.5..=4096.
        radius: f32,
        /// Growth threshold in seed standard deviations, 0..=100.
        #[serde(default = "threshold")]
        threshold: f32,
        /// Texture feature weight, 0..=100.
        #[serde(default = "weight")]
        texture_weight: f32,
        /// Morphological closing radius, 0..=4096 pixels.
        #[serde(default)]
        close: f32,
    },
    /// CIE Lab colour-distance selection.
    #[serde(alias = "color_range")]
    ColourRange {
        /// Display-referred sRGB samples, each component in 0..=1.
        samples: Vec<[f32; 3]>,
        /// Fuzziness in Delta E*ab, 0..=400.
        fuzziness: f32,
    },
    /// Promptable object segmentation; requires an installed model provider.
    Object {
        /// Object prompt.
        prompt: ObjectPrompt,
        /// Guided-filter radius, 0..=4096 pixels.
        #[serde(default)]
        refine_radius: u32,
    },
    /// Salient-subject segmentation; requires a model provider.
    Subject {
        /// Guided-filter radius, 0..=4096 pixels.
        #[serde(default)]
        refine_radius: u32,
    },
    /// Sky segmentation; requires a model provider.
    Sky {
        /// Guided-filter radius, 0..=4096 pixels.
        #[serde(default)]
        refine_radius: u32,
    },
}

/// Serializable prompts for the selection crate's segmentation API.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ObjectPrompt {
    /// Ordered [left, top, right, bottom] bounds.
    Box {
        /// Pixel bounds.
        bounds: [f32; 4],
    },
    /// Positive/negative clicks.
    Points {
        /// At least one positive click.
        points: Vec<SelectionClick>,
    },
    /// Closed polygon around an object.
    Lasso {
        /// At least three finite vertices.
        points: Vec<[f32; 2]>,
    },
}

/// One labelled segmentation click.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SelectionClick {
    /// Canvas coordinate.
    pub point: [f32; 2],
    /// Foreground when true, background when false.
    pub positive: bool,
}

/// Refine the current selection against the visible document composite.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RefineSelection {
    /// Open-document identifier.
    pub document: u64,
    /// Edge detection band, pixels (0..=4096).
    #[serde(default)]
    pub radius: f32,
    /// Adapt the band to local edge sharpness.
    #[serde(default)]
    pub smart_radius: bool,
    /// Outline smoothing sigma, pixels (0..=4096).
    #[serde(default)]
    pub smooth: f32,
    /// Feather sigma, pixels (0..=4096).
    #[serde(default)]
    pub feather: f32,
    /// Edge contrast (0..=1).
    #[serde(default)]
    pub contrast: f32,
    /// Edge shift, pixels (-4096..=4096); positive grows.
    #[serde(default)]
    pub shift_edge: f32,
    /// Positive guided-filter regularization (at most 1).
    #[serde(default = "epsilon")]
    pub epsilon: f32,
}

/// Combine the current mask with a saved selection. The executor resolves
/// `selection` in the same document and passes its raster to `compute_boolean`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SelectionBoolean {
    /// Open-document identifier.
    pub document: u64,
    /// Boolean operation.
    pub operation: BooleanOperation,
    /// Saved-selection identifier in this document.
    pub selection: u64,
}

fn epsilon() -> f32 {
    1e-3
}
fn yes() -> bool {
    true
}
fn one() -> u32 {
    1
}
fn threshold() -> f32 {
    4.0
}
fn weight() -> f32 {
    1.0
}

/// Compute a selection against the document's visible composite.
/// ML operations fail explicitly unless `compute_with_model` is used.
pub fn compute(
    doc: &Document,
    current: Option<&Raster>,
    params: &SelectAdvanced,
) -> EngineResult<Raster> {
    compute_with_model(doc, current, params, None)
}

/// Compute with an explicitly supplied segmentation provider. The caller owns
/// model loading; this adapter never substitutes a heuristic for an ML result.
pub fn compute_with_model(
    doc: &Document,
    current: Option<&Raster>,
    params: &SelectAdvanced,
    model: Option<&mut dyn selection::ml::SegmentModel>,
) -> EngineResult<Raster> {
    use selection::wand::{WandOptions, magic_wand};
    validate_canvas(doc.state().canvas)?;
    bounded("feather", params.feather, 0.0, 4096.0)?;
    let previous = current_mask(current, doc.state().canvas)?;
    let mask = match &params.operation {
        AdvancedOperation::Wand {
            seed,
            tolerance,
            contiguous,
            antialias,
            sample_size,
        } => {
            bounded("tolerance", *tolerance, 0.0, 255.0)?;
            if seed[0] >= doc.state().canvas.width || seed[1] >= doc.state().canvas.height {
                return Err(EngineError::invalid("seed", "must be inside the canvas"));
            }
            if *sample_size == 0 || *sample_size > 255 || sample_size % 2 == 0 {
                return Err(EngineError::invalid(
                    "sample_size",
                    "must be odd and in 1..=255",
                ));
            }
            magic_wand(
                &composite(doc)?,
                (seed[0], seed[1]),
                &WandOptions {
                    tolerance: *tolerance,
                    contiguous: *contiguous,
                    antialias: *antialias,
                    sample_size: *sample_size,
                },
            )
        }
        AdvancedOperation::Quick {
            stroke,
            radius,
            threshold,
            texture_weight,
            close,
        } => {
            validate_points("stroke", stroke, 1, doc.state().canvas)?;
            bounded("radius", *radius, 0.5, 4096.0)?;
            bounded("threshold", *threshold, 0.0, 100.0)?;
            bounded("texture_weight", *texture_weight, 0.0, 100.0)?;
            bounded("close", *close, 0.0, 4096.0)?;
            selection::quick::quick_select(
                &composite(doc)?,
                stroke,
                &selection::quick::QuickOptions {
                    radius: *radius,
                    threshold: *threshold,
                    texture_weight: *texture_weight,
                    close: *close,
                },
                None,
                false,
            )
        }
        AdvancedOperation::ColourRange { samples, fuzziness } => {
            if samples.is_empty() || samples.len() > 4096 {
                return Err(EngineError::invalid("samples", "requires 1..=4096 colours"));
            }
            for &v in samples.iter().flatten() {
                bounded("samples", v, 0.0, 1.0)?;
            }
            bounded("fuzziness", *fuzziness, 0.0, 400.0)?;
            selection::range::color_range(&composite(doc)?, samples, *fuzziness)
        }
        AdvancedOperation::Subject { refine_radius }
        | AdvancedOperation::Sky { refine_radius }
        | AdvancedOperation::Object { refine_radius, .. } => {
            if *refine_radius > 4096 {
                return Err(EngineError::invalid("refine_radius", "must be in 0..=4096"));
            }
            let prompt = match &params.operation {
                AdvancedOperation::Object { prompt, .. } => {
                    Some(model_prompt(prompt, doc.state().canvas)?)
                }
                _ => None,
            };
            let model = model.ok_or_else(|| EngineError::invalid("model", "object/subject/sky selection requires a configured segmentation model provider"))?;
            let img = composite(doc)?;
            match &params.operation {
                AdvancedOperation::Subject { .. } => {
                    selection::ml::select_subject(model, &img, *refine_radius as usize)?
                }
                AdvancedOperation::Sky { .. } => {
                    selection::ml::select_sky(model, &img, *refine_radius as usize)?
                }
                _ => selection::ml::select_object(
                    model,
                    &img,
                    prompt.as_ref().expect("object prompt validated"),
                    *refine_radius as usize,
                )?,
            }
        }
    };
    finish(&previous, &mask, params.mode, params.feather)
}

/// Refine a real current selection. No selection is an error rather than
/// silently thresholding the implicit all-selected mask.
pub fn compute_refine(
    doc: &Document,
    current: Option<&Raster>,
    params: &RefineSelection,
) -> EngineResult<Raster> {
    validate_canvas(doc.state().canvas)?;
    let current = current
        .ok_or_else(|| EngineError::invalid("selection", "refine requires an active selection"))?;
    for (name, value) in [
        ("radius", params.radius),
        ("smooth", params.smooth),
        ("feather", params.feather),
    ] {
        bounded(name, value, 0.0, 4096.0)?;
    }
    bounded("contrast", params.contrast, 0.0, 1.0)?;
    bounded("shift_edge", params.shift_edge, -4096.0, 4096.0)?;
    bounded("epsilon", params.epsilon, f32::MIN_POSITIVE, 1.0)?;
    let mask = current_mask(Some(current), doc.state().canvas)?;
    let p = selection::refine::RefineParams {
        radius: params.radius,
        smart_radius: params.smart_radius,
        smooth: params.smooth,
        feather: params.feather,
        contrast: params.contrast,
        shift_edge: params.shift_edge,
        epsilon: params.epsilon,
    };
    selection::refine::refine_edge(&mask, &composite(doc)?, &p)?.to_raster(Depth::F32)
}

/// Combine with a resolved saved-selection raster. `None` current means all
/// selected, consistent with compositor and selection crate semantics.
pub fn compute_boolean(
    doc: &Document,
    current: Option<&Raster>,
    operand: &Raster,
    params: &SelectionBoolean,
) -> EngineResult<Raster> {
    validate_canvas(doc.state().canvas)?;
    let a = current_mask(current, doc.state().canvas)?;
    let b = current_mask(Some(operand), doc.state().canvas)?;
    finish(&a, &b, params.operation, 0.0)
}

fn model_prompt(
    p: &ObjectPrompt,
    e: engine_api::tile::Extent,
) -> EngineResult<selection::ml::ObjectPrompt> {
    Ok(match p {
        ObjectPrompt::Box { bounds: b } => {
            validate_points("bounds", &[[b[0], b[1]], [b[2], b[3]]], 2, e)?;
            if b[0] >= b[2] || b[1] >= b[3] {
                return Err(EngineError::invalid(
                    "bounds",
                    "must have positive width and height",
                ));
            }
            selection::ml::ObjectPrompt::Box(*b)
        }
        ObjectPrompt::Points { points } => {
            let ps: Vec<_> = points.iter().map(|p| p.point).collect();
            validate_points("points", &ps, 1, e)?;
            if !points.iter().any(|p| p.positive) {
                return Err(EngineError::invalid(
                    "points",
                    "requires a positive foreground click",
                ));
            }
            selection::ml::ObjectPrompt::Points(
                points.iter().map(|p| (p.point, p.positive)).collect(),
            )
        }
        ObjectPrompt::Lasso { points } => {
            validate_points("points", points, 3, e)?;
            selection::ml::ObjectPrompt::Lasso(points.clone())
        }
    })
}

fn validate_points(
    field: &str,
    points: &[[f32; 2]],
    min: usize,
    e: engine_api::tile::Extent,
) -> EngineResult<()> {
    if points.len() < min
        || points.len() > 100_000
        || points.iter().any(|p| {
            !p[0].is_finite()
                || !p[1].is_finite()
                || p[0] < 0.0
                || p[1] < 0.0
                || p[0] > e.width as f32
                || p[1] > e.height as f32
        })
    {
        return Err(EngineError::invalid(
            field,
            format!("requires {min}..=100000 finite points within the canvas"),
        ));
    }
    Ok(())
}

fn bounded(field: &str, value: f32, min: f32, max: f32) -> EngineResult<()> {
    if !value.is_finite() || value < min || value > max {
        return Err(EngineError::invalid(
            field,
            format!("must be finite and in {min}..={max}"),
        ));
    }
    Ok(())
}

fn validate_canvas(e: engine_api::tile::Extent) -> EngineResult<()> {
    if e.width == 0 || e.height == 0 {
        return Err(EngineError::invalid("canvas", "must be non-empty"));
    }
    Ok(())
}

fn current_mask(current: Option<&Raster>, extent: engine_api::tile::Extent) -> EngineResult<Mask> {
    if let Some(r) = current
        && (r.channels() != 1 || r.extent() != extent)
    {
        return Err(EngineError::invalid(
            "selection",
            "must be a canvas-sized single-channel mask",
        ));
    }
    let m = selection::api::current_mask(current, extent)?;
    if m.data()
        .iter()
        .any(|v| !v.is_finite() || !(0.0..=1.0).contains(v))
    {
        return Err(EngineError::invalid(
            "selection",
            "mask values must be finite and in 0..=1",
        ));
    }
    Ok(m)
}

fn finish(
    current: &Mask,
    new: &Mask,
    mode: BooleanOperation,
    feather: f32,
) -> EngineResult<Raster> {
    if new
        .data()
        .iter()
        .any(|v| !v.is_finite() || !(0.0..=1.0).contains(v))
    {
        return Err(EngineError::invalid(
            "selection",
            "computed mask values must be finite and in 0..=1",
        ));
    }
    let op = match mode {
        BooleanOperation::Replace => selection::Combine::Replace,
        BooleanOperation::Add => selection::Combine::Add,
        BooleanOperation::Subtract => selection::Combine::Subtract,
        BooleanOperation::Intersect => selection::Combine::Intersect,
        BooleanOperation::Xor => selection::Combine::Xor,
    };
    selection::ops::combine(current, &selection::ops::feather(new, feather), op)?
        .to_raster(Depth::F32)
}

fn composite(doc: &Document) -> EngineResult<Image> {
    let (e, rgba) = compositor::Compositor::new(64 << 20).render_level_rgba(doc, 0)?;
    let mut data: Vec<[f32; 4]> = rgba
        .as_chunks::<4>()
        .0
        .iter()
        .map(|p| [p[0], p[1], p[2], p[3]])
        .collect();
    if let Some(profile) = &doc.state().profile {
        let icc = profile.icc.as_ref().ok_or_else(|| {
            EngineError::invalid("profile", "tagged document has no embedded ICC bytes")
        })?;
        let mut registry = color_mgmt::Registry::new();
        let err = |e: color_mgmt::Error| {
            EngineError::invalid("profile", format!("selection colour conversion: {e}"))
        };
        let source = registry.load_bytes(icc).map_err(err)?;
        let srgb = registry.builtin(color_mgmt::Builtin::Srgb).map_err(err)?;
        let transform =
            color_mgmt::Transform::new(&source, &srgb, color_mgmt::TransformOptions::default())
                .map_err(err)?;
        for p in &mut data {
            let rgb = transform.apply([p[0], p[1], p[2]]);
            p[..3].copy_from_slice(&rgb);
        }
    }
    for p in &mut data {
        if p.iter().any(|v| !v.is_finite()) {
            return Err(EngineError::invalid(
                "image",
                "composite contains non-finite values",
            ));
        }
        // Selection tools operate in normalized display-referred sRGB, not HDR.
        for v in p {
            *v = v.clamp(0.0, 1.0);
        }
    }
    Ok(Image {
        width: e.width,
        height: e.height,
        data,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use compositor::{DocOp, DocState, Layer, Rect};
    use engine_api::tile::Extent;

    fn document() -> Document {
        let e = Extent::new(9, 5);
        let mut raster = Raster::new(e, 4, Depth::F32, 0.0);
        raster
            .edit_region(Rect::of_extent(e), 0, |x, _, p| {
                *p = if !(3..=5).contains(&x) {
                    [1.0, 0.0, 0.0, 1.0]
                } else {
                    [0.0, 0.0, 1.0, 1.0]
                };
            })
            .unwrap();
        let mut doc = Document::new(DocState::new(e, Depth::F32));
        doc.apply(DocOp::AddLayer {
            parent: None,
            index: 0,
            layer: Layer::new("colours", compositor::LayerKind::Pixel(raster)),
        })
        .unwrap();
        doc
    }

    fn wand(contiguous: bool) -> SelectAdvanced {
        serde_json::from_value(serde_json::json!({"document":1, "operation":{"kind":"wand", "seed":[0,2], "tolerance":0, "contiguous":contiguous, "antialias":false}})).unwrap()
    }

    #[test]
    fn schema_roundtrip_and_invalid_parameters() {
        for schema in [
            schemars::schema_for!(SelectAdvanced),
            schemars::schema_for!(RefineSelection),
            schemars::schema_for!(SelectionBoolean),
        ] {
            assert!(serde_json::to_value(schema).unwrap().is_object());
        }
        let doc = document();
        let p = wand(true);
        let roundtrip: SelectAdvanced =
            serde_json::from_value(serde_json::to_value(&p).unwrap()).unwrap();
        assert_eq!(
            compute(&doc, None, &p).unwrap().pixel(1, 2),
            compute(&doc, None, &roundtrip).unwrap().pixel(1, 2)
        );
        for operation in [
            AdvancedOperation::Wand {
                seed: [9, 0],
                tolerance: 0.0,
                contiguous: true,
                antialias: false,
                sample_size: 1,
            },
            AdvancedOperation::Wand {
                seed: [0, 0],
                tolerance: f32::NAN,
                contiguous: true,
                antialias: false,
                sample_size: 1,
            },
            AdvancedOperation::Wand {
                seed: [0, 0],
                tolerance: 0.0,
                contiguous: true,
                antialias: false,
                sample_size: 2,
            },
            AdvancedOperation::ColourRange {
                samples: vec![],
                fuzziness: 0.0,
            },
            AdvancedOperation::ColourRange {
                samples: vec![[2.0, 0.0, 0.0]],
                fuzziness: 0.0,
            },
            AdvancedOperation::Quick {
                stroke: vec![[f32::NAN, 0.0]],
                radius: 1.0,
                threshold: 1.0,
                texture_weight: 1.0,
                close: 0.0,
            },
        ] {
            let mut bad = p.clone();
            bad.operation = operation;
            assert!(compute(&doc, None, &bad).is_err());
        }
        let mut bad = p.clone();
        bad.feather = f32::INFINITY;
        assert!(compute(&doc, None, &bad).is_err());
        let invalid = Mask::filled(9, 5, f32::NAN).to_raster(Depth::F32).unwrap();
        assert!(compute(&doc, Some(&invalid), &p).is_err());
        assert!(
            serde_json::from_value::<SelectAdvanced>(
                serde_json::json!({"document":1,"operation":{"kind":"magic"}})
            )
            .is_err()
        );
    }

    #[test]
    fn tagged_composite_is_converted_to_srgb_and_bad_profiles_fail() {
        use color_mgmt::{Builtin, Registry};
        let mut registry = Registry::new();
        let profile = registry.builtin(Builtin::LinearRec2020).unwrap();
        let mut state = DocState::new(Extent::new(2, 2), Depth::F32);
        state.profile = Some(compositor::ColorProfile::from_icc(
            "linear Rec2020",
            profile.icc_bytes().to_vec(),
        ));
        let mut doc = Document::new(state);
        let mut raster = Raster::new(Extent::new(2, 2), 4, Depth::F32, 0.0);
        raster
            .edit_region(Rect::new(0, 0, 2, 2), 0, |_, _, p| {
                *p = [0.25, 0.25, 0.25, 0.5]
            })
            .unwrap();
        doc.apply(DocOp::AddLayer {
            parent: None,
            index: 0,
            layer: Layer::new("gray", compositor::LayerKind::Pixel(raster)),
        })
        .unwrap();
        let img = composite(&doc).unwrap();
        assert!(img.data[0][0] > 0.5 && img.data[0][0] < 0.56);
        assert_eq!(img.data[0][3], 0.5);
        let mut bad = DocState::new(Extent::new(2, 2), Depth::F32);
        bad.profile = Some(compositor::ColorProfile::from_icc("bad", vec![1, 2, 3]));
        assert!(composite(&Document::new(bad)).is_err());
    }

    #[test]
    fn refine_feathers_edges_and_boolean_preserves_soft_values() {
        let doc = document();
        let initial = compute(&doc, None, &wand(true)).unwrap();
        let refine: RefineSelection =
            serde_json::from_value(serde_json::json!({"document":1,"feather":1.0})).unwrap();
        let soft = compute_refine(&doc, Some(&initial), &refine).unwrap();
        assert!(soft.pixel(3, 2)[0] > 0.0 && soft.pixel(3, 2)[0] < 1.0);
        let a = Mask::filled(9, 5, 0.7).to_raster(Depth::F32).unwrap();
        let b = Mask::filled(9, 5, 0.4).to_raster(Depth::F32).unwrap();
        for (operation, expected) in [
            (BooleanOperation::Replace, 0.4),
            (BooleanOperation::Add, 0.7),
            (BooleanOperation::Subtract, 0.6),
            (BooleanOperation::Intersect, 0.4),
            (BooleanOperation::Xor, 0.3),
        ] {
            let p = SelectionBoolean {
                document: 1,
                selection: 1,
                operation,
            };
            let actual = compute_boolean(&doc, Some(&a), &b, &p).unwrap();
            assert!((actual.pixel(0, 0)[0] - expected).abs() < 1e-6);
        }
        let p = SelectionBoolean {
            document: 1,
            selection: 1,
            operation: BooleanOperation::Subtract,
        };
        let from_all = compute_boolean(&doc, None, &b, &p).unwrap();
        assert!((from_all.pixel(0, 0)[0] - 0.6).abs() < 1e-6);
        let wrong = Mask::new(1, 1).to_raster(Depth::F32).unwrap();
        assert!(compute_boolean(&doc, Some(&a), &wrong, &p).is_err());
        assert!(compute_refine(&doc, None, &refine).is_err());
    }

    #[test]
    fn ml_requires_a_provider_and_propagates_provider_failure() {
        struct Unavailable;
        impl selection::ml::SegmentModel for Unavailable {
            fn subject(&mut self, _: &Image) -> EngineResult<Mask> {
                Err(EngineError::internal("model weights unavailable"))
            }
            fn sky(&mut self, img: &Image) -> EngineResult<Mask> {
                self.subject(img)
            }
            fn object(
                &mut self,
                img: &Image,
                _: &selection::ml::ObjectPrompt,
            ) -> EngineResult<Mask> {
                self.subject(img)
            }
        }
        let doc = document();
        for operation in [
            AdvancedOperation::Subject { refine_radius: 0 },
            AdvancedOperation::Sky { refine_radius: 0 },
            AdvancedOperation::Object {
                prompt: ObjectPrompt::Box {
                    bounds: [0.0, 0.0, 3.0, 4.0],
                },
                refine_radius: 0,
            },
        ] {
            let mut p = wand(true);
            p.operation = operation;
            assert!(format!("{:?}", compute(&doc, None, &p).unwrap_err()).contains("provider"));
            assert!(
                format!(
                    "{:?}",
                    compute_with_model(&doc, None, &p, Some(&mut Unavailable)).unwrap_err()
                )
                .contains("model weights unavailable")
            );
        }
    }

    #[test]
    fn quick_growth_and_colour_range_use_image_content() {
        let doc = document();
        let mut p = wand(true);
        p.operation = AdvancedOperation::Quick {
            stroke: vec![[1.5, 2.5]],
            radius: 0.5,
            threshold: 4.0,
            texture_weight: 0.0,
            close: 0.0,
        };
        let q = compute(&doc, None, &p).unwrap();
        assert_eq!(q.pixel(0, 2)[0], 1.0);
        assert_eq!(q.pixel(4, 2)[0], 0.0);
        assert_eq!(q.pixel(8, 2)[0], 0.0);
        p.operation = AdvancedOperation::ColourRange {
            samples: vec![[1.0, 0.0, 0.0]],
            fuzziness: 0.0,
        };
        let c = compute(&doc, None, &p).unwrap();
        assert_eq!(c.pixel(8, 2)[0], 1.0);
        assert_eq!(c.pixel(4, 2)[0], 0.0);
        p.operation = AdvancedOperation::Quick {
            stroke: vec![],
            radius: 1.0,
            threshold: 4.0,
            texture_weight: 0.0,
            close: 0.0,
        };
        assert!(compute(&doc, None, &p).is_err());
    }

    #[test]
    fn wand_selects_connected_or_global_colour_without_mutating_document() {
        let doc = document();
        let before = doc.history().current();
        let local = compute(&doc, None, &wand(true)).unwrap();
        assert_eq!(local.channels(), 1);
        assert_eq!(local.pixel(1, 2)[0], 1.0);
        assert_eq!(local.pixel(4, 2)[0], 0.0);
        assert_eq!(local.pixel(7, 2)[0], 0.0);
        let global = compute(&doc, None, &wand(false)).unwrap();
        assert_eq!(global.pixel(7, 2)[0], 1.0);
        assert_eq!(doc.history().current(), before);
    }
}
