//! Persisted adapter between numeric engine IDs and opaque index identities.
//! Tombstones survive merges so saved actions never target a reused ID.
use crate::Console;
use engine_api::{EngineError, EngineResult, id::PersonId, people::PersonSummary};
use std::collections::{BTreeMap, BTreeSet, HashSet};

#[derive(Default, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
struct Ids {
    active: BTreeMap<String, PersonId>,
    retired: BTreeSet<PersonId>,
}

impl Console {
    fn write_people_ids(&self, ids: &Ids) -> EngineResult<()> {
        let path = self.app.join("people-ids.json");
        let temp = self.app.join("people-ids.json.tmp");
        std::fs::write(&temp, serde_json::to_vec(ids)?)?;
        std::fs::rename(temp, path)?;
        Ok(())
    }

    fn people_ids(&self) -> EngineResult<Ids> {
        let path = self.app.join("people-ids.json");
        let mut ids: Ids = match std::fs::read(&path) {
            Ok(bytes) => serde_json::from_slice(&bytes)?,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ids::default(),
            Err(e) => return Err(EngineError::io_at(&path, &e)),
        };
        let mut used: HashSet<u64> = ids
            .active
            .values()
            .chain(&ids.retired)
            .map(|p| p.0)
            .collect();
        if used.len() != ids.active.len() + ids.retired.len() {
            return Err(EngineError::invalid(
                "people IDs",
                "duplicate numeric mapping",
            ));
        }
        let people = self.index.people()?;
        let before = ids.clone();
        ids.active.retain(|key, id| {
            if people.iter().any(|p| p.id == *key) {
                true
            } else {
                ids.retired.insert(*id);
                false
            }
        });
        // Preserve canonical numeric IDs when first importing a catalog. Later
        // collisions get a fresh mapping; existing bindings never change.
        for p in &people {
            if !ids.active.contains_key(&p.id)
                && let Ok(n) = p.id.parse::<u64>()
                && n.to_string() == p.id
                && used.insert(n)
            {
                ids.active.insert(p.id.clone(), PersonId(n));
            }
        }
        for p in people {
            if let std::collections::btree_map::Entry::Vacant(entry) = ids.active.entry(p.id) {
                let next = used
                    .iter()
                    .max()
                    .copied()
                    .unwrap_or(0)
                    .checked_add(1)
                    .ok_or_else(|| EngineError::invalid("person_id", "ID space exhausted"))?;
                used.insert(next);
                entry.insert(PersonId(next));
            }
        }
        if ids != before {
            self.write_people_ids(&ids)?;
        }
        Ok(ids)
    }

    pub(crate) fn person_key(&self, id: PersonId) -> EngineResult<String> {
        let ids = self.people_ids()?;
        self.index
            .people()?
            .into_iter()
            .find(|p| ids.active.get(&p.id) == Some(&id))
            .map(|p| p.id)
            .ok_or_else(|| EngineError::not_found("person", id))
    }

    pub(crate) fn reserve_person(&self) -> EngineResult<(PersonId, String)> {
        let mut ids = self.people_ids()?;
        let mut next = ids
            .active
            .values()
            .chain(&ids.retired)
            .map(|p| p.0)
            .max()
            .unwrap_or(0);
        loop {
            next = next
                .checked_add(1)
                .ok_or_else(|| EngineError::invalid("person_id", "ID space exhausted"))?;
            let key = next.to_string();
            if !ids.active.contains_key(&key) {
                let id = PersonId(next);
                ids.active.insert(key.clone(), id);
                self.write_people_ids(&ids)?;
                return Ok((id, key));
            }
        }
    }

    /// All catalog identities with persistent numeric wire IDs, including
    /// identities created by ml_faces. Enumeration also reserves new mappings.
    pub fn people(&self) -> EngineResult<Vec<PersonSummary>> {
        let ids = self.people_ids()?;
        self.index
            .people()?
            .into_iter()
            .map(|p| {
                let mut confirmed_count = 0;
                let mut approximate = false;
                for image in self
                    .index
                    .images_with_person(&p.id, false, i64::MAX as usize, 0)?
                {
                    for a in self
                        .index
                        .face_assignments(image)?
                        .into_iter()
                        .filter(|a| a.person_id == p.id)
                    {
                        confirmed_count += u64::from(a.confirmed);
                        // The index does not retain clustering provenance. Do
                        // not claim exact clustering on confirmation: all
                        // nonempty memberships conservatively remain approximate.
                        approximate = true;
                    }
                }
                Ok(PersonSummary {
                    id: ids.active[&p.id],
                    name: p.name,
                    confirmed_count,
                    approximate,
                })
            })
            .collect()
    }
}
