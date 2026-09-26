//! Serialized, host-driven clustering jobs. No implicit threads or sidecar writes.
use crate::{FaceQuality, QualityGate, cluster_eligible, nearest_medoid, normalize};
use anyhow::{Result, ensure};
use engine_api::id::ImageId;
use index::{FaceKey, Index, Person};
use std::collections::{HashMap, HashSet};

#[derive(Debug, Clone, Copy)]
pub struct PeopleOptions {
    pub threshold: f32,
    pub quality: QualityGate,
    /// Refit unnamed/unconfirmed faces after this many newly observed faces.
    pub recluster_every: usize,
}
impl Default for PeopleOptions {
    fn default() -> Self {
        Self {
            threshold: 0.363,
            quality: QualityGate::default(),
            recluster_every: 1000,
        }
    }
}
#[derive(Debug, Default, Clone, Copy)]
pub struct PeopleReport {
    pub assigned: usize,
    pub reclustered: bool,
    pub approximate: bool,
}
/// Keep one job per catalog. Run on the host's worker queue, with all catalog
/// images for a periodic refit; queue subsets are valid incremental scopes.
/// Named clusters and confirmed faces are never moved by automatic jobs.
pub struct PeopleJob {
    options: PeopleOptions,
    pending: usize,
}
impl PeopleJob {
    pub fn new(options: PeopleOptions) -> Result<Self> {
        ensure!(
            (-1.0..=1.0).contains(&options.threshold),
            "invalid cosine threshold"
        );
        ensure!(options.recluster_every > 0, "zero recluster interval");
        options.quality.validate()?;
        Ok(Self {
            options,
            pending: 0,
        })
    }
    pub fn run(&mut self, index: &Index, images: &[ImageId], force: bool) -> Result<PeopleReport> {
        let mut existing = index.people()?;
        let mut repaired = Vec::new();
        for person in &mut existing {
            if person.medoid.is_none() {
                person.medoid = catalog_medoid(index, &person.id, self.options.quality)?;
                if person.medoid.is_some() {
                    repaired.push(person.clone());
                }
            }
        }
        let mut old = HashMap::new();
        let mut rows = Vec::new();
        let mut seen = HashSet::new();
        for &image_id in images {
            if !seen.insert(image_id) {
                continue;
            }
            for a in index.face_assignments(image_id)? {
                old.insert(a.face, a);
            }
            for f in index.faces(image_id)? {
                let key = FaceKey {
                    image_id,
                    ordinal: f.id,
                };
                let embedding = f
                    .embedding
                    .as_deref()
                    .and_then(|e| <&[f32; 128]>::try_from(e).ok())
                    .and_then(|e| normalize(e).ok());
                let q = FaceQuality {
                    confidence: f.confidence,
                    width: f.bbox[2],
                    height: f.bbox[3],
                    sharpness: f.sharpness,
                };
                rows.push((key, embedding, q));
            }
        }
        let new_count = rows
            .iter()
            .filter(|(key, _, _)| !old.contains_key(key))
            .count();
        let refit = force
            || existing.is_empty()
            || self.pending.saturating_add(new_count) >= self.options.recluster_every;
        let protected: HashSet<_> = old
            .values()
            .filter(|a| a.confirmed || a.person_name.is_some())
            .map(|a| a.person_id.clone())
            .collect();
        let mut centers = Vec::new();
        let mut center_ids = Vec::new();
        for p in &existing {
            if (!refit || p.name.is_some() || protected.contains(&p.id))
                && let Some(m) = p
                    .medoid
                    .as_deref()
                    .and_then(|m| <&[f32; 128]>::try_from(m).ok())
                    .and_then(|m| normalize(m).ok())
            {
                center_ids.push(p.id.clone());
                centers.push(m);
            }
        }
        let mut assignments = Vec::new();
        let mut pending_keys = Vec::new();
        let mut embeddings = Vec::new();
        let mut quality = Vec::new();
        for (key, embedding, q) in &rows {
            if let Some(a) = old.get(key)
                && (!refit || protected.contains(&a.person_id))
            {
                continue;
            }
            if self.options.quality.eligible(*q)?
                && let Some(e) = embedding
                && let Some(m) = nearest_medoid(e, &centers, self.options.threshold)?
            {
                assignments.push((*key, center_ids[m.medoid_index].clone()));
                continue;
            }
            pending_keys.push(*key);
            embeddings.push(embedding.unwrap_or([0.; 128]));
            // Invalid/missing descriptors remain assignable but cannot train.
            quality.push(if embedding.is_some() {
                *q
            } else {
                FaceQuality {
                    confidence: 0.,
                    width: 0.,
                    ..*q
                }
            });
        }
        let result = cluster_eligible(
            &embeddings,
            &quality,
            self.options.threshold,
            self.options.quality,
        )?;
        let mut used: HashSet<_> = existing.iter().map(|p| p.id.clone()).collect();
        let mut reused = HashSet::new();
        let mut people = repaired;
        for c in result.clusters {
            // Retain an overlapping unnamed ID at refit, but never collapse
            // protected identities. Ties are deterministic in input order.
            let prior = c
                .members
                .iter()
                .filter_map(|&i| old.get(&pending_keys[i]))
                .map(|a| &a.person_id)
                .find(|id| !protected.contains(*id) && !reused.contains(*id));
            let id = if let Some(prior) = prior {
                reused.insert(prior.clone());
                prior.clone()
            } else {
                let key = pending_keys[c.medoid_index];
                let base = format!("person-{}-{}", key.image_id, key.ordinal);
                let mut id = base.clone();
                let mut suffix = 0usize;
                while !used.insert(id.clone()) {
                    suffix += 1;
                    id = format!("{base}-{suffix}");
                }
                id
            };
            let medoid = result.eligibility[c.medoid_index].then(|| c.medoid.to_vec());
            people.push(Person {
                id: id.clone(),
                name: None,
                medoid,
            });
            for i in c.members {
                assignments.push((pending_keys[i], id.clone()));
            }
        }
        index.apply_people_plan(&people, &assignments)?;
        self.pending = if refit {
            0
        } else {
            self.pending.saturating_add(new_count)
        };
        Ok(PeopleReport {
            assigned: assignments.len(),
            reclustered: refit,
            approximate: result.approximate,
        })
    }
}

