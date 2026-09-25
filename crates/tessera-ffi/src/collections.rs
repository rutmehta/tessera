//! Library (library.json) over UniFFI: albums, album groups, smart albums,
//! sidebar order, saved-search rules with located diagnostics, and faceted
//! search. Every call reads the document fresh and writes it atomically, so
//! a `CullSession` sharing the same file (basket) always sees current state.
//!
//! Safe delete: nothing here opens, moves or deletes photos or sidecars.
use crate::{Engine, Result, failure, parse_id};
use engine_api::id::ImageId;
use library::{Diagnostic, Library, NodeKind, SavedSearch};
use serde_json::Value;
use std::{
    path::PathBuf,
    sync::{Arc, Mutex},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum LibraryNodeKind {
    Group,
    Album,
    SmartAlbum,
}

/// One sidebar entry, listed depth-first in sidebar order.
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct LibraryNode {
    pub id: i64,
    pub kind: LibraryNodeKind,
    pub name: String,
    pub parent: Option<i64>,
    pub depth: u32,
    /// Album map key (the basket handle); None for groups and smart albums.
    pub handle: Option<String>,
    /// Manual album members; zero otherwise.
    pub image_count: u32,
    /// Smart album rule in the text grammar.
    pub rule: Option<String>,
    /// Smart album inside a group searches only that group's albums.
    pub scoped: bool,
}

/// UTF-8 byte range into the checked text. `start == end` marks a position
/// (for example "expected expression" at the end of the input).
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct RuleDiagnostic {
    pub start: u32,
    pub end: u32,
    pub message: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum RuleItemKind {
    /// AND group
    All,
    /// OR group
    Any,
    /// NOT (none of the children)
    Not,
    Rule,
}

/// A saved-search AST in preorder. Groups carry their child count; rules
/// carry field, operator and value (numbers as their decimal text).
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct RuleItem {
    pub kind: RuleItemKind,
    pub children: u32,
    pub field: String,
    pub op: String,
    pub value: String,
}

#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct RuleCheck {
    /// Text for the rule: normalized when it came from items, else the input.
    pub text: String,
    pub diagnostic: Option<RuleDiagnostic>,
    /// Parsed tree (empty when the text does not parse or is empty).
    pub items: Vec<RuleItem>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum SearchScope {
    All,
    Album { id: i64 },
    SmartAlbum { id: i64 },
    Group { id: i64 },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, uniffi::Enum)]
pub enum FacetField {
    Decision,
    /// Keep grade "1".."3".
    Grade,
    Mark,
    Camera,
    Lens,
    Keyword,
    /// "none" (in no album), "any", or an album handle.
    Album,
    /// `YYYY[-MM[-DD]]` or an inclusive range `from..to`.
    Date,
}

/// Values of one field are ORed; fields are ANDed with each other and the text.
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct FacetFilter {
    pub field: FacetField,
    pub values: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct SearchRequest {
    /// Free text in the saved-search grammar (may be empty).
    pub text: String,
    pub filters: Vec<FacetFilter>,
    pub scope: SearchScope,
    /// Limit to images under this folder (recursive); None for the whole catalog.
    pub folder: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct FacetCount {
    pub value: String,
    pub count: u32,
}

/// Each facet counts over the result of every *other* active filter, so the
/// alternatives within a facet stay visible (and their counts stay live).
#[derive(Clone, Debug, Default, PartialEq, Eq, uniffi::Record)]
pub struct SearchFacets {
    pub decisions: Vec<FacetCount>,
    pub grades: Vec<FacetCount>,
    pub marks: Vec<FacetCount>,
    pub cameras: Vec<FacetCount>,
    pub lenses: Vec<FacetCount>,
    pub keywords: Vec<FacetCount>,
    pub in_any_album: u32,
    pub in_no_album: u32,
}

#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct SearchResult {
    /// Matching images: album order for an album scope, else capture time.
    pub image_ids: Vec<String>,
    pub facets: SearchFacets,
    /// Problem with `text`; the rest of the request still applied.
    pub diagnostic: Option<RuleDiagnostic>,
    /// Grammar text equivalent to text + filters + album/smart-album scope,
    /// for "Save as Smart Album". Empty when nothing narrows the search.
    pub rule: String,
    /// Group to place (and scope) a saved smart album in.
    pub group: Option<i64>,
}

/// library.json plus the engine's catalog. Create with `Engine::open_library`.
#[derive(uniffi::Object)]
pub struct LibraryStore {
    pub(crate) engine: Arc<Engine>,
    pub(crate) path: PathBuf,
    pub(crate) guard: Mutex<()>,
}

#[uniffi::export]
impl Engine {
    /// Opens (without creating) the library document at `path`, conventionally
    /// `<folder>/library.json`, and aligns the catalog's keyword hierarchy.
    pub fn open_library(self: Arc<Self>, path: String) -> Result<Arc<LibraryStore>> {
        let library = Library::read(&path)?;
        self.lock()?
            .index
            .sync_keyword_tree(&library.keyword_pairs())?;
        Ok(Arc::new(LibraryStore {
            engine: self,
            path: path.into(),
            guard: Mutex::new(()),
        }))
    }
}

impl LibraryStore {
    pub(crate) fn read(&self) -> Result<Library> {
        Ok(Library::read(&self.path)?)
    }
    /// Read-modify-write under the store's lock.
    pub(crate) fn edit<T>(&self, f: impl FnOnce(&mut Library) -> Result<T>) -> Result<T> {
        let _guard = self.guard.lock().map_err(failure)?;
        let mut library = Library::read(&self.path)?;
        let out = f(&mut library)?;
        library.write(&self.path)?;
        Ok(out)
    }
}

fn ids(values: &[String]) -> Result<Vec<ImageId>> {
    values.iter().map(|id| parse_id(id)).collect()
}

fn diagnostic(d: Diagnostic) -> RuleDiagnostic {
    RuleDiagnostic {
        start: d.start as u32,
        end: d.end as u32,
        message: d.message,
    }
}

fn parse_rule(text: &str) -> Result<SavedSearch> {
    SavedSearch::parse_diagnostic(text).map_err(|d| failure(d.message))
}

// --- Rule text rendering (readable, re-parses to an equivalent AST) ---

const KEYWORDS: [&str; 3] = ["AND", "OR", "NOT"];

fn bare(s: &str) -> bool {
    !s.is_empty()
        && !KEYWORDS.iter().any(|k| k.eq_ignore_ascii_case(s))
        && s.chars()
            .all(|c| c.is_alphanumeric() || matches!(c, '-' | '_' | '.' | '/' | '\'' | '*' | '+'))
}

fn quote(s: &str) -> String {
    if bare(s) {
        s.into()
    } else {
        serde_json::to_string(s).unwrap_or_else(|_| "\"\"".into())
    }
}

fn leaf(criteria: &str, operation: &str, value: &Value) -> String {
    let value = match value {
        Value::String(s) => quote(s),
        other => other.to_string(),
    };
    if criteria == "text" && matches!(operation, ":" | "=") {
        value
    } else {
        format!("{criteria}{operation}{value}")
    }
}

/// AND is explicit; nested groups are parenthesized; NOT binds tightest.
pub(crate) fn render(node: &SavedSearch, root: bool) -> String {
    let join = |v: &[SavedSearch], sep: &str| {
        v.iter()
            .map(|n| render(n, false))
            .collect::<Vec<_>>()
            .join(sep)
    };
    match node {
        SavedSearch::Rule {
            criteria,
            operation,
            value,
        } => leaf(criteria, operation, value),
        SavedSearch::All(v) | SavedSearch::Any(v) => {
            let body = join(
                v,
                if matches!(node, SavedSearch::All(_)) {
                    " AND "
                } else {
                    " OR "
                },
            );
            if root { body } else { format!("({body})") }
        }
        SavedSearch::None(v) if v.len() == 1 => format!("NOT {}", render(&v[0], false)),
        SavedSearch::None(v) => format!("NOT ({})", join(v, " OR ")),
    }
}

/// Readable text when it round-trips exactly, else the lossless canonical form.
pub(crate) fn rule_text(search: &SavedSearch) -> String {
    let text = render(search, true);
    if text.parse::<SavedSearch>().as_ref() == Ok(search) {
        text
    } else {
        search.to_string()
    }
}

fn flatten(node: &SavedSearch, out: &mut Vec<RuleItem>) {
    let group = |kind, n: usize| RuleItem {
        kind,
        children: n as u32,
        field: String::new(),
        op: String::new(),
        value: String::new(),
    };
    match node {
        SavedSearch::Rule {
            criteria,
            operation,
            value,
        } => out.push(RuleItem {
            kind: RuleItemKind::Rule,
            children: 0,
            field: criteria.clone(),
            op: operation.clone(),
            value: match value {
                Value::String(s) => s.clone(),
                other => other.to_string(),
            },
        }),
        SavedSearch::All(v) | SavedSearch::Any(v) | SavedSearch::None(v) => {
            out.push(group(
                match node {
                    SavedSearch::All(_) => RuleItemKind::All,
                    SavedSearch::Any(_) => RuleItemKind::Any,
                    _ => RuleItemKind::Not,
                },
                v.len(),
            ));
            for child in v {
                flatten(child, out);
            }
        }
    }
}

/// Renders editor items to text without validating leaves (so the parser can
/// locate their errors in the rendered text). Numeric fields keep numbers bare.
fn render_items(items: &[RuleItem]) -> Result<String> {
    fn next(it: &mut std::slice::Iter<'_, RuleItem>, root: bool, depth: usize) -> Result<String> {
        if depth > 64 {
            return Err(failure("rule nesting is too deep"));
        }
        let item = it.next().ok_or_else(|| failure("incomplete rule items"))?;
        if item.kind == RuleItemKind::Rule {
            let numeric = matches!(item.field.as_str(), "rating" | "grade" | "focus")
                && item.value.parse::<f64>().is_ok();
            let value = if numeric {
                item.value.clone()
            } else {
                quote(&item.value)
            };
            return Ok(
                if item.field == "text" && matches!(item.op.as_str(), ":" | "=") {
                    value
                } else {
                    format!("{}{}{value}", item.field, item.op)
                },
            );
        }
        let children = (0..item.children)
            .map(|_| next(it, false, depth + 1))
            .collect::<Result<Vec<_>>>()?;
        Ok(match item.kind {
            RuleItemKind::Not if children.len() == 1 => format!("NOT {}", children[0]),
            RuleItemKind::Not => format!("NOT ({})", children.join(" OR ")),
            kind => {
                let body = children.join(if kind == RuleItemKind::All {
                    " AND "
                } else {
                    " OR "
                });
                if root || children.len() < 2 {
                    body
                } else {
                    format!("({body})")
                }
            }
        })
    }
    if items.is_empty() {
        return Ok(String::new());
    }
    let mut it = items.iter();
    let text = next(&mut it, true, 0)?;
    if it.next().is_some() {
        return Err(failure("trailing rule items"));
    }
    Ok(text)
}

impl LibraryStore {
    fn check(&self, library: &Library, text: String) -> RuleCheck {
        if text.trim().is_empty() {
            return RuleCheck {
                text,
                diagnostic: Some(RuleDiagnostic {
                    start: 0,
                    end: 0,
                    message: "Enter at least one condition".into(),
                }),
                items: vec![],
            };
        }
        match SavedSearch::parse_diagnostic(&text) {
            Err(d) => RuleCheck {
                text,
                diagnostic: Some(diagnostic(d)),
                items: vec![],
            },
            Ok(search) => {
                let mut items = Vec::new();
                flatten(&search, &mut items);
                let diagnostic = library
                    .compile_search(&search)
                    .err()
                    .map(|e| diagnostic(Diagnostic::whole(&text, &e)));
                RuleCheck {
                    text,
                    diagnostic,
                    items,
                }
            }
        }
    }

    fn scope_group(library: &Library, scope: SearchScope) -> Option<i64> {
        match scope {
            SearchScope::Group { id } => Some(id),
            SearchScope::SmartAlbum { id } => library
                .smart_albums
                .iter()
                .find(|s| s.id == id && s.scoped)
                .and_then(|s| s.parent),
            _ => None,
        }
    }
}

fn facet_rule(field: FacetField, value: &str) -> Result<SavedSearch> {
    let (criteria, value) = match field {
        FacetField::Decision => ("decision", Value::String(value.into())),
        FacetField::Grade => (
            "rating",
            Value::Number(
                value
                    .parse::<u8>()
                    .map_err(|_| failure("grade filter must be 1, 2 or 3"))?
                    .into(),
            ),
        ),
        FacetField::Mark => ("mark", Value::String(value.into())),
        FacetField::Camera => ("camera", Value::String(value.into())),
        FacetField::Lens => ("lens", Value::String(value.into())),
        FacetField::Keyword => ("keyword", Value::String(value.into())),
        FacetField::Album => ("album", Value::String(value.into())),
        FacetField::Date => ("date", Value::String(value.into())),
    };
    let rule = SavedSearch::Rule {
        criteria: criteria.into(),
        operation: ":".into(),
        value,
    };
    // Validate through the grammar so malformed values are reported, not ignored.
    rule_text(&rule)
        .parse::<SavedSearch>()
        .map_err(|e| failure(format!("{criteria} filter: {e}")))?;
    Ok(rule)
}

fn all(mut clauses: Vec<SavedSearch>) -> Option<SavedSearch> {
    match clauses.len() {
        0 => None,
        1 => clauses.pop(),
        _ => Some(SavedSearch::All(clauses)),
    }
}

fn counts(values: Vec<(String, u64)>) -> Vec<FacetCount> {
    let mut out: Vec<_> = values
        .into_iter()
        .map(|(value, count)| FacetCount {
            value,
            count: count.min(u32::MAX as u64) as u32,
        })
        .collect();
    out.sort_by(|a, b| b.count.cmp(&a.count).then_with(|| a.value.cmp(&b.value)));
    out
}

#[uniffi::export]
impl LibraryStore {
    pub fn path(&self) -> String {
        self.path.to_string_lossy().into_owned()
    }

    /// Sidebar entries depth-first, in sidebar order.
    pub fn nodes(&self) -> Result<Vec<LibraryNode>> {
        let library = self.read()?;
        Ok(library
            .sidebar()
            .into_iter()
            .map(|n| {
                let smart = library.smart_albums.iter().find(|s| s.id == n.id);
                LibraryNode {
                    id: n.id,
                    kind: match n.kind {
                        NodeKind::Group => LibraryNodeKind::Group,
                        NodeKind::Album => LibraryNodeKind::Album,
                        NodeKind::SmartAlbum => LibraryNodeKind::SmartAlbum,
                    },
                    image_count: library
                        .album_by_id(n.id)
                        .map_or(0, |(_, a)| a.images.len() as u32),
                    rule: smart.map(|s| rule_text(&s.search)),
                    scoped: smart.is_some_and(|s| s.scoped && s.parent.is_some()),
                    name: n.name,
                    parent: n.parent,
                    depth: n.depth as u32,
                    handle: n.handle,
                }
            })
            .collect())
    }

    pub fn create_album(&self, name: String, parent: Option<i64>) -> Result<i64> {
        self.edit(|l| Ok(l.create_album(name.trim(), parent)?))
    }
    pub fn create_group(&self, name: String, parent: Option<i64>) -> Result<i64> {
        self.edit(|l| Ok(l.create_group(name.trim(), parent)?))
    }
    /// Fails (with the diagnostic message) when the rule does not parse or
    /// does not compile against this library.
    pub fn create_smart_album(
        &self,
        name: String,
        rule: String,
        parent: Option<i64>,
        scoped: bool,
    ) -> Result<i64> {
        let search = parse_rule(&rule)?;
        self.edit(|l| Ok(l.create_smart_album(name.trim(), search, parent, scoped)?))
    }
    pub fn update_smart_album(
        &self,
        id: i64,
        rule: Option<String>,
        scoped: Option<bool>,
    ) -> Result<()> {
        let search = rule.as_deref().map(parse_rule).transpose()?;
        self.edit(|l| Ok(l.update_smart_album(id, search, scoped)?))
    }
    pub fn rename(&self, id: i64, name: String) -> Result<()> {
        self.edit(|l| Ok(l.rename_node(id, name.trim())?))
    }
    /// Reorder and nest: the entry becomes child `index` of `parent` (a group).
    pub fn move_node(&self, id: i64, parent: Option<i64>, index: u32) -> Result<()> {
        self.edit(|l| Ok(l.move_node(id, parent, index as usize)?))
    }
    /// Safe delete: an album loses only its membership list; a group's
    /// contents move up one level; a smart album is only a rule.
    pub fn delete_node(&self, id: i64) -> Result<()> {
        self.edit(|l| Ok(l.delete_node(id)?))
    }

    /// Members in manual album order.
    pub fn album_images(&self, id: i64) -> Result<Vec<String>> {
        let library = self.read()?;
        let (_, album) = library
            .album_by_id(id)
            .ok_or_else(|| failure(format!("album not found: {id}")))?;
        Ok(album.images.iter().map(ToString::to_string).collect())
    }
    /// Appends in order, skipping members.
    pub fn add_to_album(&self, id: i64, image_ids: Vec<String>) -> Result<()> {
        let images = ids(&image_ids)?;
        self.edit(|l| Ok(l.add_to_album(id, &images)?))
    }
    /// Membership only; files and sidecars are untouched.
    pub fn remove_from_album(&self, id: i64, image_ids: Vec<String>) -> Result<()> {
        let images = ids(&image_ids)?;
        self.edit(|l| Ok(l.remove_from_album(id, &images)?))
    }
    /// `image_ids` must be a permutation of the members.
    pub fn reorder_album(&self, id: i64, image_ids: Vec<String>) -> Result<()> {
        let images = ids(&image_ids)?;
        self.edit(|l| Ok(l.reorder_album(id, &images)?))
    }

    /// Parses and compiles a rule against this library (album names must exist).
    pub fn check_rule(&self, text: String) -> Result<RuleCheck> {
        Ok(self.check(&self.read()?, text))
    }
    /// Renders editor items to text, then checks it; diagnostics refer to `text`.
    pub fn format_rule(&self, items: Vec<RuleItem>) -> Result<RuleCheck> {
        let text = render_items(&items)?;
        Ok(self.check(&self.read()?, text))
    }

    pub fn search(&self, request: SearchRequest) -> Result<SearchResult> {
        let library = self.read()?;
        let mut diag = None;
        let text = if request.text.trim().is_empty() {
            None
        } else {
            match SavedSearch::parse_diagnostic(&request.text) {
                Ok(search) => match library.compile_search(&search) {
                    Ok(q) if q.semantic.is_some() => {
                        diag = Some(RuleDiagnostic {
                            start: 0,
                            end: request.text.len() as u32,
                            message: "Semantic search is not available in the library yet".into(),
                        });
                        None
                    }
                    Ok(_) => Some(search),
                    Err(e) => {
                        diag = Some(diagnostic(Diagnostic::whole(&request.text, &e)));
                        None
                    }
                },
                Err(d) => {
                    diag = Some(diagnostic(d));
                    None
                }
            }
        };
        // Scope clause (album / smart album) is part of the saved rule; a
        // group scope becomes the saved album's scoped parent instead.
        let mut scope_clause = None;
        let mut album_order = None;
        let mut scope_ids = None;
        match request.scope {
            SearchScope::All => {}
            SearchScope::Album { id } => {
                let (handle, album) = library
                    .album_by_id(id)
                    .ok_or_else(|| failure(format!("album not found: {id}")))?;
                scope_clause = Some(SavedSearch::Rule {
                    criteria: "album".into(),
                    operation: ":".into(),
                    value: Value::String(handle.clone()),
                });
                album_order = Some(album.images.clone());
            }
            SearchScope::SmartAlbum { id } => {
                let smart = library
                    .smart_albums
                    .iter()
                    .find(|s| s.id == id)
                    .ok_or_else(|| failure(format!("smart album not found: {id}")))?;
                scope_clause = Some(smart.search.clone());
            }
            SearchScope::Group { .. } => {}
        }
        if let Some(group) = Self::scope_group(&library, request.scope) {
            scope_ids = Some(library.group_images(group)?);
        }
        let mut filters: Vec<(FacetField, SavedSearch)> = Vec::new();
        for f in &request.filters {
            let values: Vec<_> = f
                .values
                .iter()
                .filter(|v| !v.trim().is_empty())
                .map(|v| facet_rule(f.field, v))
                .collect::<Result<_>>()?;
            let clause = match values.len() {
                0 => continue,
                1 => values.into_iter().next().expect("one value"),
                _ => SavedSearch::Any(values),
            };
            filters.push((f.field, clause));
        }
        let clauses = |skip: Option<FacetField>| -> Vec<SavedSearch> {
            scope_clause
                .iter()
                .chain(text.iter())
                .chain(
                    filters
                        .iter()
                        .filter(|(field, _)| Some(*field) != skip)
                        .map(|(_, c)| c),
                )
                .cloned()
                .collect()
        };
        let query = |clauses: Vec<SavedSearch>| -> Result<index::Query> {
            let mut predicates = Vec::new();
            let mut semantic = None;
            if let Some(ids) = &scope_ids {
                predicates.push(index::Predicate::Ids(ids.clone()));
            }
            if let Some(search) = all(clauses) {
                let q = library.compile_search(&search)?;
                predicates.extend(q.predicate);
                semantic = q.semantic;
            }
            if semantic.is_some() {
                return Err(failure(
                    "semantic rules need the embedding index, which the library cannot search yet",
                ));
            }
            Ok(index::Query {
                predicate: Some(index::Predicate::All(predicates)),
                folder: request.folder.clone(),
                limit: i64::MAX as usize,
                ..Default::default()
            })
        };
        let rule = all(clauses(None))
            .map(|s| rule_text(&s))
            .unwrap_or_default();
        let c = self.engine.lock()?;
        let full = query(clauses(None))?;
        let mut found = c.index.search(&full)?;
        if let Some(order) = album_order {
            let rank: std::collections::HashMap<_, _> =
                order.iter().enumerate().map(|(n, id)| (*id, n)).collect();
            found.sort_by_key(|id| rank.get(id).copied().unwrap_or(usize::MAX));
        }
        let base = c.index.facets(&full)?;
        let active = |field| filters.iter().any(|(f, _)| *f == field);
        let facet_of = |field: FacetField| -> Result<index::Facets> {
            if active(field) {
                Ok(c.index.facets(&query(clauses(Some(field)))?)?)
            } else {
                Ok(base.clone())
            }
        };
        let album_total = |value: &str| -> Result<u32> {
            let mut list = clauses(Some(FacetField::Album));
            list.push(facet_rule(FacetField::Album, value)?);
            Ok(c.index.search(&query(list)?)?.len().min(u32::MAX as usize) as u32)
        };
        let facets = SearchFacets {
            decisions: counts(facet_of(FacetField::Decision)?.decisions),
            grades: counts(facet_of(FacetField::Grade)?.grades),
            marks: counts(facet_of(FacetField::Mark)?.marks),
            cameras: counts(facet_of(FacetField::Camera)?.cameras),
            lenses: counts(facet_of(FacetField::Lens)?.lenses),
            keywords: counts(facet_of(FacetField::Keyword)?.keywords),
            in_any_album: album_total("any")?,
            in_no_album: album_total("none")?,
        };
        Ok(SearchResult {
            image_ids: found.iter().map(ToString::to_string).collect(),
            facets,
            diagnostic: diag,
            rule,
            group: Self::scope_group(&library, request.scope),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rendered_items_round_trip_through_the_parser() {
        for text in [
            "rating>=2 AND (camera:\"EOS R5\" OR keyword:beach) AND NOT decision:reject",
            "NOT (album:none OR mark:\"Needs Retouch\")",
            "beach AND date:2024-01..2024-06",
            "\"a quoted phrase\"",
        ] {
            let search: SavedSearch = text.parse().unwrap();
            assert_eq!(rule_text(&search), text);
            let mut items = Vec::new();
            flatten(&search, &mut items);
            assert_eq!(render_items(&items).unwrap(), text);
        }
    }
}
