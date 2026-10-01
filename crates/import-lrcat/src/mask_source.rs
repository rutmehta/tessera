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
        "Mask/Gradient"
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
            ("MaskValue" | "Midpoint" | "Roundness", Field::Scalar(v)) => {
                if !v.parse::<f64>().ok()?.is_finite() {
                    return None;
                }
            }
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
    for (name, value) in fields(n)? {
        match (name.as_str(), value) {
            ("LocalToningHue" | "LocalToningSaturation", Field::Scalar(v))
                if v.parse::<f64>().ok()? == 0.0 => {}
            ("LocalDefringe", Field::Scalar(v)) if v.parse::<f64>().ok()? != 0.0 => return None,
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
        [
            "Dabs",
            "Masks",
            "CorrectionRangeMask",
            "Flipped",
            "MaskID",
            "MaskSyncID",
            "MaskName",
            "MaskVersion",
            "MaskValue",
            "Midpoint",
            "Roundness",
            "CorrectionID",
            "CorrectionSyncID",
            "LocalToningHue",
            "LocalToningSaturation",
        ]
        .iter()
        .any(|key| n.has_tag_name((CRS, *key)) || n.attribute((CRS, *key)).is_some())
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
                    MaskKind::LuminanceRange { range, smoothness } => {
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
                    _ => false,
                })
    })
}

/// Machine A's approximation contract. This envelope is informational only:
/// it is never copied into ImportedImage warnings or the unsupported UI count.
pub(crate) fn approximation_diagnostics(root: Node<'_, '_>) -> serde_json::Value {
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
    serde_json::Value::Array(
        reasons
            .into_iter()
            .map(|(key, reason)| {
                serde_json::json!({
                    "key":key, "level":"info", "message":format!("approximate: {reason}")
                })
            })
            .collect(),
    )
}
