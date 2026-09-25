use crate::{Album, Library};
use engine_api::{EngineError, EngineResult, id::ImageId};
use index::{Predicate, Query};
use std::collections::BTreeSet;

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
        let mut query = album.search.compile()?;
        if let Some(group) = album.parent {
            let mut predicates = vec![Predicate::Ids(self.group_images(group)?)];
            predicates.extend(query.predicate.take());
            query.predicate = Some(Predicate::All(predicates));
        }
        Ok(query)
    }
}
