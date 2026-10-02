//! Approximation audit: every field must be known in its structural position.
//! Exact source always survives; legacy flat groups retain their byte envelope.
use roxmltree::Node;
use std::collections::BTreeMap;
const CRS: &str = engine_api::recipe::crs::CRS_NAMESPACE;
const RDF: &str = "http://www.w3.org/1999/02/22-rdf-syntax-ns#";

enum Field<'a, 'input> {
    Scalar(String),
    Structure(Node<'a, 'input>),
}
impl Field<'_, '_> {
    fn scalar(&self) -> Option<&str> {
        match self {
            Self::Scalar(v) => Some(v),
            _ => None,
        }
    }
}
fn fields<'a, 'input>(n: Node<'a, 'input>) -> Option<BTreeMap<String, Field<'a, 'input>>> {
    let descriptions: Vec<_> = n
        .children()
        .filter(|c| c.has_tag_name((RDF, "Description")))
        .collect();
    let n = if descriptions.is_empty() {
        n
    } else {
        if descriptions.len() != 1
            || n.attributes().len() != 0
            || n.children().filter(Node::is_element).count() != 1
        {
            return None;
        }
        descriptions[0]
    };
    let mut out = BTreeMap::new();
    for a in n.attributes() {
        if a.namespace() == Some(RDF) && a.name() == "parseType" && a.value() == "Resource" {
            continue;
        }
        if a.namespace() != Some(CRS)
            || out
                .insert(a.name().into(), Field::Scalar(a.value().into()))
                .is_some()
        {
            return None;
        }
    }
    for c in n.children().filter(Node::is_element) {
        if c.tag_name().namespace() != Some(CRS) {
            return None;
        }
        let value = if c.children().any(|n| n.is_element()) || c.attributes().len() != 0 {
            Field::Structure(c)
        } else {
            Field::Scalar(c.text().unwrap_or("").trim().into())
        };
        if out.insert(c.tag_name().name().into(), value).is_some() {
            return None;
        }
    }
    Some(out)
}
fn sequence<'a, 'input>(n: Node<'a, 'input>) -> Option<Vec<Node<'a, 'input>>> {
    if n.attributes().len() != 0 {
        return None;
    }
    let children: Vec<_> = n.children().filter(Node::is_element).collect();
    if children.len() != 1
        || !children[0].has_tag_name((RDF, "Seq"))
        || children[0].attributes().len() != 0
    {
        return None;
    }
    let items: Vec<_> = children[0].children().filter(Node::is_element).collect();
    items
        .iter()
        .all(|c| c.has_tag_name((RDF, "li")))
        .then_some(items)
}
fn scalar_sequence(n: Node<'_, '_>) -> Option<()> {
    let items = sequence(n)?;
    if items.is_empty() {
        return None;
    }
    for item in items {
        if item.attributes().len() != 0
            || item.children().any(|c| c.is_element())
            || item.text().is_none()
        {
            return None;
        }
    }
    Some(())
}
fn range(n: Node<'_, '_>) -> Option<()> {
    let f = fields(n)?;
    let lum = ["LumMin", "LumMax", "LumFeather", "LumRange"]
        .iter()
        .any(|key| f.contains_key(*key));
    let depth = ["DepthMin", "DepthMax", "DepthFeather"]
        .iter()
        .any(|key| f.contains_key(*key));
    let color = ["PointModels", "AreaModels", "ColorAmount"]
        .iter()
        .any(|key| f.contains_key(*key));
    if [lum, depth, color].into_iter().filter(|v| *v).count() != 1 {
        return None;
    }
    for (name, value) in &f {
        match (name.as_str(), value) {
            ("PointModels" | "AreaModels", Field::Structure(n)) => scalar_sequence(*n)?,
            (
                "Type"
                | "Version"
                | "SampleType"
                | "LuminanceDepthSampleInfo"
                | "LumRange"
                | "LumMin"
                | "LumMax"
                | "LumFeather"
                | "DepthMin"
                | "DepthMax"
                | "DepthFeather"
                | "ColorAmount"
                | "Invert",
                Field::Scalar(_),
            ) => (),
            _ => return None,
        }
    }
    // The codec validates dispatch, bounds and all sample/dab numbers.
    Some(())
}
fn component(n: Node<'_, '_>) -> Option<()> {
    component_at_depth(n, 1)
}
fn component_at_depth(n: Node<'_, '_>, depth: usize) -> Option<()> {
    if depth > 8 {
        return None;
    }
    let f = fields(n)?;
    let kind = f.get("What")?.scalar()?;
    if !matches!(
        kind,
        "Mask/Image"
            | "Mask/People"
            | "Mask/Person"
            | "Mask/Object"
            | "Mask/Subject"
            | "Mask/Sky"
            | "Mask/Background"
            | "Mask/Gradient"
            | "Mask/Paint"
            | "Mask/CircularGradient"
            | "Mask/Group"
            | "Mask/Aggregate"
            | "Mask/Range"
            | "Mask/RangeMask"
    ) {
        return None;
    }
    for (name, value) in &f {
        match (name.as_str(), value) {
            ("Masks", Field::Structure(n)) if matches!(kind, "Mask/Group" | "Mask/Aggregate") => {
                for n in sequence(*n)? {
                    component_at_depth(n, depth + 1)?;
                }
            }
            ("CorrectionRangeMask", Field::Structure(n)) => range(*n)?,
            ("Dabs", Field::Structure(n)) if kind == "Mask/Paint" => scalar_sequence(*n)?,
            ("Radius" | "Flow" | "CenterWeight", Field::Scalar(_)) if kind == "Mask/Paint" => (),
            ("MaskID" | "MaskSyncID" | "MaskName" | "MaskVersion", Field::Scalar(_)) => (),
            ("Version", Field::Scalar(_)) if kind == "Mask/CircularGradient" => (),
            ("MaskValue" | "Midpoint" | "Roundness", Field::Scalar(v)) => {
                if !v.parse::<f64>().ok()?.is_finite() {
                    return None;
                }
            }
            (
                "MaskType"
                | "MaskSubType"
                | "MaskSubCategoryID"
                | "ReferencePoint"
                | "InputDigest"
                | "InputDigestVersion"
                | "FullMaskSize"
                | "LocalInputDigest"
                | "LocalInputDigestVersion"
                | "ModelVersion"
                | "WholeImageArea"
                | "Origin"
                | "ErrorReason"
                | "MaskDigest"
                | "Left"
                | "Top"
                | "Right"
                | "Bottom",
                Field::Scalar(_),
            ) if matches!(
                kind,
                "Mask/Image"
                    | "Mask/People"
                    | "Mask/Person"
                    | "Mask/Object"
                    | "Mask/Subject"
                    | "Mask/Sky"
                    | "Mask/Background"
            ) => {}
            ("What" | "MaskActive" | "MaskInverted" | "MaskBlendMode", Field::Scalar(_)) => (),
            ("FullX" | "FullY" | "ZeroX" | "ZeroY", Field::Scalar(_))
                if kind == "Mask/Gradient" => {}
            (
                "Left" | "Right" | "Top" | "Bottom" | "Angle" | "Feather" | "Flipped",
                Field::Scalar(_),
            ) if kind == "Mask/CircularGradient" => {}
            _ => return None,
        }
    }
    Some(())
}
fn correction(n: Node<'_, '_>) -> Option<()> {
    let inactive_overlay = fields(n)?
        .get("LocalToningSaturation")
        .and_then(Field::scalar)
        .and_then(|s| s.parse::<f64>().ok())
        == Some(0.);
    for (name, value) in fields(n)? {
        match (name.as_str(), value) {
            ("CorrectionReferenceX" | "CorrectionReferenceY", Field::Scalar(v))
                if v.parse::<f64>().ok()?.is_finite() => {}
            (
                "LocalBrightness"
                | "LocalContrast"
                | "LocalExposure"
                | "LocalClarity"
                | "LocalGrain"
                | "LocalCorrectedDepth"
                | "LocalColorVariance",
                Field::Scalar(v),
            ) if v.parse::<f64>().ok()? == 0.0 => (),
            ("LocalCurveRefineSaturation", Field::Scalar(v)) if v.parse::<f64>().ok()? == 100.0 => {
            }
            ("LocalColorVariance", Field::Structure(n))
                if sequence(n)?.iter().all(|n| {
                    n.attributes().len() == 0
                        && !n.children().any(|n| n.is_element())
                        && n.text().and_then(|s| s.trim().parse::<f64>().ok()) == Some(0.)
                }) => {}
            ("LocalPointColors", Field::Scalar(v)) if v.is_empty() => (),
            ("LocalPointColors", Field::Structure(n)) if sequence(n)?.is_empty() => (),
            ("LocalToningHue", Field::Scalar(v))
                if inactive_overlay && v.parse::<f64>().ok()?.is_finite() => {}
            ("LocalToningHue" | "LocalToningSaturation", Field::Scalar(v))
                if v.parse::<f64>().ok()? == 0.0 => {}
            ("LocalDefringe", Field::Scalar(v))
                if (0.0..=100.0).contains(&v.parse::<f64>().ok()?) => {}
            ("LocalToningHue" | "LocalToningSaturation", Field::Scalar(v))
                if v.parse::<f64>().ok()?.is_finite() => {}
            (
                "MainCurve" | "RedCurve" | "GreenCurve" | "BlueCurve" | "ExtendedMainCurve"
                | "ExtendedRedCurve" | "ExtendedGreenCurve" | "ExtendedBlueCurve"
                | "LocalPointColors",
                Field::Structure(_),
            ) => (),
            ("CorrectionMasks", Field::Structure(n)) => {
                for n in sequence(n)? {
                    component(n)?;
                }
            }
            ("CorrectionRangeMask", Field::Structure(n)) => range(n)?,
            ("What", Field::Scalar(v)) if v == "Correction" => (),
            (
                "CorrectionName"
                | "CorrectionSyncID"
                | "CorrectionID"
                | "CorrectionActive"
                | "CorrectionAmount"
                | "LocalExposure2012"
                | "LocalContrast2012"
                | "LocalHighlights2012"
                | "LocalShadows2012"
                | "LocalWhites2012"
                | "LocalBlacks2012"
                | "LocalTemperature"
                | "LocalTint"
                | "LocalHue"
                | "LocalSaturation"
                | "LocalTexture"
                | "LocalClarity2012"
                | "LocalDehaze"
                | "LocalSharpness"
                | "LocalLuminanceNoise"
                | "LocalMoire"
                | "LocalDefringe",
                Field::Scalar(_),
            ) => (),
            _ => return None,
        }
    }
    Some(())
}
pub(crate) fn audited_approximation(root: Node<'_, '_>) -> bool {
    // Do not change the bytes or allocate audit maps for previously supported
    // flat shapes (the 29c baseline), even though their geometry already maps.
    let new_shape = root.descendants().any(|n| {
        n.attribute((CRS,"What")).or_else(|| n.has_tag_name((CRS,"What")).then(|| n.text()).flatten()).is_some_and(|what| matches!(what, "Mask/Image" | "Mask/Subject" | "Mask/Sky" | "Mask/Background" | "Mask/People" | "Mask/Person" | "Mask/Object")) || [
            "MainCurve", "RedCurve", "GreenCurve", "BlueCurve", "ExtendedMainCurve", "ExtendedRedCurve", "ExtendedGreenCurve", "ExtendedBlueCurve",
            "MaskType",
            "MaskDigest",
            "Dabs",
            "Masks",
            "CorrectionRangeMask",
            "Flipped",
            "MaskID",
            "MaskSyncID",
            "MaskName",
            "MaskVersion",
            "Midpoint",
            "Roundness",
            "CorrectionID",
            "CorrectionSyncID",
            "LocalToningHue",
            "LocalToningSaturation",
        ]
        .iter()
        .any(|key| n.has_tag_name((CRS, *key)) || n.attribute((CRS, *key)).is_some())
            || n.has_tag_name((CRS,"LocalPointColors")) && sequence(n).is_some_and(|items| items.iter().any(|item| {
                item.children().any(|n|n.is_element()) || item.attributes().len()!=0 || item.text().is_some_and(|s| !s.trim().is_empty() && !s.split(',').all(|v|v.trim().parse::<f64>()==Ok(-1.)))
            }))
            || n.attribute((CRS,"LocalDefringe")).or_else(|| n.has_tag_name((CRS,"LocalDefringe")).then(||n.text()).flatten()).is_some_and(|s|s.parse::<f64>().is_ok_and(|v| v!=0.))
            // Neutral MaskValue=1 was already present in legacy flat shapes.
            // Accept it in the audit, but do not change their retained envelope
            // solely because that no-op metadata is present.
            || (n.has_tag_name((CRS, "MaskValue"))
                .then(|| n.text()).flatten()
                .or_else(|| n.attribute((CRS, "MaskValue"))))
                .is_some_and(|v| v.trim().parse::<f64>().ok() != Some(1.0))
            || n.has_tag_name((CRS, "MaskActive"))
                && matches!(n.text(), Some("False" | "false" | "0"))
            || n.attribute((CRS, "MaskActive"))
                .is_some_and(|v| matches!(v, "False" | "false" | "0"))
    });
    new_shape
        && sequence(root).is_some_and(|groups| groups.into_iter().all(|n| correction(n).is_some()))
}

