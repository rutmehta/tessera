//! Persistent people identities, separate from image-local detector ordinals.
use crate::{Index, IndexError};
use engine_api::error::{EngineError, EngineResult};
use engine_api::id::ImageId;
use rusqlite::params;

/// Composite identity of a detector face. Replacement detection invalidates assignments.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct FaceKey {
    pub image_id: ImageId,
    pub ordinal: u32,
}

/// An assigned face with its current cluster name, resolved at read time.
#[derive(Debug, Clone, PartialEq)]
pub struct FaceAssignment {
    pub face: FaceKey,
    pub person_id: String,
    pub person_name: Option<String>,
    pub confirmed: bool,
}

/// A stable, caller-allocated cluster identity. Names need not be unique.
#[derive(Debug, Clone, PartialEq)]
pub struct Person {
    pub id: String,
    pub name: Option<String>,
    /// Optional finite 128-component SFace descriptor.
    pub medoid: Option<Vec<f32>>,
}

fn sql(error: rusqlite::Error) -> EngineError {
    IndexError::Sql(error).into()
}
fn require_changed(n: usize, kind: &str, id: &str) -> EngineResult<()> {
    if n == 0 {
        Err(EngineError::not_found(kind, id))
    } else {
        Ok(())
    }
}

impl Index {
    /// Commit an automatic clustering plan atomically. Existing names and
    /// confirmed assignments are never overwritten. Caller serializes jobs.
    pub fn apply_people_plan(
        &self,
        people: &[Person],
        assignments: &[(FaceKey, String)],
    ) -> EngineResult<()> {
        let tx = self.0.conn.unchecked_transaction().map_err(sql)?;
        for person in people {
            if person.id.trim().is_empty()
                || person
                    .medoid
                    .as_ref()
                    .is_some_and(|m| m.len() != 128 || m.iter().any(|v| !v.is_finite()))
            {
                return Err(EngineError::invalid("person", "invalid clustering plan"));
            }
            let medoid = person
                .medoid
                .as_ref()
                .map(serde_json::to_string)
                .transpose()
                .map_err(|e| EngineError::invalid("medoid", e.to_string()))?;
            tx.execute("INSERT INTO person(id,name,medoid) VALUES(?,?,?) ON CONFLICT(id) DO UPDATE SET medoid=excluded.medoid", params![person.id,person.name,medoid]).map_err(sql)?;
        }
        for (face, person) in assignments {
            tx.execute("INSERT INTO face_person(image_id,ordinal,person_id,confirmed) VALUES(?,?,?,0) ON CONFLICT(image_id,ordinal) DO UPDATE SET person_id=excluded.person_id WHERE face_person.confirmed=0 AND NOT EXISTS(SELECT 1 FROM person WHERE id=face_person.person_id AND name IS NOT NULL)", params![face.image_id.to_string(),face.ordinal,person]).map_err(sql)?;
        }
        tx.commit().map_err(sql)
    }

    /// Atomically move source into target and delete source. Target's name wins,
    /// confirmations survive, and the now-stale target medoid is cleared.
    /// Both identities must exist and differ.
    pub fn merge_people(&self, target: &str, source: &str) -> EngineResult<()> {
        if target == source {
            return Err(EngineError::invalid(
                "person",
                "cannot merge a person into itself",
            ));
        }
        let tx = self.0.conn.unchecked_transaction().map_err(sql)?;
        require_changed(
            tx.execute("UPDATE person SET medoid=NULL WHERE id=?", [target])
                .map_err(sql)?,
            "person",
            target,
        )?;
        tx.execute(
            "UPDATE face_person SET person_id=? WHERE person_id=?",
            params![target, source],
        )
        .map_err(sql)?;
        require_changed(
            tx.execute("DELETE FROM person WHERE id=?", [source])
                .map_err(sql)?,
            "person",
            source,
        )?;
        tx.commit().map_err(sql)
    }

    /// Atomically move selected members into a NEW unnamed cluster. Selected
    /// confirmations reset, both medoids clear; the source (even empty) survives.
    /// Empty/duplicate selections, missing members and existing new IDs fail.
    pub fn split_person(&self, source: &str, new_id: &str, faces: &[FaceKey]) -> EngineResult<()> {
        let unique: std::collections::HashSet<_> = faces.iter().collect();
        if new_id.trim().is_empty() || faces.is_empty() || unique.len() != faces.len() {
            return Err(EngineError::invalid(
                "split",
                "nonblank new ID and nonempty unique faces required",
            ));
        }
        let tx = self.0.conn.unchecked_transaction().map_err(sql)?;
        require_changed(
            tx.execute("UPDATE person SET medoid=NULL WHERE id=?", [source])
                .map_err(sql)?,
            "person",
            source,
        )?;
        tx.execute(
            "INSERT INTO person(id,name,medoid) VALUES(?,NULL,NULL)",
            [new_id],
        )
        .map_err(sql)?;
        for face in faces {
            require_changed(tx.execute("UPDATE face_person SET person_id=?,confirmed=0 WHERE image_id=? AND ordinal=? AND person_id=?",params![new_id,face.image_id.to_string(),face.ordinal,source]).map_err(sql)?,"source face", &format!("{}:{}",face.image_id,face.ordinal))?;
        }
        tx.commit().map_err(sql)
    }

