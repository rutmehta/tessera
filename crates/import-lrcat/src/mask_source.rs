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
                if (-100.0..=100.0).contains(&v.parse::<f64>().ok()?) => {}
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

/// Stable group ids of the source groups, in source order, by the shared
/// codec's rule (`sidecar::assign_mask_group_ids`). `None` if a native id is
/// malformed or repeated; no group is then paired with a source.
fn source_group_ids(groups: &[Node<'_, '_>]) -> Option<Vec<u64>> {
    let native = groups
        .iter()
        .map(|group| {
            let resource = group
                .children()
                .find(|c| c.has_tag_name((RDF, "Description")))
                .unwrap_or(*group);
            let mut ids = resource
                .children()
                .filter(|c| c.has_tag_name((engine_api::recipe::crs::TS_NAMESPACE, "LocalId")));
            match (ids.next(), ids.next()) {
                (None, _) => Some(None),
                (Some(id), None) => id.text()?.trim().parse::<u64>().ok().map(Some),
                _ => None,
            }
        })
        .collect::<Option<Vec<_>>>()?;
    sidecar::assign_mask_group_ids(&native)
}

/// Machine A's approximation contract. This envelope is informational only:
/// it is never copied into ImportedImage warnings or the unsupported UI count.
pub(crate) fn record_approximation_diagnostics(
    recipe: &mut engine_api::recipe::Recipe,
    root: Node<'_, '_>,
) {
    let mut extra_notes = Vec::new();
    // Pair each recipe group with its source group by the codec's stable group
    // id. Position is not an identity: a group whose id has no source group
    // gets no source-keyed note.
    let source_groups = sequence(root).unwrap_or_default();
    let source_ids = source_group_ids(&source_groups);
    for (i, g) in recipe.settings.locals.adjustments.iter().enumerate() {
        let p = &g.params;
        let source_fields = source_ids
            .as_ref()
            .and_then(|ids| ids.iter().position(|id| *id == u64::from(g.id.0)))
            .and_then(|j| fields(source_groups[j]));
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
                p.point_colors.is_some() && has_source("LocalPointColors"),
                "LocalPointColors",
                "point_colors",
                "local Point Color uses the shared HSL operator before monochrome",
            ),
            (
                p.color_overlay.is_some() && has_source("LocalToningSaturation"),
                "LocalToningSaturation",
                "color_overlay",
                "local tint blends a luminance-preserving hue at the requested saturation",
            ),
            (
                p.defringe > 0. && has_source("LocalDefringe"),
                "LocalDefringe",
                "defringe",
                "local defringe uses Tessera edge-selective purple and green suppression",
            ),
            (
                p.defringe < 0. && has_source("LocalDefringe"),
                "LocalDefringe",
                "defringe",
                "negative local defringe protects the area from global defringe; that protection is not rendered and no local defringe is added",
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
            // Presence alone blocks the group: an instance selection is never
            // widened to the whole object. Local operators are rendered, so
            // they are named by `decoder_reason` only when they fail to decode.
            if matches!(name, "InstanceBounds" | "InstanceIDs") {
                reasons.insert(INSTANCE_REASON);
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
        }
    }
    if reasons.is_empty() {
        reasons.insert("mask geometry, blend mode or selection encoding cannot be rendered");
    }
    reasons.into_iter().collect::<Vec<_>>().join("; ")
}

const INSTANCE_REASON: &str = "individual AI instance selection is not implemented";