/// Source promotion must not discard geometry the CPU operator cannot consume.
/// Depth resource availability remains a runtime concern, not a geometry error.
pub(crate) fn renderable(groups: &[engine_api::recipe::LocalAdjustment]) -> bool {
    use engine_api::recipe::{MaskComponent, MaskKind};
    let bounded = |v: f32, lo: f32, hi: f32| v.is_finite() && (lo..=hi).contains(&v);
    let coords = |p: &[f32; 2]| p.iter().all(|&v| bounded(v, -16., 16.));
    groups.iter().all(|g| {
        bounded(g.amount, 0., 200.)
            && g.validate_mask_tree().is_ok()
            && g.components
                .iter()
                .flat_map(MaskComponent::active_leaves)
                .all(|c| match &c.kind {
                    MaskKind::Linear { start, end } => {
                        coords(start)
                            && coords(end)
                            && (end[0] - start[0]).hypot(end[1] - start[1]) >= 1e-6
                    }
                    MaskKind::Radial {
                        center,
                        radii,
                        angle,
                        feather,
                    } => {
                        coords(center)
                            && radii.iter().all(|&v| bounded(v, 1e-6, 16.))
                            && angle.is_finite()
                            && bounded(*feather, 0., 100.)
                    }
                    MaskKind::LuminanceRange {
                        range, smoothness, ..
                    } => {
                        bounded(range[0], 0., 1.)
                            && bounded(range[1], range[0], 1.)
                            && bounded(*smoothness, 0., 100.)
                    }
                    MaskKind::Depth { range, feather, .. } => {
                        bounded(range[0], 0., 1.)
                            && bounded(range[1], range[0], 1.)
                            && bounded(*feather, 0., 100.)
                    }
                    MaskKind::Brush { strokes } => {
                        !strokes.is_empty()
                            && strokes.iter().all(|s| {
                                bounded(s.radius, 1e-6, 16.)
                                    && bounded(s.feather, 0., 100.)
                                    && bounded(s.flow, 0., 100.)
                                    && s.points
                                        .iter()
                                        .all(|p| coords(&[p[0], p[1]]) && bounded(p[2], 0., 1.))
                            })
                    }
                    MaskKind::ColorRange { samples, amount } => {
                        !samples.is_empty()
                            && bounded(*amount, 0., 100.)
                            && samples.iter().flatten().all(|v| v.is_finite())
                    }
                    MaskKind::Subject { .. }
                    | MaskKind::Sky { .. }
                    | MaskKind::Background { .. }
                    | MaskKind::Object { .. } => c.adobe_ai.is_some(),
                    _ => false,
                })
    })
}

