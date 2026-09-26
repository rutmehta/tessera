//! Library change feed over the bridge (M2-28).
//!
//! Pull: `Engine::changes_since(sequence)` returns net per-image changes, and a
//! `CullSession::sync_changes()` applies them to its queue in place. Push:
//! `EngineEvent::LibraryChanged { sequence }` after the engine's own writes
//! (scans, sidecar writes, tether frames) and, while a listener is set, from a
//! watcher that notices writes made by other connections or processes. The
//! event only says "pull"; hosts keep the last sequence they applied, so
//! duplicate or coalesced notifications are harmless.
use crate::{Engine, EngineEvent, Result};
use index::{ChangeFields, ChangeKind};
use rusqlite::{Connection, OpenFlags};
use std::{
    sync::{Weak, atomic::Ordering},
    time::Duration,
};

/// Poll interval of the background watcher.
const WATCH_INTERVAL: Duration = Duration::from_millis(250);

/// What changed about an image (all false for adds and removes).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, uniffi::Record)]
pub struct ChangedFields {
    /// File path, size or modification time: new pixels or a move.
    pub file: bool,
    pub capture_time: bool,
    /// Camera, lens, caption, GPS or orientation.
    pub metadata: bool,
    pub selection: bool,
    /// A develop edit was saved (thumbnails and derived status change).
    pub recipe: bool,
    pub keywords: bool,
    /// AI signals (focus, faces, quality); a group's suggested best may change.
    pub scores: bool,
}
impl From<ChangeFields> for ChangedFields {
    fn from(f: ChangeFields) -> Self {
        Self {
            file: f.contains(ChangeFields::FILE),
            capture_time: f.contains(ChangeFields::CAPTURE_TIME),
            metadata: f.contains(ChangeFields::METADATA),
            selection: f.contains(ChangeFields::SELECTION),
            recipe: f.contains(ChangeFields::RECIPE),
            keywords: f.contains(ChangeFields::KEYWORDS),
            scores: f.contains(ChangeFields::SCORES),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum LibraryChangeKind {
    Added,
    Removed,
    Updated,
}

/// Net change of one image since the pull's starting sequence.
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct LibraryChange {
    /// Sequence of the image's latest contributing write.
    pub sequence: u64,
    pub image_id: String,
    pub kind: LibraryChangeKind,
    pub fields: ChangedFields,
}

/// Changes after `from` up to `sequence`, one entry per image in write order.
/// `reset`: the range is no longer available (trimmed, or another catalog);
/// reload instead of applying.
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct LibraryChanges {
    pub from: u64,
    pub sequence: u64,
    pub reset: bool,
    pub changes: Vec<LibraryChange>,
}

impl From<index::ChangeBatch> for LibraryChanges {
    fn from(batch: index::ChangeBatch) -> Self {
        Self {
            from: batch.from,
            sequence: batch.to,
            reset: batch.reset,
            changes: batch
                .changes
                .into_iter()
                .map(|c| {
                    let (kind, fields) = match c.kind {
                        ChangeKind::Added => (LibraryChangeKind::Added, ChangedFields::default()),
                        ChangeKind::Removed => {
                            (LibraryChangeKind::Removed, ChangedFields::default())
                        }
                        ChangeKind::Updated(f) => (LibraryChangeKind::Updated, f.into()),
                    };
                    LibraryChange {
                        sequence: c.seq,
                        image_id: c.id.to_string(),
                        kind,
                        fields,
                    }
                })
                .collect(),
        }
    }
}

fn head(conn: &Connection) -> rusqlite::Result<u64> {
    use rusqlite::OptionalExtension;
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

impl Engine {
    /// Emits `LibraryChanged` when the catalog moved past the last notified
    /// sequence. Never call it while holding the catalog lock.
    pub(crate) fn notify_changes(&self) {
        let sequence = {
            let conn = self.heads.lock().unwrap_or_else(|e| e.into_inner());
            head(&conn)
        };
        if let Ok(sequence) = sequence {
            self.notify_sequence(sequence);
        }
    }

    fn notify_sequence(&self, sequence: u64) {
        if self.notified.fetch_max(sequence, Ordering::AcqRel) < sequence {
            self.emit(EngineEvent::LibraryChanged { sequence });
        }
    }

    /// Starts the watcher once: it covers writers the engine does not see
    /// (ML jobs, the MCP server, a second engine such as a catalog import).
    /// It holds only a weak reference and ends with the engine.
    pub(crate) fn watch_changes(&self) {
        if self.watching.swap(true, Ordering::AcqRel) {
            return;
        }
        let weak: Weak<Self> = self.this.clone();
        let db = self.db.clone();
        let spawned = std::thread::Builder::new()
            .name("tessera-change-watch".into())
            .spawn(move || {
                let Ok(conn) = Connection::open_with_flags(&db, OpenFlags::SQLITE_OPEN_READ_ONLY)
                else {
                    return;
                };
                loop {
                    std::thread::sleep(WATCH_INTERVAL);
                    let Some(engine) = weak.upgrade() else { return };
                    if let Ok(sequence) = head(&conn) {
                        engine.notify_sequence(sequence);
                    }
                }
            });
        if spawned.is_err() {
            self.watching.store(false, Ordering::Release);
        }
    }
}

#[uniffi::export]
impl Engine {
    /// Latest change sequence of the catalog (0 when nothing was ever written).
    pub fn change_sequence(&self) -> Result<u64> {
        Ok(self.lock()?.index.change_head()?)
    }
    /// Net changes after `sequence` (see `LibraryChanges`).
    pub fn changes_since(&self, sequence: u64) -> Result<LibraryChanges> {
        Ok(self.lock()?.index.changes_since(sequence)?.into())
    }
    /// Drops the catalog rows of images whose files are gone (after "Delete
    /// from Disk"); images whose files still exist are kept. Open sessions see
    /// them leave through `sync_changes`. Returns the number removed.
    pub fn forget_missing(&self, image_ids: Vec<String>) -> Result<u32> {
        let ids = image_ids
            .iter()
            .map(|id| crate::parse_id(id))
            .collect::<Result<Vec<_>>>()?;
        let removed = self.lock()?.index.forget_missing(&ids)?;
        self.notify_changes();
        Ok(removed as u32)
    }
}
