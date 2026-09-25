use crate::{Album, AlbumGroup, Keyword, Library, SavedSearch, SmartAlbum, search::Albums};
use engine_api::{EngineError, EngineResult, id::ImageId};
use index::{Predicate, Query};
use std::collections::{BTreeMap, BTreeSet};

/// Sidebar entry kinds. IDs are unique across all three kinds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum NodeKind {
    Group,
    Album,
    SmartAlbum,
}

/// One sidebar entry. `handle` is the album map key (basket/UI handle).
#[derive(Debug, Clone, PartialEq)]
pub struct SidebarNode {
    pub id: i64,
    pub kind: NodeKind,
    pub name: String,
    pub parent: Option<i64>,
    pub handle: Option<String>,
    pub depth: usize,
}

impl Library {
    pub fn next_id(&self) -> EngineResult<i64> {
        self.albums
            .values()
            .map(|a| a.id)
            .chain(self.album_groups.iter().map(|g| g.id))
            .chain(self.smart_albums.iter().map(|s| s.id))
            .max()
            .unwrap_or(0)
            .max(0)
            .checked_add(1)
            .ok_or_else(|| EngineError::invalid("library", "ID space exhausted"))
    }
    pub fn create_album(&mut self, name: &str, parent: Option<i64>) -> EngineResult<i64> {
        self.check_name(name)?;
        if let Some(parent) = parent {
            self.group_images(parent)?;
        }
        let id = self.next_id()?;
        self.albums.insert(
            name.into(),
            Album {
                id,
                name: name.into(),
                parent,
                ..Default::default()
            },
        );
        Ok(id)
    }
    fn check_name(&self, name: &str) -> EngineResult<()> {
        if name.trim().is_empty() || self.albums.contains_key(name) {
            Err(EngineError::invalid(
                "album",
                "name must be nonempty and unique",
            ))
        } else {
            Ok(())
        }
    }
    pub fn rename_album(&mut self, id: i64, name: &str) -> EngineResult<()> {
        let old = self
            .albums
            .iter()
            .find(|(_, a)| a.id == id)
            .map(|(k, _)| k.clone())
            .ok_or_else(|| EngineError::not_found("album", id.to_string()))?;
        if old == name {
            return Ok(());
        }
        self.check_name(name)?;
        let mut album = self.albums.remove(&old).expect("key just found");
        album.name = name.into();
        self.albums.insert(name.into(), album);
        Ok(())
    }
    pub fn add_to_album(&mut self, id: i64, images: &[ImageId]) -> EngineResult<()> {
        let album = self
            .albums
            .values_mut()
            .find(|a| a.id == id)
            .ok_or_else(|| EngineError::not_found("album", id.to_string()))?;
        for image in images {
            if !album.images.contains(image) {
                album.images.push(*image);
            }
        }
        Ok(())
    }
    pub fn reorder_album(&mut self, id: i64, order: &[ImageId]) -> EngineResult<()> {
        let album = self
            .albums
            .values_mut()
            .find(|a| a.id == id)
            .ok_or_else(|| EngineError::not_found("album", id.to_string()))?;
        if order.len() != album.images.len()
            || order.iter().collect::<BTreeSet<_>>().len() != order.len()
            || order.iter().collect::<BTreeSet<_>>() != album.images.iter().collect::<BTreeSet<_>>()
        {
            return Err(EngineError::invalid(
                "album",
                "order must be a permutation of members",
            ));
        }
        album.images = order.to_vec();
        Ok(())
    }
    /// Validate hierarchy before scoping: never turn a malformed scope into a
    /// global search. Manual memberships only, so smart albums cannot recurse.
    pub fn group_images(&self, group: i64) -> EngineResult<Vec<ImageId>> {
        let groups: std::collections::BTreeMap<_, _> =
            self.album_groups.iter().map(|g| (g.id, g.parent)).collect();
        if groups.len() != self.album_groups.len() {
            return Err(EngineError::invalid("group", "duplicate ID"));
        }
        if !groups.contains_key(&group) {
            return Err(EngineError::not_found("group", group.to_string()));
        }
        for id in groups.keys() {
            let mut seen = BTreeSet::new();
            let mut next = Some(*id);
            while let Some(id) = next {
                if !seen.insert(id) {
                    return Err(EngineError::invalid("group", "hierarchy cycle"));
                }
                next = *groups
                    .get(&id)
                    .ok_or_else(|| EngineError::not_found("group", id.to_string()))?;
            }
        }
        let mut scope = BTreeSet::from([group]);
        loop {
            let before = scope.len();
            for (id, parent) in &groups {
                if parent.is_some_and(|p| scope.contains(&p)) {
                    scope.insert(*id);
                }
            }
            if scope.len() == before {
                break;
            }
        }
        Ok(self
            .albums
            .values()
            .filter(|a| a.parent.is_some_and(|p| scope.contains(&p)))
            .flat_map(|a| a.images.iter().copied())
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect())
    }
    pub fn smart_query(&self, id: i64) -> EngineResult<Query> {
        let album = self
            .smart_albums
            .iter()
            .find(|a| a.id == id)
            .ok_or_else(|| EngineError::not_found("smart album", id.to_string()))?;
        let mut query = self.compile_search(&album.search)?;
        if let Some(group) = album.parent.filter(|_| album.scoped) {
            let mut predicates = vec![Predicate::Ids(self.group_images(group)?)];
            predicates.extend(query.predicate.take());
            query.predicate = Some(Predicate::All(predicates));
        }
        Ok(query)
    }
}