/// Machine A's approximation contract. This envelope is informational only:
/// it is never copied into ImportedImage warnings or the unsupported UI count.
pub(crate) fn record_approximation_diagnostics(
    recipe: &mut engine_api::recipe::Recipe,
    root: Node<'_, '_>,
) {
    let mut extra_notes = Vec::new();
    for (i, g) in recipe.settings.locals.adjustments.iter().enumerate() {
        let p = &g.params;
        let source_fields =
            sequence(root).and_then(|groups| groups.get(i).and_then(|n| fields(*n)));
        let has_source = |key: &str| source_fields.as_ref().is_some_and(|f| f.contains_key(key));
        for (present, key, field, reason) in [
            (
                p.curves.is_some() && has_source("MainCurve"),
                "MainCurve",
                "curves",
                "local point curves use Tessera spline interpolation",
            ),
            (
                p.curves_extended.is_some() && has_source("ExtendedMainCurve"),
                "ExtendedMainCurve",
                "curves_extended",
                "local extended point curves use Tessera spline interpolation",
            ),
            (
                p.curves.is_some() && has_source("RedCurve"),
                "RedCurve",
                "curves",
                "local point curves use Tessera spline interpolation",
            ),
            (
                p.curves.is_some() && has_source("GreenCurve"),
                "GreenCurve",
                "curves",
                "local point curves use Tessera spline interpolation",
            ),
            (
                p.curves.is_some() && has_source("BlueCurve"),
                "BlueCurve",
                "curves",
                "local point curves use Tessera spline interpolation",
            ),
            (
                p.curves_extended.is_some() && has_source("ExtendedRedCurve"),
                "ExtendedRedCurve",
                "curves_extended",
                "local point curves use Tessera spline interpolation",
            ),
            (
                p.curves_extended.is_some() && has_source("ExtendedGreenCurve"),
                "ExtendedGreenCurve",
                "curves_extended",
                "local point curves use Tessera spline interpolation",
            ),
            (
                p.curves_extended.is_some() && has_source("ExtendedBlueCurve"),
                "ExtendedBlueCurve",
                "curves_extended",
                "local point curves use Tessera spline interpolation",
            ),
            (
                p.color_overlay.is_some() && has_source("LocalToningHue"),
                "LocalToningHue",
                "color_overlay",
                "local tint blends a luminance-preserving hue at the requested saturation",
            ),
            (
                p.point_colors.is_some(),
                "LocalPointColors",
                "point_colors",
                "local Point Color uses the shared HSL operator before monochrome",
            ),
            (
                p.color_overlay.is_some(),
                "LocalToningSaturation",
                "color_overlay",
                "local tint blends a luminance-preserving hue at the requested saturation",
            ),
            (
                p.defringe != 0.,
                "LocalDefringe",
                "defringe",
                "local defringe uses Tessera edge-selective purple and green suppression",
            ),
        ] {
            if present {
                extra_notes.push((
                    format!("MaskGroupBasedCorrections/{key}"),
                    format!("/settings/locals/adjustments/{i}/params/{field}"),
                    reason,
                ));
            }
        }
    }
    for (key, path, reason) in extra_notes {
        crate::diagnostics::push_approximate(recipe, &key, &path, "LR-11", reason);
    }
    let mut categories = Vec::new();
    let mut stack: Vec<_> = recipe
        .settings
        .locals
        .adjustments
        .iter()
        .flat_map(|g| &g.components)
        .collect();
    while let Some(c) = stack.pop() {
        if let Some(children) = &c.group {
            stack.extend(children);
        }
        if let Some(state) = &c.adobe_ai {
            categories.push(state.category.clone());
        }
    }
    if !categories.is_empty() {
        for f in root.descendants().filter_map(fields) {
            if let Some(what) = f.get("What").and_then(Field::scalar).filter(|s| {
                matches!(
                    *s,
                    "Mask/Image"
                        | "Mask/Subject"
                        | "Mask/Sky"
                        | "Mask/Background"
                        | "Mask/Object"
                        | "Mask/People"
                        | "Mask/Person"
                )
            }) {
                crate::diagnostics::push_approximate(
                    recipe,
                    &format!("MaskGroupBasedCorrections/{what}"),
                    "/settings/locals/adjustments",
                    "LR-5",
                    "AI category and sensor-coordinate interpretation are approximate; Adobe model and raster codec are unverified",
                );
            }
        }
    }
    let shape_value = root.descendants().filter_map(fields).any(|f| {
        matches!(
            f.get("What").and_then(Field::scalar),
            Some("Mask/Gradient" | "Mask/CircularGradient")
        ) && f.contains_key("MaskValue")
    });
    let mut reasons = BTreeMap::from([(
        "MaskGroupBasedCorrections".to_string(),
        "ordered recipe composition and pre-geometry sensor coordinates; Adobe blend and coordinate conventions are unverified",
    )]);
    for n in root.descendants().filter(Node::is_element) {
        let properties = n
            .attributes()
            .filter(|a| a.namespace() == Some(CRS))
            .map(|a| (a.name(), a.value()))
            .chain(
                (n.tag_name().namespace() == Some(CRS))
                    .then_some((n.tag_name().name(), n.text().unwrap_or(""))),
            );
        for (name, value) in properties {
            let (field, reason) = match name {
                "Dabs" => (
                    "Mask/Paint/Dabs",
                    "d stamps use normalized x/y; r/f/h persist for following stamps; no path interpolation; Tessera smoothstep hardness and linear flow assumed",
                ),
                "Radius" => ("Mask/Paint/Radius", "radius normalized to image width"),
                "Flow" => (
                    "Mask/Paint/Flow",
                    "unit flow mapped to linear opacity times 100; Adobe accumulation unverified",
                ),
                "CenterWeight" => (
                    "Mask/Paint/CenterWeight",
                    "unit hardness mapped to (1-hardness)*100 smoothstep feather",
                ),
                "MaskValue" => (
                    "MaskValue",
                    "paint value maps to stamp opacity (zero erases); parametric shapes use blend/inversion and unit selection",
                ),
                "Midpoint" | "Roundness" => (
                    name,
                    "Tessera elliptical smoothstep radial shape used; Adobe midpoint/roundness shape modifier retained, not reproduced",
                ),
                "MaskBlendMode" => (
                    "MaskBlendMode",
                    "codes 0/1/2 assumed add/subtract/intersect; Adobe convention unverified",
                ),
                "Type" if value == "1" => (
                    "CorrectionRangeMask/Type=1",
                    "color subtype with encoded sRGB sample assumption",
                ),
                "Type" if value == "2" => (
                    "CorrectionRangeMask/Type=2",
                    "luminance subtype in display-encoded perceptual domain",
                ),
                "Type" if value == "3" => (
                    "CorrectionRangeMask/Type=3",
                    "depth subtype uses normalized Tessera or supplied depth",
                ),
                "MaskActive" => (
                    "MaskActive",
                    "false excludes this component and subtree before ordered composition",
                ),
                "Masks" => (
                    "Masks",
                    "ordered nested composition uses Tessera add/subtract/intersect semantics",
                ),
                "CorrectionRangeMask" => (
                    "CorrectionRangeMask",
                    "range intersects the seed after seed inversion; Adobe selection kernel unverified",
                ),
                "LumRange" | "LumMin" | "LumMax" | "LumFeather" => (
                    name,
                    "range evaluated on sRGB-display-encoded Rec.2020 luminance with smoothstep shoulders; Adobe perceptual transfer unverified",
                ),
                "DepthMin" | "DepthMax" | "DepthFeather" => (
                    name,
                    "normalized range evaluated against supplied or Tessera-estimated depth, not Adobe depth calibration",
                ),
                "PointModels" => (
                    "PointModels",
                    "leading sample triple assumed encoded sRGB D65 and converted to OkLab; sample-position/reserved values retained",
                ),
                "AreaModels" => (
                    "AreaModels",
                    "leading sample triple assumed encoded sRGB D65 and converted to OkLab; area shape/remaining values retained, selection uses sample color only",
                ),
                "ColorAmount" => (
                    "ColorAmount",
                    "unit amount maps to OkLab distance tolerance with Tessera smoothstep, not a measured Adobe kernel",
                ),
                "Flipped" => (
                    "Flipped",
                    "complement of MaskInverted applied once; conflicting flags refused",
                ),
                "What" if value == "Mask/CircularGradient" => (
                    "Mask/CircularGradient",
                    "ellipse rotates in normalized image coordinates; Adobe rotation/aspect and feather conventions unverified",
                ),
                "What" if value == "Mask/Paint" => (
                    "Mask/Paint",
                    "paint stamps use Tessera smoothstep hardness and linear flow; Adobe accumulation unverified",
                ),
                "What" if value == "Mask/Gradient" => (
                    "Mask/Gradient",
                    "linear full-to-zero projection in normalized pre-geometry coordinates; Adobe convention unverified",
                ),
                _ => continue,
            };
            let field = if matches!(
                name,
                "LumRange"
                    | "LumMin"
                    | "LumMax"
                    | "LumFeather"
                    | "DepthMin"
                    | "DepthMax"
                    | "DepthFeather"
                    | "PointModels"
                    | "AreaModels"
                    | "ColorAmount"
            ) {
                format!("CorrectionRangeMask/{field}")
            } else {
                field.to_string()
            };
            reasons.insert(format!("MaskGroupBasedCorrections/{field}"), reason);
        }
    }
    // Resolve to a field actually produced by translation. Retained metadata
    // (for example Midpoint/Roundness) must not claim an approximate translation.
    let value = serde_json::to_value(&recipe.settings.locals.adjustments)
        .expect("finite translated mask fields");
    for (key, reason) in reasons {
        let suffix = key.strip_prefix("MaskGroupBasedCorrections/").unwrap_or("");
        if suffix.is_empty() {
            crate::diagnostics::push_approximate(
                recipe,
                &key,
                "/settings/locals/adjustments",
                "LR-4",
                reason,
            );
            continue;
        }
        let field = match suffix {
            "Mask/Paint" | "Mask/Paint/Dabs" | "MaskValue" => "strokes",
            "Mask/Paint/Radius" => "strokes/0/radius",
            "Mask/Paint/Flow" => "strokes/0/flow",
            "Mask/Paint/CenterWeight" => "strokes/0/feather",
            "MaskBlendMode" => "combine",
            "MaskActive" => "enabled",
            "Masks" => "group",
            "CorrectionRangeMask/LumRange" => "luminance_bounds",
            "CorrectionRangeMask/PointModels"
            | "CorrectionRangeMask/AreaModels"
            | "CorrectionRangeMask/Type=1" => "samples",
            "CorrectionRangeMask/ColorAmount" => "amount",
            "CorrectionRangeMask"
            | "CorrectionRangeMask/Type=2"
            | "CorrectionRangeMask/Type=3"
            | "CorrectionRangeMask/LumMin"
            | "CorrectionRangeMask/LumMax"
            | "CorrectionRangeMask/DepthMin"
            | "CorrectionRangeMask/DepthMax" => "range",
            "CorrectionRangeMask/LumFeather" => "smoothness",
            "CorrectionRangeMask/DepthFeather" => "feather",
            "Flipped" => "invert",
            "Mask/CircularGradient" => "radii",
            "Mask/Gradient" => "start",
            _ => continue,
        };
        let mut paths = Vec::new();
        for (i, adjustment) in value.as_array().into_iter().flatten().enumerate() {
            if let Some(components) = adjustment["components"].as_array() {
                translated_field_paths(
                    components,
                    &format!("/settings/locals/adjustments/{i}/components"),
                    field,
                    &mut paths,
                );
            }
        }
        if suffix == "MaskValue" && shape_value {
            crate::diagnostics::push_approximate(
                recipe,
                &key,
                "/settings/locals/adjustments",
                "LR-4",
                "parametric shape MaskValue is retained but not reproduced: unit selection used",
            );
        }
        for path in paths {
            crate::diagnostics::push_approximate(recipe, &key, &path, "LR-4", reason);
        }
    }
}

