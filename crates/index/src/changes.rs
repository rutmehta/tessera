//! Catalog change feed (M2-28). SQLite triggers (migration 007) append one row per
//! catalog write to `change_log` inside the writer's transaction, whichever
//! connection or crate made it (scans, sidecar writes, scores, prunes, tether
//! ingest). Readers pull a coalesced batch since the last sequence they applied.
use super::*;
use engine_api::error::{EngineError, EngineResult};
use std::collections::HashMap;

/// Rows kept after trimming; a pull older than the trimmed range must reload.
const KEEP: i64 = 100_000;
/// Trim once the log holds this many rows.
const TRIM_AT: i64 = 200_000;

/// Which parts of an image's catalog row changed. A bit set; see the constants.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct ChangeFields(pub u32);
impl ChangeFields {
    /// File path, size or modification time (new pixels or a move).
    pub const FILE: Self = Self(1);
    pub const CAPTURE_TIME: Self = Self(2);
    /// Camera, lens, caption, GPS or orientation.
    pub const METADATA: Self = Self(4);
    pub const SELECTION: Self = Self(8);
    /// Recipe hash (a develop edit was saved).
    pub const RECIPE: Self = Self(16);
    pub const KEYWORDS: Self = Self(32);
    /// AI signals (quality, faces, sharpness, ...).
    pub const SCORES: Self = Self(64);
    pub const ALL: Self = Self(127);
    pub fn contains(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }
    pub fn intersects(self, other: Self) -> bool {
        self.0 & other.0 != 0
    }
    pub fn is_empty(self) -> bool {
        self.0 == 0
    }
}
impl std::ops::BitOr for ChangeFields {
    type Output = Self;
    fn bitor(self, rhs: Self) -> Self {
        Self(self.0 | rhs.0)
    }
}
impl std::ops::BitOrAssign for ChangeFields {
    fn bitor_assign(&mut self, rhs: Self) {
        self.0 |= rhs.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChangeKind {
    Added,
    Removed,
    Updated(ChangeFields),
}

/// Net change of one image. `seq` is the sequence of its latest contributing row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ImageChange {
    pub seq: u64,
    pub id: ImageId,
    pub kind: ChangeKind,
}

/// Changes after `from`, up to and including `to`, coalesced per image and
/// ordered by each image's latest sequence. `reset` means rows after `from`
/// were trimmed (or the catalog was replaced): reload instead of applying.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ChangeBatch {
    pub from: u64,
    pub to: u64,
    pub reset: bool,
    pub changes: Vec<ImageChange>,
}
impl ChangeBatch {
    pub fn is_empty(&self) -> bool {
        self.changes.is_empty() && !self.reset
    }
}

fn sql(error: rusqlite::Error) -> EngineError {
    IndexError::Sql(error).into()
}

/// Latest sequence ever assigned (0 for a new catalog). AUTOINCREMENT keeps it
/// monotonic across deletes and connections.
pub(crate) fn head(conn: &Connection) -> rusqlite::Result<u64> {
    Ok(conn
        .query_row(
            "SELECT seq FROM sqlite_sequence WHERE name='change_log'",
            [],
            |r| r.get::<_, i64>(0),
        )
        .optional()?
        .unwrap_or(0)
        .max(0) as u64)
}

/// Folds raw rows `(seq, id, kind, fields)` in sequence order into net changes.
pub(crate) fn coalesce(
    rows: impl IntoIterator<Item = (u64, String, i64, u32)>,
) -> Vec<ImageChange> {
    struct Net {
        first: Option<bool>,
        last: Option<bool>,
        structural: usize,
        fields: ChangeFields,
        seq: u64,
    }
    let mut nets: HashMap<String, Net> = HashMap::new();
    for (seq, id, kind, fields) in rows {
        let net = nets.entry(id).or_insert(Net {
            first: None,
            last: None,
            structural: 0,
            fields: ChangeFields::default(),
            seq,
        });
        net.seq = seq;
        match kind {
            1 | 2 => {
                let added = kind == 1;
                net.first.get_or_insert(added);
                net.last = Some(added);
                net.structural += 1;
            }
            _ => net.fields |= ChangeFields(fields),
        }
    }
    let mut out: Vec<ImageChange> = nets
        .into_iter()
        .filter_map(|(id, net)| {
            // First structural event Added: it did not exist before the batch.
            let existed = net.first != Some(true);
            let exists = net.last != Some(false);
            let kind = match (existed, exists) {
                (false, true) => ChangeKind::Added,
                (true, false) => ChangeKind::Removed,
                (false, false) => return None,
                // Removed and re-added within the batch: every field may differ.
                (true, true) if net.structural > 0 => ChangeKind::Updated(ChangeFields::ALL),
                (true, true) if net.fields.is_empty() => return None,
                (true, true) => ChangeKind::Updated(net.fields),
            };
            let id = u128::from_str_radix(&id, 16).ok().map(ImageId)?;
            Some(ImageChange {
                seq: net.seq,
                id,
                kind,
            })
        })
        .collect();
    out.sort_by_key(|c| c.seq);
    out
}

impl Index {
    /// Latest change sequence; hosts start pulling from here after a full load.
    pub fn change_head(&self) -> EngineResult<u64> {
        head(&self.0.conn).map_err(sql)
    }