/// Refine the static reason with what the shared decoder actually rejected.
pub(crate) fn decoder_reason(reason: String, warning: &str) -> String {
    // The decoder stops at the first failure, so these name the real blocker
    // even when the group also carries other retained content.
    for (needle, label) in [
        (
            "radial Flipped conflicts",
            "radial mask inversion flags conflict",
        ),
        ("individual AI instance selection", INSTANCE_REASON),
        (
            "local curve:",
            "local tone curve encoding cannot be decoded",
        ),
        (
            "local point colours:",
            "local point-color encoding cannot be decoded",
        ),
        (
            "invalid local defringe",
            "local defringe value is outside the supported range",
        ),
        (
            "invalid local colour overlay",
            "local color overlay encoding cannot be decoded",
        ),
    ] {
        if warning.contains(needle) {
            return label.into();
        }
    }
    if reason != "mask geometry, blend mode or selection encoding cannot be rendered" {
        return reason;
    }
    for (needle, label) in [
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

#[cfg(test)]
mod lr11b_tests {
    use super::*;
    use engine_api::id::MaskId;
    use engine_api::recipe::{LocalAdjustment, Recipe};

    const TS: &str = engine_api::recipe::crs::TS_NAMESPACE;

    fn curve(name: &str) -> String {
        format!(
            "<crs:{name}><rdf:Seq><rdf:li>0,0</rdf:li><rdf:li>255,127.5</rdf:li></rdf:Seq></crs:{name}>"
        )
    }

    fn document(groups: &[String]) -> String {
        let items: String = groups
            .iter()
            .map(|g| format!("<rdf:li rdf:parseType=\"Resource\">{g}</rdf:li>"))
            .collect();
        format!(
            "<crs:MaskGroupBasedCorrections xmlns:crs=\"{CRS}\" xmlns:rdf=\"{RDF}\" xmlns:ts=\"{TS}\"><rdf:Seq>{items}</rdf:Seq></crs:MaskGroupBasedCorrections>"
        )
    }

    fn group(id: u32) -> LocalAdjustment {
        let mut g = LocalAdjustment {
            id: MaskId(id),
            ..Default::default()
        };
        g.params.curves = Some(Default::default());
        g
    }

    fn noted(recipe: &Recipe, key: &str) -> Vec<String> {
        crate::diagnostics::entries(recipe)
            .get(&format!("MaskGroupBasedCorrections/{key}"))
            .into_iter()
            .flatten()
            .filter(|e| e.lane == "LR-11")
            .filter_map(|e| e.field.clone())
            .collect()
    }

    /// S8: a recipe group is matched to its source group by the codec's stable
    /// group id, not by its position in the recipe.
    #[test]
    fn s8_groups_match_their_source_by_stable_id_not_index() {
        // Source order: id 0 carries MainCurve, id 1 carries RedCurve.
        let xml = document(&[curve("MainCurve"), curve("RedCurve")]);
        let doc = roxmltree::Document::parse(&xml).unwrap();
        let mut recipe = Recipe::default();
        // The recipe lists the same two groups in the opposite order.
        recipe.settings.locals.adjustments = vec![group(1), group(0)];
        record_approximation_diagnostics(&mut recipe, doc.root_element());
        assert_eq!(
            noted(&recipe, "MainCurve"),
            ["/settings/locals/adjustments/1/params/curves"]
        );
        assert_eq!(
            noted(&recipe, "RedCurve"),
            ["/settings/locals/adjustments/0/params/curves"]
        );
    }

    /// S8: a recipe group whose id has no source group gets no source-keyed
    /// note (fail closed), and the remaining group still finds its own source.
    #[test]
    fn s8_group_without_a_source_id_gets_no_source_keyed_note() {
        let xml = document(&[curve("MainCurve"), curve("RedCurve")]);
        let doc = roxmltree::Document::parse(&xml).unwrap();
        let mut recipe = Recipe::default();
        recipe.settings.locals.adjustments = vec![group(7), group(1)];
        record_approximation_diagnostics(&mut recipe, doc.root_element());
        assert!(noted(&recipe, "MainCurve").is_empty());
        assert_eq!(
            noted(&recipe, "RedCurve"),
            ["/settings/locals/adjustments/1/params/curves"]
        );
    }

    /// Every source-keyed operator must follow its own source id and key,
    /// including both signs of defringe and recipes with unmatched ids.
    #[test]
    fn s8_non_curve_notes_require_the_matching_source_key() {
        for (key, field) in [
            ("LocalPointColors", "point_colors"),
            ("LocalToningHue", "color_overlay"),
            ("LocalToningSaturation", "color_overlay"),
            ("LocalDefringe", "defringe"),
        ] {
            for defringe in [-50., 50.] {
                let xml = document(&[format!("<crs:{key}>1</crs:{key}>"), curve("RedCurve")]);
                let doc = roxmltree::Document::parse(&xml).unwrap();
                for (ids, expected) in [
                    (
                        [1, 0],
                        vec![format!("/settings/locals/adjustments/1/params/{field}")],
                    ),
                    ([7, 1], vec![]),
                ] {
                    let mut recipe = Recipe::default();
                    recipe.settings.locals.adjustments = ids
                        .into_iter()
                        .map(|id| {
                            let mut g = group(id);
                            g.params.point_colors = Some(Default::default());
                            g.params.color_overlay = Some(Default::default());
                            g.params.defringe = defringe;
                            g
                        })
                        .collect();
                    record_approximation_diagnostics(&mut recipe, doc.root_element());
                    assert_eq!(
                        noted(&recipe, key),
                        expected,
                        "{key}, ids={ids:?}, defringe={defringe}"
                    );
                    for absent in [
                        "LocalPointColors",
                        "LocalToningHue",
                        "LocalToningSaturation",
                        "LocalDefringe",
                    ] {
                        if absent != key {
                            assert!(
                                noted(&recipe, absent).is_empty(),
                                "absent key {absent}, source key {key}"
                            );
                        }
                    }
                }
            }
        }
    }

    /// S8: the id rule is the shared codec's: foreign groups take the lowest
    /// ids that no native `ts:LocalId` in the packet uses, in source order.
    #[test]
    fn s8_foreign_group_ids_skip_native_ids() {
        let native = format!(
            "<ts:LocalId ts:type=\"number\">0</ts:LocalId>{}",
            curve("BlueCurve")
        );
        let xml = document(&[curve("MainCurve"), native, curve("RedCurve")]);
        let doc = roxmltree::Document::parse(&xml).unwrap();
        let mut recipe = Recipe::default();
        // Codec ids in source order are 1, 0 (native), 2.
        recipe.settings.locals.adjustments = vec![group(1), group(0), group(2)];
        record_approximation_diagnostics(&mut recipe, doc.root_element());
        assert_eq!(
            noted(&recipe, "MainCurve"),
            ["/settings/locals/adjustments/0/params/curves"]
        );
        assert_eq!(
            noted(&recipe, "RedCurve"),
            ["/settings/locals/adjustments/2/params/curves"]
        );
    }
}