fn translated_field_paths(
    components: &[serde_json::Value],
    prefix: &str,
    field: &str,
    paths: &mut Vec<String>,
) {
    for (i, component) in components.iter().enumerate() {
        let path = format!("{prefix}/{i}");
        if component
            .pointer(&format!("/{field}"))
            .is_some_and(|v| !v.is_null() && !v.as_array().is_some_and(Vec::is_empty))
        {
            paths.push(format!("{path}/{field}"));
        }
        if let Some(children) = component["group"].as_array() {
            translated_field_paths(children, &format!("{path}/group"), field, paths);
        }
    }
}

/// Static feature labels only: never include property values in diagnostics.
pub(crate) fn unsupported_reason(root: Node<'_, '_>) -> String {
    let mut reasons = std::collections::BTreeSet::new();
    for n in root.descendants() {
        for name in n
            .attributes()
            .filter(|a| a.namespace() == Some(CRS))
            .map(|a| a.name())
            .chain((n.tag_name().namespace() == Some(CRS)).then_some(n.tag_name().name()))
        {
            if let Some(reason) = match name {
                "MainCurve" | "RedCurve" | "GreenCurve" | "BlueCurve" | "ExtendedMainCurve"
                | "ExtendedRedCurve" | "ExtendedGreenCurve" | "ExtendedBlueCurve" => {
                    Some("local tone curve encoding cannot be decoded")
                }
                "LocalPointColors" => Some("local point-color encoding cannot be decoded"),

                "InstanceBounds" | "InstanceIDs" => {
                    Some("individual AI instance selection is not implemented")
                }
                _ => None,
            } {
                reasons.insert(reason);
            }
        }
        if let Some(f) = fields(n) {
            if f.get("What").and_then(Field::scalar).is_some_and(|what| {
                what.starts_with("Mask/")
                    && !matches!(
                        what,
                        "Mask/Image"
                            | "Mask/Subject"
                            | "Mask/Sky"
                            | "Mask/Background"
                            | "Mask/People"
                            | "Mask/Person"
                            | "Mask/Object"
                            | "Mask/Gradient"
                            | "Mask/CircularGradient"
                            | "Mask/Paint"
                            | "Mask/Group"
                            | "Mask/Aggregate"
                            | "Mask/Range"
                            | "Mask/RangeMask"
                    )
            }) {
                reasons.insert("unrecognized mask selection kind");
            }
            if let Some(value) = f.get("LocalColorVariance") {
                let neutral = match value {
                    Field::Scalar(v) => v.parse::<f64>().ok() == Some(0.),
                    Field::Structure(n) => sequence(*n).is_some_and(|items| {
                        items.iter().all(|n| {
                            n.text().and_then(|s| s.trim().parse::<f64>().ok()) == Some(0.)
                        })
                    }),
                };
                if !neutral {
                    reasons.insert("local color-variance adjustment is not implemented");
                }
            }
            for (key, label) in [
                (
                    "LocalDefringe",
                    "local defringe value is outside the supported range",
                ),
                (
                    "LocalToningSaturation",
                    "local color overlay encoding cannot be decoded",
                ),
            ] {
                if f.get(key)
                    .and_then(Field::scalar)
                    .and_then(|v| v.parse::<f64>().ok())
                    .is_some_and(|v| v != 0.)
                {
                    reasons.insert(label);
                }
            }
        }
    }
    if reasons.is_empty() {
        reasons.insert("mask geometry, blend mode or selection encoding cannot be rendered");
    }
    reasons.into_iter().collect::<Vec<_>>().join("; ")
}

