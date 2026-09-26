//! Namespace-aware XML indexing. Edits splice only owned properties, leaving foreign XML raw.
use engine_api::error::{EngineError, EngineResult};
use quick_xml::{Reader, events::Event};
use std::{collections::BTreeMap, ops::Range};

pub(crate) const RDF: &str = "http://www.w3.org/1999/02/22-rdf-syntax-ns#";
pub(crate) const XMP: &str = engine_api::recipe::crs::XMP_NAMESPACE;
pub(crate) const DM: &str = engine_api::recipe::crs::XMP_DM_NAMESPACE;
pub(crate) const DC: &str = "http://purl.org/dc/elements/1.1/";
pub(crate) const LR: &str = "http://ns.adobe.com/lightroom/1.0/";
pub(crate) const CRS: &str = engine_api::recipe::crs::CRS_NAMESPACE;
pub(crate) const PRIVATE: &str = engine_api::recipe::crs::TS_NAMESPACE;
/// IPTC Core (2021+): `AltTextAccessibility` lives here.
pub(crate) const IPTC_CORE: &str = "http://iptc.org/std/Iptc4xmpCore/1.0/xmlns/";

pub(crate) fn error(e: impl std::fmt::Display) -> EngineError {
    EngineError::Decode {
        format: "xmp".into(),
        message: e.to_string(),
    }
}
pub(crate) fn escape(s: &str) -> String {
    // Literal CR is normalized to LF by XML readers; a character reference is not.
    quick_xml::escape::escape(s).replace('\r', "&#13;")
}
pub(crate) fn text(name: &str, value: &str) -> String {
    format!("<{name}>{}</{name}>", escape(value))
}
pub(crate) fn container(name: &str, kind: &str, values: &[String]) -> String {
    let items: String = values
        .iter()
        .map(|v| {
            if kind == "Alt" {
                format!("<rdf:li xml:lang=\"x-default\">{}</rdf:li>", escape(v))
            } else {
                text("rdf:li", v)
            }
        })
        .collect();
    format!("<{name}><rdf:{kind}>{items}</rdf:{kind}></{name}>")
}
pub(crate) fn description(body: &str) -> String {
    format!(
        r#"<rdf:Description rdf:about="" xmlns:rdf="{RDF}" xmlns:xmp="{XMP}" xmlns:xmpDM="{DM}" xmlns:dc="{DC}" xmlns:lr="{LR}" xmlns:crs="{CRS}" xmlns:ts="{PRIVATE}" xmlns:Iptc4xmpCore="{IPTC_CORE}">{body}</rdf:Description>"#
    )
}
pub(crate) fn packet(body: &str) -> String {
    format!(r#"<?xpacket begin="" id="W5M0MpCehiHzreSzNTczkc9d"?><x:xmpmeta xmlns:x="adobe:ns:meta/"><rdf:RDF xmlns:rdf="{RDF}">{}</rdf:RDF></x:xmpmeta><?xpacket end="w"?>"#, description(body)).replace('\u{1}', "\u{feff}")
}

#[derive(Clone, Debug)]
pub(crate) struct Attr {
    pub ns: String,
    pub local: String,
    pub value: String,
    pub span: Range<usize>,
}
#[derive(Clone, Debug)]
pub(crate) struct Node {
    pub ns: String,
    pub local: String,
    pub attrs: Vec<Attr>,
    pub parent: Option<usize>,
    pub children: Vec<usize>,
    pub text: String,
    pub span: Range<usize>,
    pub close: usize,
}
#[derive(Debug)]
pub(crate) struct Tree {
    pub nodes: Vec<Node>,
    pub rdf: usize,
}
fn resolve(
    name: &str,
    ns: &BTreeMap<String, String>,
    attr: bool,
) -> EngineResult<(String, String)> {
    if let Some((prefix, local)) = name.split_once(':') {
        let uri = ns
            .get(prefix)
            .ok_or_else(|| error(format!("undeclared prefix {prefix}")))?;
        Ok((uri.clone(), local.into()))
    } else {
        Ok((
            if attr {
                String::new()
            } else {
                ns.get("").cloned().unwrap_or_default()
            },
            name.into(),
        ))
    }
}
// Locate lexical attribute ranges so removing an owned attribute does not reserialize foreign ones.
fn attr_ranges(tag: &str, offset: usize) -> EngineResult<Vec<Range<usize>>> {
    let b = tag.as_bytes();
    let mut p = 1;
    while p < b.len() && !b[p].is_ascii_whitespace() && b[p] != b'>' && b[p] != b'/' {
        p += 1;
    }
    let mut out = Vec::new();
    loop {
        while p < b.len() && b[p].is_ascii_whitespace() {
            p += 1;
        }
        if p >= b.len() || matches!(b[p], b'/' | b'>') {
            break;
        }
        let start = p;
        while p < b.len() && b[p] != b'=' {
            p += 1;
        }
        p += 1;
        while p < b.len() && b[p].is_ascii_whitespace() {
            p += 1;
        }
        let quote = *b.get(p).ok_or_else(|| error("missing attribute quote"))?;
        if quote != b'\'' && quote != b'"' {
            return Err(error("unquoted attribute"));
        }
        p += 1;
        while p < b.len() && b[p] != quote {
            p += 1;
        }
        if p >= b.len() {
            return Err(error("unterminated attribute"));
        }
        p += 1;
        out.push(offset + start..offset + p);
    }
    Ok(out)
}
impl Tree {
    pub fn parse(xml: &str) -> EngineResult<Self> {
        if xml.len() > 64 * 1024 * 1024 {
            return Err(error("packet exceeds 64 MiB"));
        }
        let mut reader = Reader::from_str(xml);
        let mut nodes: Vec<Node> = Vec::new();
        let mut stack: Vec<(usize, BTreeMap<String, String>)> = Vec::new();
        let mut roots = 0;
        loop {
            let start = reader.buffer_position() as usize;
            let event = reader.read_event().map_err(error)?;
            let end = reader.buffer_position() as usize;
            match event {
                Event::Start(ref e) | Event::Empty(ref e) => {
                    if stack.len() >= 128 {
                        return Err(error("XML nesting exceeds 128"));
                    }
                    if stack.is_empty() {
                        roots += 1;
                    }
                    let mut namespaces =
                        stack.last().map(|(_, n)| n.clone()).unwrap_or_else(|| {
                            BTreeMap::from([(
                                "xml".into(),
                                "http://www.w3.org/XML/1998/namespace".into(),
                            )])
                        });
                    let mut attrs = Vec::new();
                    let ranges = attr_ranges(&xml[start..end], start)?;
                    for (a, range) in e.attributes().zip(ranges) {
                        let a = a.map_err(error)?;
                        let name = std::str::from_utf8(a.key.as_ref())
                            .map_err(error)?
                            .to_owned();
                        let value = a
                            .decode_and_unescape_value(reader.decoder())
                            .map_err(error)?
                            .into_owned();
                        if name == "xmlns" {
                            namespaces.insert(String::new(), value.clone());
                        }
                        if let Some(prefix) = name.strip_prefix("xmlns:") {
                            namespaces.insert(prefix.into(), value.clone());
                        }
                        attrs.push((name, value, range));
                    }
                    let attrs = attrs
                        .into_iter()
                        .filter(|(n, _, _)| n != "xmlns" && !n.starts_with("xmlns:"))
                        .map(|(name, value, span)| {
                            let (ns, local) = resolve(&name, &namespaces, true)?;
                            Ok(Attr {
                                ns,
                                local,
                                value,
                                span,
                            })
                        })
                        .collect::<EngineResult<Vec<_>>>()?;
                    let name = e.name();
                    let (ns, local) = resolve(
                        std::str::from_utf8(name.as_ref()).map_err(error)?,
                        &namespaces,
                        false,
                    )?;
                    let index = nodes.len();
                    let parent = stack.last().map(|(i, _)| *i);
                    if let Some(p) = parent {
                        nodes[p].children.push(index);
                    }
                    nodes.push(Node {
                        ns,
                        local,
                        attrs,
                        parent,
                        children: Vec::new(),
                        text: String::new(),
                        span: start..end,
                        close: end - 2,
                    });
                    if matches!(event, Event::Start(_)) {
                        stack.push((index, namespaces));
                    }
                }
                Event::End(_) => {
                    let (i, _) = stack.pop().ok_or_else(|| error("unexpected close"))?;
                    nodes[i].span.end = end;
                    nodes[i].close = start;
                }
                Event::Text(t) => {
                    let value = t.xml_content().map_err(error)?;
                    if let Some((i, _)) = stack.last() {
                        nodes[*i].text.push_str(&value);
                    } else if !value.trim().is_empty() {
                        return Err(error("text outside root"));
                    }
                }
                Event::CData(t) => {
                    let (i, _) = stack.last().ok_or_else(|| error("CDATA outside root"))?;
                    nodes[*i].text.push_str(&t.decode().map_err(error)?);
                }
                Event::GeneralRef(r) => {
                    let (i, _) = stack.last().ok_or_else(|| error("entity outside root"))?;
                    let entity = format!("&{};", r.decode().map_err(error)?);
                    nodes[*i]
                        .text
                        .push_str(&quick_xml::escape::unescape(&entity).map_err(error)?);
                }
                Event::DocType(_) => return Err(error("DTDs are not supported in XMP")),
                Event::Eof => break,
                _ => {}
            }
        }
        if !stack.is_empty() || roots != 1 {
            return Err(error("XMP must have one complete root"));
        }
        let rdf = nodes
            .iter()
            .position(|n| n.ns == RDF && n.local == "RDF")
            .ok_or_else(|| error("missing rdf:RDF"))?;
        Ok(Self { nodes, rdf })
    }
    pub fn descriptions(&self) -> impl Iterator<Item = &Node> {
        self.nodes
            .iter()
            .filter(|n| n.parent == Some(self.rdf) && n.ns == RDF && n.local == "Description")
    }
    pub fn property(&self, ns: &str, local: &str) -> Option<Property<'_>> {
        for desc in self.descriptions() {
            if let Some(a) = desc.attrs.iter().find(|a| a.ns == ns && a.local == local) {
                return Some(Property::Scalar(&a.value));
            }
            if let Some(n) = desc
                .children
                .iter()
                .map(|i| &self.nodes[*i])
                .find(|n| n.ns == ns && n.local == local)
            {
                return Some(Property::Node(n));
            }
        }
        None
    }
    pub fn items<'a>(&'a self, n: &'a Node) -> Vec<&'a Node> {
        n.children
            .iter()
            .map(|i| &self.nodes[*i])
            .filter(|n| n.ns == RDF && matches!(n.local.as_str(), "Seq" | "Bag" | "Alt"))
            .flat_map(|n| n.children.iter().map(|i| &self.nodes[*i]))
            .filter(|n| n.ns == RDF && n.local == "li")
            .collect()
    }
    pub fn value(&self, ns: &str, name: &str) -> Option<String> {
        self.property(ns, name).map(|p| match p {
            Property::Scalar(s) => s.into(),
            Property::Node(n) => {
                let items = self.items(n);
                items
                    .iter()
                    .find(|n| {
                        n.attrs.iter().any(|a| {
                            a.ns == "http://www.w3.org/XML/1998/namespace"
                                && a.local == "lang"
                                && a.value == "x-default"
                        })
                    })
                    .or_else(|| items.first())
                    .map_or_else(|| n.text.clone(), |n| n.text.clone())
            }
        })
    }
    pub fn list(&self, ns: &str, name: &str) -> Vec<String> {
        match self.property(ns, name) {
            Some(Property::Node(n)) => self.items(n).iter().map(|n| n.text.clone()).collect(),
            Some(Property::Scalar(s)) => vec![s.into()],
            None => Vec::new(),
        }
    }
    pub fn replace(&self, xml: &str, owned: &[(&str, &str)], body: &str) -> EngineResult<String> {
        let mut ranges = Vec::new();
        for desc in self.descriptions() {
            for a in &desc.attrs {
                if owned.contains(&(a.ns.as_str(), a.local.as_str())) {
                    ranges.push((a.span.clone(), String::new()));
                }
            }
            for i in &desc.children {
                let n = &self.nodes[*i];
                if owned.contains(&(n.ns.as_str(), n.local.as_str())) {
                    ranges.push((n.span.clone(), String::new()));
                }
            }
        }
        let rdf = &self.nodes[self.rdf];
        if xml[rdf.span.clone()].ends_with("/>") {
            return Err(error("cannot update an empty rdf:RDF; use a full packet"));
        }
        ranges.push((rdf.close..rdf.close, description(body)));
        ranges.sort_by_key(|(r, _)| r.start);
        let mut result = xml.to_owned();
        for (r, v) in ranges.into_iter().rev() {
            result.replace_range(r, &v);
        }
        Ok(result)
    }
}
pub(crate) enum Property<'a> {
    Scalar(&'a str),
    Node(&'a Node),
}
