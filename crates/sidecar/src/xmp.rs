use crate::{MarkPreset, Sidecar, XmpPacket, xml::*};
use engine_api::{
    error::EngineResult,
    recipe::{CrsKey, Decision, Selection},
};
use std::collections::BTreeMap;

/// Dublin Core/IPTC fields. Titles, captions and rights use the x-default language.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Metadata {
    pub title: String,
    pub description: String,
    pub creators: Vec<String>,
    pub copyright: String,
    pub keywords: Vec<String>,
    pub hierarchical_keywords: Vec<String>,
}
const METADATA: &[(&str, &str)] = &[
    (XMP, "Rating"),
    (XMP, "Label"),
    (DM, "pick"),
    (DM, "good"),
    (DC, "title"),
    (DC, "description"),
    (DC, "creator"),
    (DC, "rights"),
    (DC, "subject"),
    (LR, "hierarchicalSubject"),
    (PRIVATE, "Mark"),
    (PRIVATE, "MarkLabel"),
];
impl XmpPacket {
    pub fn metadata(&self) -> EngineResult<Metadata> {
        let t = Tree::parse(&self.xml)?;
        Ok(Metadata {
            title: t.value(DC, "title").unwrap_or_default(),
            description: t.value(DC, "description").unwrap_or_default(),
            copyright: t.value(DC, "rights").unwrap_or_default(),
            creators: t.list(DC, "creator"),
            keywords: t.list(DC, "subject"),
            hierarchical_keywords: t.list(LR, "hierarchicalSubject"),
        })
    }
    /// Replace owned metadata only. Foreign attributes and elements stay byte-for-byte intact.
    pub fn with_metadata(
        &self,
        selection: &Selection,
        metadata: &Metadata,
        preset: &MarkPreset,
    ) -> EngineResult<Self> {
        let tree = Tree::parse(&self.xml)?;
        Self::parse(tree.replace(
            &self.xml,
            METADATA,
            &metadata_body(selection, metadata, preset),
        )?)
    }
    pub(crate) fn read_selection(&self) -> EngineResult<Selection> {
        let t = Tree::parse(&self.xml)?;
        let label = t.value(XMP, "Label");
        let mut selection = Sidecar::selection_from_xmp(
            t.value(XMP, "Rating").and_then(|s| s.trim().parse().ok()),
            t.value(DM, "pick").as_deref(),
            label.as_deref(),
        );
        // Retain an exact custom mark even with a non-injective label preset. Ignore stale
        // private state if another application changed the visible label.
        if t.value(PRIVATE, "MarkLabel") == label
            && let Some(mark) = t.value(PRIVATE, "Mark")
        {
            selection.mark = Some(engine_api::recipe::Mark::new(mark));
        }
        Ok(selection)
    }
    pub(crate) fn read_crs(&self) -> EngineResult<BTreeMap<CrsKey, String>> {
        let tree = Tree::parse(&self.xml)?;
        Ok(CrsKey::ALL
            .iter()
            .filter_map(|&key| {
                tree.property(key.namespace().uri(), key.xmp_name())
                    .map(|p| {
                        (
                            key,
                            match p {
                                Property::Scalar(s) => s.into(),
                                Property::Node(n) if n.children.is_empty() => n.text.clone(),
                                Property::Node(n) => self.xml[n.span.clone()].to_owned(),
                            },
                        )
                    })
            })
            .collect())
    }
}
pub(crate) fn metadata_body(
    selection: &Selection,
    metadata: &Metadata,
    preset: &MarkPreset,
) -> String {
    let (rating, flag, label) = Sidecar::selection_xmp(selection, Some(preset));
    let mut body = String::new();
    if selection.decision != Decision::Undecided {
        body += &text("xmp:Rating", &rating.to_string());
    }
    if let Some(flag) = flag {
        body += &text("xmpDM:pick", if flag == "reject" { "-1" } else { "1" });
        body += &text(
            "xmpDM:good",
            if flag == "reject" { "False" } else { "True" },
        );
    }
    if let Some(label) = label {
        body += &text("xmp:Label", &label);
        if let Some(mark) = &selection.mark {
            body += &text("ts:Mark", &mark.0);
            body += &text("ts:MarkLabel", &label);
        }
    }
    for (key, value) in [
        ("dc:title", &metadata.title),
        ("dc:description", &metadata.description),
        ("dc:rights", &metadata.copyright),
    ] {
        if !value.is_empty() {
            body += &container(key, "Alt", std::slice::from_ref(value));
        }
    }
    for (key, kind, values) in [
        ("dc:creator", "Seq", &metadata.creators),
        ("dc:subject", "Bag", &metadata.keywords),
        (
            "lr:hierarchicalSubject",
            "Bag",
            &metadata.hierarchical_keywords,
        ),
    ] {
        if !values.is_empty() {
            body += &container(key, kind, values);
        }
    }
    body
}
