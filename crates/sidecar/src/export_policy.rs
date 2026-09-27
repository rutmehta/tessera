//! Namespace-aware export filtering. Source packets are never modified.
use crate::{XmpPacket, xml::*};
use engine_api::EngineResult;
use std::{collections::BTreeSet, ops::Range};

const EXIF: &str = "http://ns.adobe.com/exif/1.0/";
const EXIF_EX: &str = "http://cipa.jp/exif/1.0/";
const TIFF: &str = "http://ns.adobe.com/tiff/1.0/";
const AUX: &str = "http://ns.adobe.com/exif/1.0/aux/";
const PS: &str = "http://ns.adobe.com/photoshop/1.0/";
const RIGHTS: &str = "http://ns.adobe.com/xap/1.0/rights/";
const IPTC_EXT: &str = "http://iptc.org/std/Iptc4xmpExt/2008-02-29/";
const MWG: &str = "http://www.metadataworkinggroup.com/schemas/regions/";
const MP: &str = "http://ns.microsoft.com/photo/1.2/";
const MP_REGION: &str = "http://ns.microsoft.com/photo/1.2/t/Region#";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExportMetadataPolicy {
    All,
    CopyrightOnly,
    CopyrightAndContact,
    AllExceptCamera,
}

fn copyright(ns: &str, key: &str) -> bool {
    (ns == DC && key == "rights")
        || ns == RIGHTS
        || (ns == TIFF && key == "Copyright")
        || (ns == PS && matches!(key, "Copyright" | "Credit" | "Source"))
        || (ns == IPTC_EXT && matches!(key, "ImageSupplier" | "CopyrightOwner" | "Licensor"))
}
fn contact(ns: &str, key: &str) -> bool {
    (ns == DC && key == "creator")
        || (ns == TIFF && key == "Artist")
        || (ns == IPTC_CORE && key == "CreatorContactInfo")
        || (ns == PS && key == "AuthorsPosition")
}
fn camera(ns: &str) -> bool {
    matches!(ns, EXIF | EXIF_EX | TIFF | AUX | CRS | PRIVATE)
}
fn person(ns: &str, key: &str) -> bool {
    matches!(ns, MWG | MP | MP_REGION)
        || (ns == IPTC_EXT
            && matches!(
                key,
                "PersonInImage" | "PersonInImageWDetails" | "ImageRegion"
            ))
}
fn location(ns: &str, key: &str) -> bool {
    (matches!(ns, EXIF | EXIF_EX) && key.starts_with("GPS"))
        || (ns == PS && matches!(key, "City" | "State" | "Country"))
        || (ns == IPTC_CORE && matches!(key, "Location" | "CountryCode"))
        || (ns == IPTC_EXT
            && (key.starts_with("Location")
                || key.starts_with("GPS")
                || matches!(
                    key,
                    "City"
                        | "ProvinceState"
                        | "CountryName"
                        | "CountryCode"
                        | "Sublocation"
                        | "WorldRegion"
                )))
}

impl XmpPacket {
    /// Merge a partial sidecar for a developed export. Explicit sidecar
    /// properties (including empty values) replace embedded properties by
    /// expanded name; absent properties retain their complete embedded value.
    /// Original-copy exports deliberately do not call this method.
    pub fn with_sidecar_overrides(&self, sidecar: &Self) -> EngineResult<Self> {
        let overrides = Tree::parse(&sidecar.xml)?;
        let mut owned = Vec::new();
        for desc in overrides.descriptions() {
            for a in &desc.attrs {
                if a.ns != RDF && a.ns != "http://www.w3.org/XML/1998/namespace" {
                    owned.push((a.ns.as_str(), a.local.as_str()));
                }
            }
            for &i in &desc.children {
                let n = &overrides.nodes[i];
                owned.push((n.ns.as_str(), n.local.as_str()));
            }
        }
        let embedded = Tree::parse(&self.xml)?;
        let mut ranges = Vec::new();
        for desc in embedded.descriptions() {
            for a in &desc.attrs {
                if owned.contains(&(a.ns.as_str(), a.local.as_str())) {
                    ranges.push(a.span.clone());
                }
            }
            for &i in &desc.children {
                let n = &embedded.nodes[i];
                if owned.contains(&(n.ns.as_str(), n.local.as_str())) {
                    ranges.push(n.span.clone());
                }
            }
        }
        ranges.sort_by_key(|r| r.start);
        let mut xml = self.xml.clone();
        for range in ranges.into_iter().rev() {
            xml.replace_range(range, "");
        }
        Self::parse(format!(
            "<rdf:RDF xmlns:rdf=\"{RDF}\">{}{}</rdf:RDF>",
            standalone_descriptions(&xml)?,
            standalone_descriptions(&sidecar.xml)?,
        ))
    }