fn not_found(kind: &str, id: i64) -> EngineError {
    EngineError::not_found(kind, id.to_string())
}
fn nonempty(kind: &str, name: &str) -> EngineResult<()> {
    if name.trim().is_empty() {
        Err(EngineError::invalid(kind, "name must be nonempty"))
    } else {
        Ok(())
    }
}

impl Library {
    /// Compile a saved search, resolving `album:` rules against this document.
    pub fn compile_search(&self, search: &SavedSearch) -> EngineResult<Query> {
        search.compile_with(Albums::Library(self))
    }

    /// `album:none` (in no album), `album:any`, or a named album (handle or
    /// display name). Unknown names are errors, never an empty match.
    pub(crate) fn album_predicate(&self, name: &str) -> EngineResult<Predicate> {
        let all = || -> Vec<ImageId> {
            self.albums
                .values()
                .flat_map(|a| a.images.iter().copied())
                .collect::<BTreeSet<_>>()
                .into_iter()
                .collect()
        };
        Ok(match name {
            "none" => Predicate::Not(Box::new(Predicate::Ids(all()))),
            "any" => Predicate::Ids(all()),
            _ => Predicate::Ids(
                self.albums
                    .get(name)
                    .or_else(|| self.albums.values().find(|a| a.name == name))
                    .ok_or_else(|| EngineError::not_found("album", name))?
                    .images
                    .clone(),
            ),
        })
    }

    pub fn album_by_id(&self, id: i64) -> Option<(&String, &Album)> {
        self.albums.iter().find(|(_, a)| a.id == id)
    }

    fn kind_of(&self, id: i64) -> Option<(NodeKind, Option<i64>)> {
        if let Some((_, a)) = self.album_by_id(id) {
            return Some((NodeKind::Album, a.parent));
        }
        if let Some(g) = self.album_groups.iter().find(|g| g.id == id) {
            return Some((NodeKind::Group, g.parent));
        }
        self.smart_albums
            .iter()
            .find(|s| s.id == id)
            .map(|s| (NodeKind::SmartAlbum, s.parent))
    }

    fn require_group(&self, parent: Option<i64>) -> EngineResult<()> {
        match parent {
            Some(p) if !self.album_groups.iter().any(|g| g.id == p) => Err(not_found("group", p)),
            _ => Ok(()),
        }
    }

    /// Children of every parent in sidebar order: listed IDs first, then the
    /// rest by kind (groups, albums, smart albums) and case-insensitive name.
    fn children(&self) -> BTreeMap<Option<i64>, Vec<(i64, NodeKind, String)>> {
        let rank: BTreeMap<i64, usize> = self
            .sidebar_order
            .iter()
            .enumerate()
            .map(|(n, id)| (*id, n))
            .collect();
        let mut out: BTreeMap<Option<i64>, Vec<(i64, NodeKind, String)>> = BTreeMap::new();
        let entries = self
            .album_groups
            .iter()
            .map(|g| (g.id, NodeKind::Group, g.name.clone(), g.parent))
            .chain(
                self.albums
                    .values()
                    .map(|a| (a.id, NodeKind::Album, a.name.clone(), a.parent)),
            )
            .chain(
                self.smart_albums
                    .iter()
                    .map(|s| (s.id, NodeKind::SmartAlbum, s.name.clone(), s.parent)),
            );
        for (id, kind, name, parent) in entries {
            out.entry(parent).or_default().push((id, kind, name));
        }
        for list in out.values_mut() {
            list.sort_by_cached_key(|(id, kind, name)| {
                (
                    rank.get(id).copied().unwrap_or(usize::MAX),
                    *kind,
                    name.to_lowercase(),
                    *id,
                )
            });
        }
        out
    }

