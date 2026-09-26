//! MWG face regions, independent of the engine recipe schema.
use crate::{XmpPacket, xml::*};
use engine_api::error::EngineResult;

const MWG: &str = "http://www.metadataworkinggroup.com/schemas/regions/";
const AREA: &str = "http://ns.adobe.com/xmp/sType/Area#";

/// A named rectangle in normalized image coordinates. `x` and `y` are its center,
/// not its upper-left corner. Centers must be finite in `[0, 1]`; width and height
/// must be finite in `(0, 1]`. Rectangles may extend beyond an image edge.
/// No orientation transform is performed.
#[derive(Debug, Clone, PartialEq)]
pub struct FaceRegion {
    pub name: String,
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

fn validate(face: &FaceRegion) -> EngineResult<()> {
    if [face.x, face.y, face.w, face.h]
        .iter()
        .any(|v| !v.is_finite() || !(0.0..=1.0).contains(v))
        || face.w == 0.0
        || face.h == 0.0
    {
        return Err(error(
            "Face center/size must be finite and normalized; size must be positive",
        ));
    }
    Ok(())
}

fn child<'a>(t: &'a Tree, n: &Node, ns: &str, local: &str) -> Option<&'a Node> {
    n.children
        .iter()
        .map(|i| &t.nodes[*i])
        .find(|n| n.ns == ns && n.local == local)
}
fn resource<'a>(t: &'a Tree, n: &'a Node) -> &'a Node {
    child(t, n, RDF, "Description").unwrap_or(n)
}
fn value(t: &Tree, n: &Node, ns: &str, local: &str) -> Option<String> {
    let n = resource(t, n);
    n.attrs
        .iter()
        .find(|a| a.ns == ns && a.local == local)
        .map(|a| a.value.clone())
        .or_else(|| child(t, n, ns, local).map(|n| n.text.clone()))
}
// Append in place, expanding a self-closing element without changing its attributes.
fn append_edit(xml: &str, node: &Node, body: String) -> (std::ops::Range<usize>, String) {
    let raw = &xml[node.span.clone()];
    if raw.ends_with("/>") {
        let name = raw[1..]
            .split(|c: char| c.is_whitespace() || c == '/' || c == '>')
            .next()
            .unwrap();
        (node.close..node.span.end, format!(">{body}</{name}>"))
    } else {
        (node.close..node.close, body)
    }
}
fn region_list<'a>(tree: &'a Tree, regions: &'a Node) -> Option<&'a Node> {
    child(tree, resource(tree, regions), MWG, "RegionList")
}

impl XmpPacket {
    /// Set MWG AppliedToDimensions for the oriented coordinate space of the
    /// regions. Call after with_face_regions. Does not change face geometry.
    pub fn with_face_dimensions(&self, width: u32, height: u32) -> EngineResult<Self> {
        if width == 0 || height == 0 {
            return Err(error("zero face coordinate space"));
        }
        let tree = Tree::parse(&self.xml)?;
        let Some(Property::Node(regions)) = tree.property(MWG, "Regions") else {
            return Err(error("missing MWG Regions"));
        };
        let regions = resource(&tree, regions);
        let body = format!(
            r#"<mwg-rs:AppliedToDimensions xmlns:mwg-rs="{MWG}" xmlns:rdf="{RDF}" xmlns:stDim="http://ns.adobe.com/xap/1.0/sType/Dimensions#" rdf:parseType="Resource" stDim:w="{width}" stDim:h="{height}" stDim:unit="pixel"/>"#
        );
        let (range, replacement) =
            if let Some(node) = child(&tree, regions, MWG, "AppliedToDimensions") {
                (node.span.clone(), body)
            } else {
                append_edit(&self.xml, regions, body)
            };
        let mut xml = self.xml.clone();
        xml.replace_range(range, &replacement);
        Self::parse(xml)
    }