    /// Add native IPTC keywords without replacing language alternatives or
    /// foreign XMP properties. Existing hierarchy paths remain authoritative.
    pub fn with_native_keywords(&self, keywords: &[String]) -> EngineResult<Self> {
        if keywords.is_empty() {
            return Ok(self.clone());
        }
        let tree = Tree::parse(&self.xml)?;
        let mut flat = keyword_values(&tree, DC, "subject");
        let paths = keyword_values(&tree, LR, "hierarchicalSubject");
        let mut add_flat = Vec::new();
        for keyword in keywords {
            if !flat.contains(keyword) {
                flat.push(keyword.clone());
                add_flat.push(keyword.clone());
            }
        }
        let add_paths: Vec<_> = flat
            .into_iter()
            .filter(|keyword| {
                !paths
                    .iter()
                    .any(|p| p == keyword || p.split('|').next_back() == Some(keyword.as_str()))
            })
            .collect();
        let packet = append_keywords(self, DC, "subject", "dc:subject", &add_flat)?;
        append_keywords(
            &packet,
            LR,
            "hierarchicalSubject",
            "lr:hierarchicalSubject",
            &add_paths,
        )
    }

    /// Remove baked development instructions without dropping camera or contact
    /// metadata. Used by developed DNG, not original-raw copy exports.
    pub fn without_development(&self) -> EngineResult<Self> {
        let tree = Tree::parse(&self.xml)?;
        let mut owned = Vec::new();
        for desc in tree.descriptions() {
            for a in &desc.attrs {
                if matches!(a.ns.as_str(), CRS | PRIVATE) {
                    owned.push((a.ns.as_str(), a.local.as_str()));
                }
            }
            for &i in &desc.children {
                let n = &tree.nodes[i];
                if matches!(n.ns.as_str(), CRS | PRIVATE) {
                    owned.push((n.ns.as_str(), n.local.as_str()));
                }
            }
        }
        Self::parse(tree.replace(&self.xml, &owned, "")?)
    }

    /// Filter XMP by expanded names, not prefix spelling. Person removal drops
    /// MWG/Microsoft regions, IPTC person fields and keywords identifying those
    /// people, plus keywords under a `People`/`Persons` hierarchy. It cannot infer
    /// that an otherwise unmarked free-text keyword is a person's name.
    /// Hierarchy=false omits lr:hierarchicalSubject; true retains the source
    /// hierarchy, or uses flat keywords as single-level paths when absent.
    pub fn for_export(
        &self,
        policy: ExportMetadataPolicy,
        remove_person: bool,
        remove_location: bool,
        hierarchy: bool,
    ) -> EngineResult<Self> {
        self.for_export_with_person_source(policy, remove_person, remove_location, hierarchy, None)
    }