    /// Depth-first sidebar listing. Entries whose parent is missing (or part
    /// of a cycle) are listed at the root rather than hidden.
    pub fn sidebar(&self) -> Vec<SidebarNode> {
        type Children = BTreeMap<Option<i64>, Vec<(i64, NodeKind, String)>>;
        fn walk(
            lib: &Library,
            children: &Children,
            node: (i64, NodeKind, &str, Option<i64>),
            depth: usize,
            seen: &mut BTreeSet<i64>,
            out: &mut Vec<SidebarNode>,
        ) {
            let (id, kind, name, parent) = node;
            if !seen.insert(id) {
                return;
            }
            out.push(SidebarNode {
                id,
                kind,
                name: name.into(),
                parent,
                handle: lib.album_by_id(id).map(|(k, _)| k.clone()),
                depth,
            });
            if kind == NodeKind::Group {
                for (child, kind, name) in children.get(&Some(id)).into_iter().flatten() {
                    walk(
                        lib,
                        children,
                        (*child, *kind, name, Some(id)),
                        depth + 1,
                        seen,
                        out,
                    );
                }
            }
        }
        let children = self.children();
        let mut out = Vec::new();
        let mut seen = BTreeSet::new();
        for (id, kind, name) in children.get(&None).into_iter().flatten() {
            walk(
                self,
                &children,
                (*id, *kind, name, None),
                0,
                &mut seen,
                &mut out,
            );
        }
        for list in children.values() {
            for (id, kind, name) in list {
                walk(
                    self,
                    &children,
                    (*id, *kind, name, None),
                    0,
                    &mut seen,
                    &mut out,
                );
            }
        }
        out
    }

    fn normalize_order(&mut self, children: &BTreeMap<Option<i64>, Vec<(i64, NodeKind, String)>>) {
        self.sidebar_order = children
            .values()
            .flat_map(|list| list.iter().map(|(id, _, _)| *id))
            .collect();
    }

    pub fn create_group(&mut self, name: &str, parent: Option<i64>) -> EngineResult<i64> {
        nonempty("group", name)?;
        self.require_group(parent)?;
        let id = self.next_id()?;
        self.album_groups.push(AlbumGroup {
            id,
            name: name.into(),
            parent,
        });
        Ok(id)
    }

    /// The search must compile against this library (so `album:` rules name
    /// existing albums). A parent without `scoped` is only a sidebar location.
    pub fn create_smart_album(
        &mut self,
        name: &str,
        search: SavedSearch,
        parent: Option<i64>,
        scoped: bool,
    ) -> EngineResult<i64> {
        nonempty("smart album", name)?;
        self.require_group(parent)?;
        self.compile_search(&search)?;
        let id = self.next_id()?;
        self.smart_albums.push(SmartAlbum {
            id,
            name: name.into(),
            parent,
            search,
            scoped,
        });
        Ok(id)
    }

    pub fn update_smart_album(
        &mut self,
        id: i64,
        search: Option<SavedSearch>,
        scoped: Option<bool>,
    ) -> EngineResult<()> {
        if let Some(search) = &search {
            self.compile_search(search)?;
        }
        let album = self
            .smart_albums
            .iter_mut()
            .find(|s| s.id == id)
            .ok_or_else(|| not_found("smart album", id))?;
        if let Some(search) = search {
            album.search = search;
        }
        if let Some(scoped) = scoped {
            album.scoped = scoped;
        }
        Ok(())
    }