    /// Read normalized MWG Face entries (not Microsoft regions). Accepts arbitrary
    /// namespace prefixes, attribute/element fields and nested rdf:Description structs.
    /// Like other packet property readers, uses the first top-level MWG Regions property.
    /// Missing names become empty strings; invalid geometry or non-normalized Face units
    /// return an error rather than silently discarding a face.
    pub fn face_regions(&self) -> EngineResult<Vec<FaceRegion>> {
        let tree = Tree::parse(&self.xml)?;
        let Some(Property::Node(regions)) = tree.property(MWG, "Regions") else {
            return Ok(Vec::new());
        };
        let Some(list) = region_list(&tree, regions) else {
            return Ok(Vec::new());
        };
        tree.items(list)
            .into_iter()
            .filter(|n| value(&tree, n, MWG, "Type").as_deref() == Some("Face"))
            .map(|n| {
                let area = child(&tree, resource(&tree, n), MWG, "Area")
                    .ok_or_else(|| error("Face missing MWG Area"))?;
                if value(&tree, area, AREA, "unit").as_deref() != Some("normalized") {
                    return Err(error("Face Area must use normalized units"));
                }
                let number = |key| -> EngineResult<f64> {
                    value(&tree, area, AREA, key)
                        .ok_or_else(|| error(format!("Face Area missing {key}")))?
                        .trim()
                        .parse()
                        .map_err(error)
                };
                let face = FaceRegion {
                    name: value(&tree, n, MWG, "Name").unwrap_or_default(),
                    x: number("x")?,
                    y: number("y")?,
                    w: number("w")?,
                    h: number("h")?,
                };
                validate(&face)?;
                Ok(face)
            })
            .collect()
    }

    /// Replace Face entries in the first MWG Regions property, preserving non-face
    /// regions, applied dimensions and foreign XML byte-for-byte. Applied dimensions
    /// are not synthesized because this API does not receive image dimensions.
    /// Extensions attached to replaced Face entries are removed.
    /// If `person_keywords`, append nonblank names to dc:subject (exact case-sensitive
    /// deduplication); existing keywords are never removed. Hierarchical keywords are untouched.
    pub fn with_face_regions(
        &self,
        faces: &[FaceRegion],
        person_keywords: bool,
    ) -> EngineResult<Self> {
        let tree = Tree::parse(&self.xml)?;
        for face in faces {
            validate(face)?;
        }
        // Local namespace declarations avoid collisions with arbitrary existing prefixes.
        let items: String = faces.iter().map(|f| format!(r#"<rdf:li xmlns:rdf="{RDF}" xmlns:mwg-rs="{MWG}" xmlns:stArea="{AREA}" rdf:parseType="Resource"><mwg-rs:Name>{}</mwg-rs:Name><mwg-rs:Type>Face</mwg-rs:Type><mwg-rs:Area rdf:parseType="Resource" stArea:x="{}" stArea:y="{}" stArea:w="{}" stArea:h="{}" stArea:unit="normalized"/></rdf:li>"#, escape(&f.name), f.x, f.y, f.w, f.h)).collect();
        let xml = match tree.property(MWG, "Regions") {
            Some(Property::Node(regions)) => {
                let mut edits = Vec::new();
                if let Some(list) = region_list(&tree, regions) {
                    let bag = child(&tree, list, RDF, "Bag")
                        .ok_or_else(|| error("MWG RegionList missing rdf:Bag"))?;
                    edits.extend(
                        tree.items(list)
                            .into_iter()
                            .filter(|n| value(&tree, n, MWG, "Type").as_deref() == Some("Face"))
                            .map(|n| (n.span.clone(), String::new())),
                    );
                    edits.push(append_edit(&self.xml, bag, items));
                } else {
                    edits.push(append_edit(&self.xml, resource(&tree, regions), format!(r#"<mwg-rs:RegionList xmlns:mwg-rs="{MWG}" xmlns:rdf="{RDF}"><rdf:Bag>{items}</rdf:Bag></mwg-rs:RegionList>"#)));
                }
                edits.sort_by_key(|(r, _)| r.start);
                let mut xml = self.xml.clone();
                for (range, replacement) in edits.into_iter().rev() {
                    xml.replace_range(range, &replacement);
                }
                xml
            }
            Some(Property::Scalar(_)) => return Err(error("MWG Regions must be a structure")),
            None => {
                let body = format!(
                    r#"<mwg-rs:Regions xmlns:mwg-rs="{MWG}" rdf:parseType="Resource"><mwg-rs:RegionList><rdf:Bag>{items}</rdf:Bag></mwg-rs:RegionList></mwg-rs:Regions>"#
                );
                let (range, replacement) =
                    append_edit(&self.xml, &tree.nodes[tree.rdf], description(&body));
                let mut xml = self.xml.clone();
                xml.replace_range(range, &replacement);
                xml
            }
        };
        if person_keywords {
            let mut keywords = tree.list(DC, "subject");
            for face in faces {
                if !face.name.trim().is_empty() && !keywords.contains(&face.name) {
                    keywords.push(face.name.clone());
                }
            }
            return Self::parse(Tree::parse(&xml)?.replace(
                &xml,
                &[(DC, "subject")],
                &container("dc:subject", "Bag", &keywords),
            )?);
        }
        Self::parse(xml)
    }
}