pub(crate) fn decoder_reason(reason: String, warning: &str) -> String {
    if warning.contains("radial Flipped") {
        return "radial mask inversion flags conflict".into();
    }
    if reason != "mask geometry, blend mode or selection encoding cannot be rendered" {
        return reason;
    }
    for (needle, label) in [
        ("radial Flipped", "radial mask inversion flags conflict"),
        (
            "unknown Adobe AI mask subtype",
            "unrecognized AI selection subtype",
        ),
        (
            "unsupported Adobe person or part mask",
            "AI person, part or instance selection is not implemented",
        ),
        (
            "unsupported Adobe AI mask part",
            "AI person, part or instance selection is not implemented",
        ),
        (
            "unknown Adobe AI mask category",
            "unrecognized AI selection category",
        ),
        (
            "invalid object bounds",
            "AI object selection bounds are invalid",
        ),
        (
            "invalid object reference point",
            "AI object selection reference point is invalid",
        ),
        (
            "object regeneration requires",
            "AI object selection lacks a box or reference point",
        ),
        (
            "mask tree exceeds",
            "nested mask selection exceeds eight levels",
        ),
        (
            "mask blend mode",
            "mask selection uses an unrecognized blend mode",
        ),
        ("Adobe dab", "brush stamp encoding cannot be decoded"),
        ("Adobe Dabs", "brush stamp list is empty or invalid"),
        (
            "range mask",
            "range-mask selection encoding is incomplete or ambiguous",
        ),
        (
            "color sample",
            "color-range sample encoding cannot be decoded",
        ),
    ] {
        if warning.contains(needle) {
            return label.into();
        }
    }
    reason
}