    /// Renames any sidebar entry. Album names stay unique (they are handles).
    pub fn rename_node(&mut self, id: i64, name: &str) -> EngineResult<()> {
        match self.kind_of(id).map(|(k, _)| k) {
            Some(NodeKind::Album) => self.rename_album(id, name),
            Some(NodeKind::Group) => {
                nonempty("group", name)?;
                if let Some(g) = self.album_groups.iter_mut().find(|g| g.id == id) {
                    g.name = name.into();
                }
                Ok(())
            }
            Some(NodeKind::SmartAlbum) => {
                nonempty("smart album", name)?;
                if let Some(s) = self.smart_albums.iter_mut().find(|s| s.id == id) {
                    s.name = name.into();
                }
                Ok(())
            }
            None => Err(not_found("sidebar entry", id)),
        }
    }

    /// Reparents (nesting) and reorders an entry: it becomes child `index`
    /// (clamped) of `parent`. A group cannot move into itself or a descendant.
    pub fn move_node(&mut self, id: i64, parent: Option<i64>, index: usize) -> EngineResult<()> {
        let (kind, _) = self
            .kind_of(id)
            .ok_or_else(|| not_found("sidebar entry", id))?;
        self.require_group(parent)?;
        if kind == NodeKind::Group {
            let mut next = parent;
            let mut hops = 0;
            while let Some(p) = next {
                if p == id || hops > self.album_groups.len() {
                    return Err(EngineError::invalid(
                        "group",
                        "cannot move a group into itself or a descendant",
                    ));
                }
                hops += 1;
                next = self
                    .album_groups
                    .iter()
                    .find(|g| g.id == p)
                    .and_then(|g| g.parent);
            }
        }
        let mut children = self.children();
        for list in children.values_mut() {
            list.retain(|(child, _, _)| *child != id);
        }
        match kind {
            NodeKind::Album => {
                if let Some(a) = self.albums.values_mut().find(|a| a.id == id) {
                    a.parent = parent;
                }
            }
            NodeKind::Group => {
                if let Some(g) = self.album_groups.iter_mut().find(|g| g.id == id) {
                    g.parent = parent;
                }
            }
            NodeKind::SmartAlbum => {
                if let Some(s) = self.smart_albums.iter_mut().find(|s| s.id == id) {
                    s.parent = parent;
                }
            }
        }
        let siblings = children.entry(parent).or_default();
        let at = index.min(siblings.len());
        siblings.insert(at, (id, kind, String::new()));
        self.normalize_order(&children);
        Ok(())
    }

    /// Safe delete of a sidebar entry. Albums lose only their membership list;
    /// groups are removed and their contents move up one level; smart albums
    /// are rules. Photos and sidecars are never touched.
    pub fn delete_node(&mut self, id: i64) -> EngineResult<()> {
        let (kind, parent) = self
            .kind_of(id)
            .ok_or_else(|| not_found("sidebar entry", id))?;
        let mut children = self.children();
        match kind {
            NodeKind::Album => self.delete_album(id)?,
            NodeKind::SmartAlbum => self.smart_albums.retain(|s| s.id != id),
            NodeKind::Group => {
                self.album_groups.retain(|g| g.id != id);
                for a in self.albums.values_mut().filter(|a| a.parent == Some(id)) {
                    a.parent = parent;
                }
                for g in self
                    .album_groups
                    .iter_mut()
                    .filter(|g| g.parent == Some(id))
                {
                    g.parent = parent;
                }
                for s in self
                    .smart_albums
                    .iter_mut()
                    .filter(|s| s.parent == Some(id))
                {
                    s.parent = parent;
                }
                // Lifted children take the group's place among its siblings.
                let lifted = children.remove(&Some(id)).unwrap_or_default();
                let siblings = children.entry(parent).or_default();
                let at = siblings
                    .iter()
                    .position(|(child, _, _)| *child == id)
                    .unwrap_or(siblings.len());
                siblings.splice(at..at, lifted);
            }
        }
        for list in children.values_mut() {
            list.retain(|(child, _, _)| *child != id);
        }
        self.normalize_order(&children);
        Ok(())
    }

    fn keyword_ids(list: &[Keyword], out: &mut Vec<i64>) {
        for k in list {
            out.push(k.id);
            Self::keyword_ids(&k.children, out);
        }
    }