    /// Also use identities in embedded source XMP when an external sidecar
    /// supplies the exported descriptive fields. A sidecar must not hide the
    /// identities needed to remove person keywords from native IPTC.
    pub fn for_export_with_person_source(
        &self,
        policy: ExportMetadataPolicy,
        remove_person: bool,
        remove_location: bool,
        hierarchy: bool,
        source: Option<&Self>,
    ) -> EngineResult<Self> {
        let tree = Tree::parse(&self.xml)?;
        let mut names = BTreeSet::new();
        let embedded = source.map(|p| Tree::parse(&p.xml)).transpose()?;
        if remove_person {
            for tree in std::iter::once(&tree).chain(embedded.as_ref()) {
                for n in &tree.nodes {
                    if (n.ns == MWG && n.local == "Name")
                        || (n.ns == MP_REGION && n.local == "PersonDisplayName")
                        || (n.ns == IPTC_EXT && n.local == "PersonName")
                    {
                        names.insert(keyword_value(tree, n).trim().to_lowercase());
                    }
                    for a in &n.attrs {
                        if (a.ns == MWG && a.local == "Name")
                            || (a.ns == MP_REGION && a.local == "PersonDisplayName")
                            || (a.ns == IPTC_EXT && a.local == "PersonName")
                        {
                            names.insert(a.value.trim().to_lowercase());
                        }
                    }
                    if n.ns == IPTC_EXT && n.local == "PersonInImage" {
                        names.extend(
                            tree.items(n)
                                .iter()
                                .map(|n| keyword_value(tree, n).trim().to_lowercase()),
                        );
                        names.insert(keyword_value(tree, n).trim().to_lowercase());
                    }
                    for a in &n.attrs {
                        if a.ns == IPTC_EXT && a.local == "PersonInImage" {
                            names.insert(a.value.trim().to_lowercase());
                        }
                    }
                }
                let mut collect_path = |path: &str| {
                    if people_path(path) {
                        names.extend(path.split('|').skip(1).map(|p| p.trim().to_lowercase()));
                    }
                };
                for n in &tree.nodes {
                    if n.ns == LR && n.local == "hierarchicalSubject" {
                        let items = tree.items(n);
                        if items.is_empty() {
                            collect_path(&keyword_value(tree, n));
                        }
                        for item in items {
                            collect_path(&keyword_value(tree, item));
                        }
                    }
                    for a in &n.attrs {
                        if a.ns == LR && a.local == "hierarchicalSubject" {
                            collect_path(&a.value);
                        }
                    }
                }
            }
            names.remove("");
        }
        let mut ranges: Vec<Range<usize>> = Vec::new();
        let is_person = |s: &str| {
            remove_person
                && (people_path(s)
                    || s.split('|')
                        .any(|v| names.contains(&v.trim().to_lowercase())))
        };
        let allowed = |ns: &str, key: &str| match policy {
            ExportMetadataPolicy::All => true,
            ExportMetadataPolicy::CopyrightOnly => copyright(ns, key),
            ExportMetadataPolicy::CopyrightAndContact => copyright(ns, key) || contact(ns, key),
            ExportMetadataPolicy::AllExceptCamera => {
                !camera(ns) || copyright(ns, key) || contact(ns, key)
            }
        };
        for desc in tree.descriptions() {
            for a in &desc.attrs {
                if a.ns != RDF && !allowed(&a.ns, &a.local) {
                    ranges.push(a.span.clone());
                }
            }
            for &i in &desc.children {
                let n = &tree.nodes[i];
                if !allowed(&n.ns, &n.local) {
                    ranges.push(n.span.clone());
                }
            }
        }
        // Also remove fields nested in resource structs, alternate descriptions
        // and nonstandard containers; deleting a parent subsumes its children.
        for n in &tree.nodes {
            let remove = |ns: &str, key: &str| {
                (remove_person && person(ns, key))
                    || (remove_location && location(ns, key))
                    || (policy == ExportMetadataPolicy::AllExceptCamera && !allowed(ns, key))
                    || (!hierarchy && ns == LR && key == "hierarchicalSubject")
            };
            if remove(&n.ns, &n.local) {
                ranges.push(n.span.clone());
            }
            for a in &n.attrs {
                if remove(&a.ns, &a.local)
                    || (keyword_property(&a.ns, &a.local) && is_person(&a.value))
                {
                    ranges.push(a.span.clone());
                }
            }
            if keyword_property(&n.ns, &n.local) {
                let items = tree.items(n);
                if items.is_empty() && is_person(&keyword_value(&tree, n)) {
                    ranges.push(n.span.clone());
                }
                for item in items {
                    if is_person(&keyword_value(&tree, item)) {
                        ranges.push(item.span.clone());
                    }
                }
            }
        }
        ranges.sort_by_key(|r| (r.start, std::cmp::Reverse(r.end)));
        let mut disjoint: Vec<Range<usize>> = Vec::new();
        for r in ranges {
            if disjoint.last().is_none_or(|last| r.start >= last.end) {
                disjoint.push(r);
            }
        }
        let mut xml = self.xml.clone();
        for r in disjoint.into_iter().rev() {
            xml.replace_range(r, "");
        }
        let filtered = Self::parse(xml)?;
        let t = Tree::parse(&filtered.xml)?;
        if !hierarchy || t.property(LR, "hierarchicalSubject").is_some() {
            return Ok(filtered);
        }
        let keywords = match t.property(DC, "subject") {
            Some(Property::Scalar(s)) => vec![s.to_owned()],
            Some(Property::Node(n)) => t.items(n).iter().map(|n| keyword_value(&t, n)).collect(),
            None => Vec::new(),
        };
        if keywords.is_empty() {
            return Ok(filtered);
        }
        Self::parse(t.replace(
            &filtered.xml,
            &[],
            &container("lr:hierarchicalSubject", "Bag", &keywords),
        )?)
    }
}