// Merge/split deliberately invalidate their medoids. Rebuild from the entire
// person's catalog membership, not just the queue currently being displayed.
fn catalog_medoid(index: &Index, person: &str, gate: QualityGate) -> Result<Option<Vec<f32>>> {
    let mut unit = Vec::new();
    for image in index.images_with_person(person, false, i64::MAX as usize, 0)? {
        let ordinals: HashSet<_> = index
            .face_assignments(image)?
            .into_iter()
            .filter(|a| a.person_id == person)
            .map(|a| a.face.ordinal)
            .collect();
        for face in index.faces(image)? {
            if ordinals.contains(&face.id)
                && gate.eligible(FaceQuality {
                    confidence: face.confidence,
                    width: face.bbox[2],
                    height: face.bbox[3],
                    sharpness: face.sharpness,
                })?
                && let Some(e) = face
                    .embedding
                    .as_deref()
                    .and_then(|e| <&[f32; 128]>::try_from(e).ok())
                    .and_then(|e| normalize(e).ok())
            {
                unit.push(e);
            }
        }
    }
    if unit.is_empty() {
        return Ok(None);
    }
    let members: Vec<_> = (0..unit.len()).collect();
    Ok(Some(
        unit[crate::clustering::medoid(&unit, &members)].to_vec(),
    ))
}

#[derive(Debug, Clone, PartialEq)]
pub struct NameSuggestion {
    pub unnamed_id: String,
    pub named_id: String,
    pub name: String,
    pub similarity: f32,
}
/// Read-only suggestions; accepting is an explicit merge/assignment, never a rename.
pub fn name_suggestions(index: &Index, threshold: f32) -> Result<Vec<NameSuggestion>> {
    ensure!(
        (-1.0..=1.0).contains(&threshold),
        "invalid cosine threshold"
    );
    let people = index.people()?;
    let valid = |p: &Person| {
        p.medoid
            .as_deref()
            .and_then(|m| <&[f32; 128]>::try_from(m).ok())
            .and_then(|m| normalize(m).ok())
    };
    let named: Vec<_> = people
        .iter()
        .filter_map(|p| Some((p, p.name.as_ref()?, valid(p)?)))
        .collect();
    let medoids: Vec<_> = named.iter().map(|p| p.2).collect();
    let mut out = Vec::new();
    for p in people.iter().filter(|p| p.name.is_none()) {
        if let Some(medoid) = valid(p)
            && let Some(m) = nearest_medoid(&medoid, &medoids, threshold)?
        {
            let (target, name, _) = named[m.medoid_index];
            out.push(NameSuggestion {
                unnamed_id: p.id.clone(),
                named_id: target.id.clone(),
                name: name.clone(),
                similarity: m.similarity,
            });
        }
    }
    Ok(out)
}