    /// (name, parent name) in preorder.
    pub fn keyword_pairs(&self) -> Vec<(String, Option<String>)> {
        fn walk(list: &[Keyword], parent: Option<&str>, out: &mut Vec<(String, Option<String>)>) {
            for k in list {
                out.push((k.name.clone(), parent.map(str::to_owned)));
                walk(&k.children, Some(&k.name), out);
            }
        }
        let mut out = Vec::new();
        walk(&self.keywords, None, &mut out);
        out
    }

    /// Root-to-leaf names for a keyword in the tree.
    pub fn keyword_path(&self, name: &str) -> Option<Vec<String>> {
        fn find(list: &[Keyword], name: &str, path: &mut Vec<String>) -> bool {
            for k in list {
                path.push(k.name.clone());
                if k.name == name || find(&k.children, name, path) {
                    return true;
                }
                path.pop();
            }
            false
        }
        let mut path = Vec::new();
        find(&self.keywords, name, &mut path).then_some(path)
    }

    fn take_keyword(list: &mut Vec<Keyword>, name: &str) -> Option<Keyword> {
        if let Some(i) = list.iter().position(|k| k.name == name) {
            return Some(list.remove(i));
        }
        list.iter_mut()
            .find_map(|k| Self::take_keyword(&mut k.children, name))
    }

    fn keyword_mut<'a>(list: &'a mut [Keyword], name: &str) -> Option<&'a mut Keyword> {
        for k in list {
            if k.name == name {
                return Some(k);
            }
            if let Some(found) = Self::keyword_mut(&mut k.children, name) {
                return Some(found);
            }
        }
        None
    }

    fn keyword_siblings(&mut self, parent: Option<&str>) -> EngineResult<&mut Vec<Keyword>> {
        match parent {
            None => Ok(&mut self.keywords),
            Some(p) => Self::keyword_mut(&mut self.keywords, p)
                .map(|k| &mut k.children)
                .ok_or_else(|| EngineError::not_found("keyword", p)),
        }
    }

    /// Keyword names are unique across the tree (they are what sidecars store).
    pub fn add_keyword(&mut self, name: &str, parent: Option<&str>) -> EngineResult<i64> {
        let name = name.trim();
        if name.is_empty() || name.contains('|') {
            return Err(EngineError::invalid(
                "keyword",
                "name must be nonempty and must not contain '|'",
            ));
        }
        if self.keyword_path(name).is_some() {
            return Err(EngineError::invalid("keyword", "already exists"));
        }
        let mut ids = Vec::new();
        Self::keyword_ids(&self.keywords, &mut ids);
        let id = ids.into_iter().max().unwrap_or(0).max(0) + 1;
        let siblings = self.keyword_siblings(parent)?;
        siblings.push(Keyword {
            id,
            name: name.into(),
            synonyms: vec![],
            children: vec![],
        });
        siblings.sort_by_key(|k| k.name.to_lowercase());
        Ok(id)
    }

    pub fn move_keyword(&mut self, name: &str, parent: Option<&str>) -> EngineResult<()> {
        let path = self
            .keyword_path(name)
            .ok_or_else(|| EngineError::not_found("keyword", name))?;
        if let Some(p) = parent {
            let target = self
                .keyword_path(p)
                .ok_or_else(|| EngineError::not_found("keyword", p))?;
            if target.starts_with(&path) {
                return Err(EngineError::invalid(
                    "keyword",
                    "cannot move a keyword into itself or a descendant",
                ));
            }
        }
        let keyword = Self::take_keyword(&mut self.keywords, name).expect("path found");
        let siblings = self.keyword_siblings(parent)?;
        siblings.push(keyword);
        siblings.sort_by_key(|k| k.name.to_lowercase());
        Ok(())
    }

    /// Removes a keyword from the list only; its children move up one level.
    /// Photos keep the tag in their sidecars (remove it from photos explicitly).
    pub fn delete_keyword(&mut self, name: &str) -> EngineResult<()> {
        let path = self
            .keyword_path(name)
            .ok_or_else(|| EngineError::not_found("keyword", name))?;
        let parent = path.len().checked_sub(2).map(|i| path[i].clone());
        let keyword = Self::take_keyword(&mut self.keywords, name).expect("path found");
        let siblings = self.keyword_siblings(parent.as_deref())?;
        siblings.extend(keyword.children);
        siblings.sort_by_key(|k| k.name.to_lowercase());
        Ok(())
    }
}