// Carry ancestor namespace bindings with each raw description, so collisions
// between packet prefixes cannot change the meaning of retained RDF structures.
fn standalone_descriptions(xml: &str) -> EngineResult<String> {
    use quick_xml::{NsReader, events::Event, name::PrefixDeclaration};
    let tree = Tree::parse(xml)?;
    let descriptions: std::collections::BTreeMap<_, _> = tree
        .descriptions()
        .map(|n| (n.span.start, n.span.end))
        .collect();
    let mut reader = NsReader::from_str(xml);
    let mut result = String::new();
    loop {
        let start = reader.buffer_position() as usize;
        match reader.read_event().map_err(error)? {
            Event::Start(e) | Event::Empty(e) => {
                if let Some(&end) = descriptions.get(&start) {
                    let local = e
                        .attributes()
                        .map(|a| a.map(|a| a.key.as_ref().to_vec()).map_err(error))
                        .collect::<EngineResult<Vec<_>>>()?;
                    let mut declarations = String::new();
                    // Explicitly reset the default namespace when absent.
                    if !local.iter().any(|key| key == b"xmlns")
                        && !reader
                            .prefixes()
                            .any(|(p, _)| matches!(p, PrefixDeclaration::Default))
                    {
                        declarations.push_str(" xmlns=\"\"");
                    }
                    for (prefix, namespace) in reader.prefixes() {
                        let key = match prefix {
                            PrefixDeclaration::Default => "xmlns".to_owned(),
                            PrefixDeclaration::Named(p) => {
                                format!("xmlns:{}", std::str::from_utf8(p).map_err(error)?)
                            }
                        };
                        if !local.iter().any(|k| k == key.as_bytes()) {
                            declarations.push_str(&format!(
                                " {key}=\"{}\"",
                                escape(std::str::from_utf8(namespace.as_ref()).map_err(error)?)
                            ));
                        }
                    }
                    let insert = start + 1 + e.name().as_ref().len();
                    result.push_str(&xml[start..insert]);
                    result.push_str(&declarations);
                    result.push_str(&xml[insert..end]);
                }
            }
            Event::Eof => break,
            _ => {}
        }
    }
    Ok(result)
}

fn people_path(path: &str) -> bool {
    path.split('|')
        .next()
        .is_some_and(|s| matches!(s.trim().to_lowercase().as_str(), "people" | "persons"))
}

fn keyword_property(ns: &str, key: &str) -> bool {
    (ns == DC && key == "subject") || (ns == LR && key == "hierarchicalSubject")
}

fn keyword_value(tree: &Tree, node: &Node) -> String {
    // Qualified XMP values may use an explicit resource node instead of
    // parseType="Resource". Unwrap only RDF descriptions, never arbitrary
    // qualifier descendants whose text is not the property's value.
    let mut node = node;
    while let Some(resource) = node
        .children
        .iter()
        .map(|&i| &tree.nodes[i])
        .find(|n| n.ns == RDF && n.local == "Description")
    {
        node = resource;
    }
    node.children
        .iter()
        .map(|&i| &tree.nodes[i])
        .find(|n| n.ns == RDF && n.local == "value")
        .map(|n| n.text.clone())
        .or_else(|| {
            node.attrs
                .iter()
                .find(|a| a.ns == RDF && a.local == "value")
                .map(|a| a.value.clone())
        })
        .unwrap_or_else(|| node.text.clone())
}

pub(crate) fn keyword_values(tree: &Tree, ns: &str, key: &str) -> Vec<String> {
    match tree.property(ns, key) {
        Some(Property::Scalar(s)) => vec![s.into()],
        Some(Property::Node(n)) => {
            let items = tree.items(n);
            if items.is_empty() {
                let value = keyword_value(tree, n);
                if value.is_empty() {
                    Vec::new()
                } else {
                    vec![value]
                }
            } else {
                items.iter().map(|n| keyword_value(tree, n)).collect()
            }
        }
        None => Vec::new(),
    }
}

fn append_keywords(
    packet: &XmpPacket,
    ns: &str,
    key: &str,
    name: &str,
    additions: &[String],
) -> EngineResult<XmpPacket> {
    if additions.is_empty() {
        return Ok(packet.clone());
    }
    let tree = Tree::parse(&packet.xml)?;
    if let Some(Property::Node(n)) = tree.property(ns, key)
        && let Some(bag) = n
            .children
            .iter()
            .map(|&i| &tree.nodes[i])
            .find(|n| n.ns == RDF && matches!(n.local.as_str(), "Bag" | "Seq" | "Alt"))
    {
        // Insert beside existing list items, preserving qualified RDF values,
        // property-local namespace declarations and all foreign qualifiers.
        let items: String = additions
            .iter()
            .map(|v| format!("<rdf:li xmlns:rdf=\"{RDF}\">{}</rdf:li>", escape(v)))
            .collect();
        let mut xml = packet.xml.clone();
        if xml[bag.close..].starts_with("/>") {
            let qname = xml[bag.span.start + 1..]
                .split([' ', '\t', '\r', '\n', '/', '>'])
                .next()
                .unwrap()
                .to_owned();
            xml.replace_range(bag.close..bag.close + 2, &format!(">{items}</{qname}>"));
        } else {
            xml.insert_str(bag.close, &items);
        }
        return XmpPacket::parse(xml);
    }
    let mut values = keyword_values(&tree, ns, key);
    values.extend_from_slice(additions);
    XmpPacket::parse(tree.replace(&packet.xml, &[(ns, key)], &container(name, "Bag", &values))?)
}