    /// Assign an existing face. Moving to another person resets confirmation;
    /// reassigning to the same person preserves it. Foreign keys reject missing IDs.
    /// Membership changes invalidate both representatives in the same transaction.
    pub fn assign_face(&self, face: FaceKey, person_id: &str) -> EngineResult<()> {
        let tx = self.0.conn.unchecked_transaction().map_err(sql)?;
        tx.execute(
            "UPDATE person SET medoid=NULL WHERE
             (id=?3 OR id IN (SELECT person_id FROM face_person WHERE image_id=?1 AND ordinal=?2))
             AND NOT EXISTS(SELECT 1 FROM face_person WHERE image_id=?1 AND ordinal=?2 AND person_id=?3)",
            params![face.image_id.to_string(), face.ordinal, person_id],
        ).map_err(sql)?;
        tx.execute("INSERT INTO face_person(image_id,ordinal,person_id,confirmed) VALUES(?,?,?,0) ON CONFLICT(image_id,ordinal) DO UPDATE SET person_id=excluded.person_id, confirmed=CASE WHEN face_person.person_id=excluded.person_id THEN face_person.confirmed ELSE 0 END",params![face.image_id.to_string(),face.ordinal,person_id]).map_err(sql)?;
        tx.commit().map_err(sql)
    }
    /// Set per-face confirmation; unassigned faces are errors.
    pub fn confirm_face(&self, face: FaceKey, confirmed: bool) -> EngineResult<()> {
        require_changed(
            self.0
                .conn
                .execute(
                    "UPDATE face_person SET confirmed=? WHERE image_id=? AND ordinal=?",
                    params![confirmed, face.image_id.to_string(), face.ordinal],
                )
                .map_err(sql)?,
            "face assignment",
            &format!("{}:{}", face.image_id, face.ordinal),
        )
    }
    /// Assigned faces in ordinal order. Unassigned detector faces are omitted.
    pub fn face_assignments(&self, image_id: ImageId) -> EngineResult<Vec<FaceAssignment>> {
        let mut stmt = self.0.conn.prepare("SELECT fp.ordinal,p.id,p.name,fp.confirmed FROM face_person fp JOIN person p ON p.id=fp.person_id WHERE fp.image_id=? ORDER BY fp.ordinal").map_err(sql)?;
        stmt.query_map([image_id.to_string()], |r| {
            Ok(FaceAssignment {
                face: FaceKey {
                    image_id,
                    ordinal: r.get(0)?,
                },
                person_id: r.get(1)?,
                person_name: r.get(2)?,
                confirmed: r.get(3)?,
            })
        })
        .map_err(sql)?
        .collect::<rusqlite::Result<_>>()
        .map_err(sql)
    }
    /// Indexed distinct images in stable ID order. Unknown people return empty.
    /// Zero limit defaults to 100. Confirmation filtering precedes deduplication.
    pub fn images_with_person(
        &self,
        person_id: &str,
        confirmed_only: bool,
        limit: usize,
        offset: usize,
    ) -> EngineResult<Vec<ImageId>> {
        let limit = i64::try_from(if limit == 0 { 100 } else { limit })
            .map_err(|_| EngineError::invalid("limit", "too large"))?;
        let offset =
            i64::try_from(offset).map_err(|_| EngineError::invalid("offset", "too large"))?;
        let query = if confirmed_only {
            "SELECT DISTINCT image_id FROM face_person WHERE person_id=? AND confirmed=1 ORDER BY image_id LIMIT ? OFFSET ?"
        } else {
            "SELECT DISTINCT image_id FROM face_person WHERE person_id=? ORDER BY image_id LIMIT ? OFFSET ?"
        };
        let mut stmt = self.0.conn.prepare(query).map_err(sql)?;
        stmt.query_map(params![person_id, limit, offset], |r| r.get::<_, String>(0))
            .map_err(sql)?
            .map(|r| crate::parse_id(&r.map_err(sql)?).map_err(Into::into))
            .collect()
    }

    /// Create a cluster; duplicate/blank IDs and invalid descriptors are errors.
    pub fn create_person(
        &self,
        id: &str,
        name: Option<&str>,
        medoid: Option<&[f32]>,
    ) -> EngineResult<()> {
        if id.trim().is_empty()
            || medoid.is_some_and(|m| m.len() != 128 || m.iter().any(|v| !v.is_finite()))
        {
            return Err(EngineError::invalid(
                "person",
                "nonblank ID and finite 128-component medoid required",
            ));
        }
        let medoid = medoid
            .map(serde_json::to_string)
            .transpose()
            .map_err(|e| EngineError::invalid("medoid", e.to_string()))?;
        self.0
            .conn
            .execute(
                "INSERT INTO person(id,name,medoid) VALUES(?,?,?)",
                params![id, name, medoid],
            )
            .map_err(sql)?;
        Ok(())
    }
    /// List clusters in stable ID order, including empty clusters.
    pub fn people(&self) -> EngineResult<Vec<Person>> {
        let mut stmt = self
            .0
            .conn
            .prepare("SELECT id,name,medoid FROM person ORDER BY id")
            .map_err(sql)?;
        let rows = stmt
            .query_map([], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, Option<String>>(1)?,
                    r.get::<_, Option<String>>(2)?,
                ))
            })
            .map_err(sql)?;
        rows.map(|r| {
            let (id, name, medoid) = r.map_err(sql)?;
            let medoid = medoid
                .map(|m| serde_json::from_str(&m))
                .transpose()
                .map_err(|e| EngineError::invalid("medoid", e.to_string()))?;
            Ok(Person { id, name, medoid })
        })
        .collect()
    }
    /// Rename (or clear a name); assignments resolve the current name by join.
    pub fn name_person(&self, id: &str, name: Option<&str>) -> EngineResult<()> {
        require_changed(
            self.0
                .conn
                .execute("UPDATE person SET name=? WHERE id=?", params![name, id])
                .map_err(sql)?,
            "person",
            id,
        )
    }
}
