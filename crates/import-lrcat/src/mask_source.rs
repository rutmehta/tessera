//! Source promotion is narrower than decoding: every field must be consumed in
//! its actual structural position. Legacy flat groups retain their exact envelope.
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
fn range(n: Node<'_, '_>) -> Option<()> {
    let f = fields(n)?;
    let lum = ["LumMin", "LumMax", "LumFeather", "LumRange"]
        .iter()
        .any(|key| f.contains_key(*key));
    let depth = ["DepthMin", "DepthMax", "DepthFeather"]
        .iter()
        .any(|key| f.contains_key(*key));
    if lum == depth {
        return None;
    }
    for (name, value) in &f {
        if !matches!(
            name.as_str(),
            "Type"
                | "LumRange"
                | "LumMin"
                | "LumMax"
                | "LumFeather"
                | "DepthMin"
                | "DepthMax"
                | "DepthFeather"
                | "Invert"
        ) || value.scalar().is_none()
        {
            return None;
        }
    }
    for name in ["LumFeather", "DepthFeather"] {
        if let Some(value) = f.get(name)
            && value.scalar()?.parse::<f64>().ok()? != 0.0
        {
            return None;
        }
    }
    // The codec validates bounds and requires exactly one complete range family.
    Some(())
}
fn component(n: Node<'_, '_>) -> Option<()> {
    let f = fields(n)?;
    let kind = f.get("What")?.scalar()?;
    if !matches!(
        kind,
        "Mask/Gradient"
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
                    component(n)?;
                }
            }
            ("CorrectionRangeMask", Field::Structure(n)) => range(*n)?,
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
            ("LocalToningHue" | "LocalToningSaturation", _) => return None,
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
pub(crate) fn fully_translated(root: Node<'_, '_>) -> bool {
    // Do not change the bytes or allocate audit maps for previously supported
    // flat shapes (the 29c baseline), even though their geometry already maps.
    let new_shape = root.descendants().any(|n| {
        n.has_tag_name((CRS, "Masks"))
            || n.has_tag_name((CRS, "CorrectionRangeMask"))
            || n.has_tag_name((CRS, "Flipped"))
            || n.attribute((CRS, "Flipped")).is_some()
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
                    // Other foreign payloads are not understood by the structural audit.
                    _ => false,
                })
    })
}