    /// Net changes after `seq` (see `ChangeBatch`).
    pub fn changes_since(&self, seq: u64) -> EngineResult<ChangeBatch> {
        let conn = &self.0.conn;
        // One read transaction: the head and the rows come from the same snapshot.
        let tx = conn.unchecked_transaction().map_err(sql)?;
        let to = head(&tx).map_err(sql)?;
        let trimmed: i64 = tx
            .query_row(
                "SELECT value FROM change_meta WHERE key='trimmed'",
                [],
                |r| r.get(0),
            )
            .optional()
            .map_err(sql)?
            .unwrap_or(0);
        let mut batch = ChangeBatch {
            from: seq,
            to,
            ..Default::default()
        };
        if seq > to || (seq as i64) < trimmed {
            batch.reset = true;
            return Ok(batch);
        }
        if seq == to {
            return Ok(batch);
        }
        let rows = {
            let mut stmt = tx
                .prepare_cached(
                    "SELECT seq,image_id,kind,fields FROM change_log WHERE seq>? AND seq<=? ORDER BY seq",
                )
                .map_err(sql)?;
            stmt.query_map(params![seq as i64, to as i64], |r| {
                Ok((
                    r.get::<_, i64>(0)? as u64,
                    r.get::<_, String>(1)?,
                    r.get::<_, i64>(2)?,
                    r.get::<_, i64>(3)? as u32,
                ))
            })
            .map_err(sql)?
            .collect::<rusqlite::Result<Vec<_>>>()
            .map_err(sql)?
        };
        tx.finish().map_err(sql)?;
        batch.changes = coalesce(rows);
        Ok(batch)
    }

    /// The indexed image whose original is `path` (as stored: canonical).
    pub fn image_at(&self, path: &Path) -> EngineResult<Option<ImageId>> {
        self.0
            .conn
            .query_row(
                "SELECT i.id FROM image i JOIN file f ON f.id=i.file_id WHERE f.path=?",
                [path.to_string_lossy()],
                |r| r.get::<_, String>(0),
            )
            .optional()
            .map_err(sql)?
            .map(|id| parse_id(&id).map_err(Into::into))
            .transpose()
    }

    /// Sort key of the default queue order (`search`): capture time as stored,
    /// then id. Missing images sort first, like missing capture times.
    pub fn order_keys(&self, ids: &[ImageId]) -> EngineResult<Vec<(Option<String>, String)>> {
        let mut stmt = self
            .0
            .conn
            .prepare_cached("SELECT capture_time FROM image WHERE id=?")
            .map_err(sql)?;
        ids.iter()
            .map(|id| {
                let key = id.to_string();
                let time = stmt
                    .query_row([&key], |r| r.get::<_, Option<String>>(0))
                    .optional()
                    .map_err(sql)?
                    .flatten();
                Ok((time, key))
            })
            .collect()
    }
}

impl Core {
    /// Bounds the log; called after scans (the main writer).
    pub(crate) fn trim_changes(&self) -> Result<()> {
        let (count, min): (i64, Option<i64>) =
            self.conn
                .query_row("SELECT count(*),min(seq) FROM change_log", [], |r| {
                    Ok((r.get(0)?, r.get(1)?))
                })?;
        if count < TRIM_AT {
            return Ok(());
        }
        let cut = head(&self.conn)? as i64 - KEEP;
        if min.is_some_and(|min| min <= cut) {
            let tx = self.conn.unchecked_transaction()?;
            tx.execute("DELETE FROM change_log WHERE seq<=?", [cut])?;
            tx.execute(
                "UPDATE change_meta SET value=max(value,?) WHERE key='trimmed'",
                [cut],
            )?;
            tx.commit()?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(seq: u64, id: u128, kind: i64, fields: u32) -> (u64, String, i64, u32) {
        (seq, ImageId(id).to_string(), kind, fields)
    }

    #[test]
    fn coalescing_nets_out_each_image() {
        let changes = coalesce([
            row(1, 1, 1, 0),  // 1 added
            row(2, 1, 3, 8),  // ... then updated: still Added
            row(3, 2, 3, 8),  // 2 selection
            row(4, 2, 3, 16), // 2 recipe
            row(5, 3, 1, 0),  // 3 added
            row(6, 3, 2, 0),  // ... and removed: nothing
            row(7, 4, 3, 64), // 4 updated
            row(8, 4, 2, 0),  // ... then removed
            row(9, 4, 3, 64), // cascaded delete after removal
            row(10, 5, 2, 0), // 5 removed and re-added
            row(11, 5, 1, 0),
        ]);
        assert_eq!(
            changes,
            vec![
                ImageChange {
                    seq: 2,
                    id: ImageId(1),
                    kind: ChangeKind::Added
                },
                ImageChange {
                    seq: 4,
                    id: ImageId(2),
                    kind: ChangeKind::Updated(ChangeFields::SELECTION | ChangeFields::RECIPE)
                },
                ImageChange {
                    seq: 9,
                    id: ImageId(4),
                    kind: ChangeKind::Removed
                },
                ImageChange {
                    seq: 11,
                    id: ImageId(5),
                    kind: ChangeKind::Updated(ChangeFields::ALL)
                },
            ]
        );
    }
}
