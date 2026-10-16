//! OF-1: what a folder can do without its drive (`docs/design/offline/SPEC.md`).
//!
//! Everything here derives from the index and the files under
//! `smart-previews/`; there is no new state. The catalog lock is never held
//! across file work (ST-1): the file list comes from the reader connection,
//! which is released before any stat, journal or sidecar read. A detached or
//! hung volume therefore stalls only the caller, never the catalog.
//!
//! Coverage and candidates classify a preview from its files alone (journal
//! record, pixel file presence, the original's size and its sidecars against
//! the journal baseline); they never decode pixels or hash the original, which
//! is what makes a sweep over a folder cheap. `smart_preview_info` remains the
//! authoritative check and `ensure_smart_preview` uses it for the one photo it
//! is about to build.
use crate::{
    Engine, Result, SmartPreviewInfo, SmartPreviewState, catalog, failure, parse_id,
    smart_preview::optional_bytes, smart_preview_store::SmartPreviewJournal,
};
use engine_api::id::ImageId;
use std::{
    cmp::Ordering,
    fs,
    path::{Path, PathBuf},
};

/// How many of a folder's RAWs can be edited with the drive detached.
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct OfflineCoverage {
    pub folder: String,
    /// Mosaic RAW originals under `folder` (recursive, same rule as the cull
    /// session). Imported Lightroom Smart Previews are not originals.
    pub raws: u64,
    /// Of those, with a local preview that is not stale or conflicting.
    pub ready: u64,
    /// Local previews with journaled edits not yet written to the sidecars.
    pub pending_sync: u64,
    /// Stale, conflicting or failed previews: a person decides (Smart Previews menu).
    pub needs_attention: u64,
    /// On-disk size of the previews counted in `ready`, `pending_sync` and
    /// `needs_attention`.
    pub bytes: u64,
}

/// One indexed RAW under a folder, as the reader connection reports it.
struct RawRow {
    id: ImageId,
    path: PathBuf,
    capture_time: Option<String>,
    keep: bool,
}

/// What the local files say about one preview, without decoding it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Local {
    Missing,
    Ready,
    PendingSync,
    /// The original's size no longer matches the journal; `dirty` says
    /// whether unsynchronized edits stop a rebuild.
    Stale { dirty: bool },
    /// The original's sidecars drifted from the journal baseline.
    Conflict,
    /// Unreadable journal or absent pixels.
    Failed,
}

impl Local {
    /// A build (`build_smart_preview`) can act on it without losing an edit.
    fn buildable(self) -> bool {
        matches!(self, Local::Missing | Local::Stale { dirty: false })
    }
}

/// Mosaic RAW containers the scanner discovers (`index` folder discovery
/// minus the rendered formats). A linear DNG passes here and is refused by
/// the build with the engine's message.
fn is_raw_extension(path: &Path) -> bool {
    matches!(
        path.extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_ascii_lowercase()
            .as_str(),
        "arw" | "nef" | "cr2" | "cr3" | "raf" | "rw2" | "orf" | "dng"
    )
}

/// The index's own folder pattern: `<folder>/*` with GLOB metacharacters escaped.
fn folder_glob(folder: &str) -> String {
    let folder = folder
        .trim_end_matches('/')
        .replace('[', "[[]")
        .replace('*', "[*]")
        .replace('?', "[?]");
    format!("{folder}/*")
}

/// Newest first. RAW capture times are Unix seconds, EXIF ones ISO local
/// date-times; two numbers compare numerically, anything else as text.
/// Unknown times sort last.
fn newest_first(a: &Option<String>, b: &Option<String>) -> Ordering {
    match (a, b) {
        (Some(a), Some(b)) => match (a.parse::<i64>(), b.parse::<i64>()) {
            (Ok(a), Ok(b)) => b.cmp(&a),
            _ => b.cmp(a),
        },
        (Some(_), None) => Ordering::Less,
        (None, Some(_)) => Ordering::Greater,
        (None, None) => Ordering::Equal,
    }
}

/// Bytes of the regular files directly in `dir` (journal, pixels, pending
/// intent, leftover temporaries). Absent directory: zero.
fn directory_bytes(dir: &Path) -> u64 {
    let Ok(entries) = fs::read_dir(dir) else {
        return 0;
    };
    entries
        .flatten()
        .filter_map(|entry| fs::symlink_metadata(entry.path()).ok())
        .filter(|meta| meta.file_type().is_file())
        .map(|meta| meta.len())
        .sum()
}

/// Bytes of every regular file under `dir`, recursively, without following
/// symlinks. Absent directory: zero.
fn tree_bytes(dir: &Path) -> u64 {
    let Ok(entries) = fs::read_dir(dir) else {
        return 0;
    };
    let mut total = 0;
    for entry in entries.flatten() {
        let Ok(meta) = fs::symlink_metadata(entry.path()) else {
            continue;
        };
        if meta.file_type().is_dir() {
            total += tree_bytes(&entry.path());
        } else if meta.file_type().is_file() {
            total += meta.len();
        }
    }
    total
}

impl Engine {
    fn previews_root(&self) -> Result<PathBuf> {
        Ok(self.support_dir()?.join("smart-previews"))
    }

    /// The folder's indexed RAWs. Holds the reader connection only while the
    /// rows are copied out; never the catalog.
    fn raw_rows(&self, folder: &str) -> Result<Vec<RawRow>> {
        let folder = folder.trim_end_matches('/');
        let path = Path::new(folder);
        if folder.is_empty()
            || !path.is_absolute()
            || path
                .components()
                .any(|c| matches!(c, std::path::Component::ParentDir))
        {
            return Err(failure("expected absolute catalog folder without .."));
        }
        let rows = {
            let reader = self.reader.lock()?;
            let mut statement = reader.prepare(
                "SELECT i.id,f.path,i.capture_time,COALESCE(s.decision,''),EXISTS(SELECT 1 FROM metadata m WHERE m.image_id=i.id AND m.key=?2) \
                 FROM image i JOIN file f ON f.id=i.file_id LEFT JOIN selection s ON s.image_id=i.id \
                 WHERE f.path GLOB ?1 ORDER BY i.id",
            )?;
            statement
                .query_map(
                    rusqlite::params![folder_glob(folder), catalog::LISTING_ORIGINAL_KEY],
                    |r| {
                        Ok((
                            r.get::<_, String>(0)?,
                            r.get::<_, String>(1)?,
                            r.get::<_, Option<String>>(2)?,
                            r.get::<_, String>(3)?,
                            r.get::<_, bool>(4)?,
                        ))
                    },
                )?
                .collect::<rusqlite::Result<Vec<_>>>()?
        };
        let mut out = Vec::new();
        for (id, path, capture_time, decision, lightroom_proxy) in rows {
            let path = PathBuf::from(path);
            if lightroom_proxy || !is_raw_extension(&path) {
                continue;
            }
            out.push(RawRow {
                id: parse_id(&id)?,
                path,
                capture_time,
                keep: decision == "keep",
            });
        }
        Ok(out)
    }

    /// Classify one preview from its files (see the module notes). Any I/O
    /// error on the preview's own files is `Failed`; the original being
    /// unreachable is not an error, it is the case this module is for.
    fn local_state(&self, previews: &Path, row: &RawRow) -> Local {
        let dir = previews.join(row.id.to_string());
        if !fs::symlink_metadata(dir.join("journal.json")).is_ok_and(|m| m.file_type().is_file()) {
            return Local::Missing;
        }
        let Ok(support) = self.support_dir() else {
            return Local::Failed;
        };
        let Ok((_, snapshot)) = SmartPreviewJournal::read_local_snapshot(support, row.id) else {
            return Local::Failed;
        };
        if !fs::symlink_metadata(dir.join("pixels.tsp"))
            .is_ok_and(|m| m.file_type().is_file() && m.len() > 0)
        {
            return Local::Failed;
        }
        // The original, when reachable: a size change is the cheap stale
        // signal (the full digest is checked when the preview is opened); the
        // sidecars drifting from the baseline is a conflict, the same rule
        // `smart_preview_info` applies.
        if let Ok(meta) = fs::metadata(&row.path)
            && meta.is_file()
        {
            if meta.len() != snapshot.source_len {
                return Local::Stale {
                    dirty: snapshot.dirty,
                };
            }
            let recipe = optional_bytes(sidecar::Sidecar::paths(&row.path).recipe);
            let xmp = optional_bytes(catalog::xmp_path(&row.path));
            match (recipe, xmp) {
                (Ok(recipe), Ok(xmp)) => {
                    if recipe != snapshot.baseline_recipe || xmp != snapshot.baseline_xmp {
                        return Local::Conflict;
                    }
                }
                _ => return Local::Failed,
            }
        }
        if snapshot.dirty {
            Local::PendingSync
        } else {
            Local::Ready
        }
    }
}

#[uniffi::export]
impl Engine {
    /// Cheap: the file list from the reader connection, then stat and journal
    /// reads with no catalog lock held.
    pub fn offline_coverage(&self, folder: String) -> Result<OfflineCoverage> {
        let rows = self.raw_rows(&folder)?;
        let previews = self.previews_root()?;
        let mut coverage = OfflineCoverage {
            folder,
            raws: rows.len() as u64,
            ready: 0,
            pending_sync: 0,
            needs_attention: 0,
            bytes: 0,
        };
        for row in &rows {
            let state = self.local_state(&previews, row);
            match state {
                Local::Missing => continue,
                Local::Ready => coverage.ready += 1,
                Local::PendingSync => coverage.pending_sync += 1,
                Local::Stale { .. } | Local::Conflict | Local::Failed => {
                    coverage.needs_attention += 1
                }
            }
            coverage.bytes += directory_bytes(&previews.join(row.id.to_string()));
        }
        Ok(coverage)
    }

    /// RAW image ids under `folder` that a build can act on (no local preview,
    /// or a clean stale one), ordered: keep decisions first, then newest
    /// capture first. `limit` 0 means all. Conflicting, failed and dirty
    /// stale previews are not candidates: a build would refuse them, and they
    /// are counted in `needs_attention` for a person to resolve.
    pub fn offline_candidates(&self, folder: String, limit: u32) -> Result<Vec<String>> {
        let rows = self.raw_rows(&folder)?;
        let previews = self.previews_root()?;
        let mut candidates: Vec<&RawRow> = rows
            .iter()
            .filter(|row| self.local_state(&previews, row).buildable())
            .collect();
        candidates.sort_by(|a, b| {
            b.keep
                .cmp(&a.keep)
                .then_with(|| newest_first(&a.capture_time, &b.capture_time))
                .then_with(|| a.id.cmp(&b.id))
        });
        if limit > 0 {
            candidates.truncate(limit as usize);
        }
        Ok(candidates.iter().map(|row| row.id.to_string()).collect())
    }

    /// The existing preview's info when one is present and usable (ready,
    /// original offline, or dirty awaiting sync); otherwise the build. A
    /// clean stale preview is replaced by the build; a conflict, a failed
    /// preview or a dirty stale one is refused by it with the engine's message.
    pub fn ensure_smart_preview(&self, image_id: String) -> Result<SmartPreviewInfo> {
        let info = self.smart_preview_info(image_id.clone())?;
        match info.state {
            SmartPreviewState::Ready
            | SmartPreviewState::OriginalOffline
            | SmartPreviewState::Dirty => Ok(info),
            SmartPreviewState::Missing
            | SmartPreviewState::Stale
            | SmartPreviewState::Conflict
            | SmartPreviewState::Failed => self.build_smart_preview(image_id),
        }
    }

    /// Ids whose journal has edits not yet synchronized, from
    /// `smart-previews/<image id>/journal.json`, sorted. An unreadable journal is not
    /// listed: nothing can be published from it.
    pub fn pending_sync_smart_previews(&self) -> Result<Vec<String>> {
        let previews = self.previews_root()?;
        let support = self.support_dir()?;
        let entries = match fs::read_dir(&previews) {
            Ok(entries) => entries,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(e) => return Err(e.into()),
        };
        let mut ids = Vec::new();
        for entry in entries {
            let entry = entry?;
            let name = entry.file_name();
            let Ok(id) = parse_id(&name.to_string_lossy()) else {
                continue;
            };
            if let Ok((_, snapshot)) = SmartPreviewJournal::read_local_snapshot(support, id)
                && snapshot.dirty
            {
                ids.push(id.to_string());
            }
        }
        ids.sort();
        Ok(ids)
    }

    /// Bytes under `smart-previews/`.
    pub fn smart_preview_storage(&self) -> Result<u64> {
        Ok(tree_bytes(&self.previews_root()?))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Decision, ImageQuery, Selection};
    use std::time::{Duration, Instant};

    #[test]
    fn newest_first_orders_unix_seconds_numerically_and_unknown_last() {
        let t = |s: &str| Some(s.to_string());
        assert_eq!(newest_first(&t("1700000000"), &t("900000000")), Ordering::Less);
        assert_eq!(newest_first(&t("900000000"), &t("1700000000")), Ordering::Greater);
        assert_eq!(
            newest_first(&t("2026-05-01T10:00:00"), &t("2026-04-01T10:00:00")),
            Ordering::Less
        );
        assert_eq!(newest_first(&t("1"), &None), Ordering::Less);
        assert_eq!(newest_first(&None, &None), Ordering::Equal);
    }

    #[test]
    fn folder_glob_escapes_metacharacters_like_the_index() {
        assert_eq!(folder_glob("/a/b/"), "/a/b/*");
        assert_eq!(folder_glob("/a/[x]*?"), "/a/[[]x][*][?]/*");
    }

    #[test]
    fn storage_and_pending_are_zero_without_a_store() {
        let dir = tempfile::tempdir().unwrap();
        let engine = Engine::open(dir.path().to_string_lossy().into()).unwrap();
        assert_eq!(engine.smart_preview_storage().unwrap(), 0);
        assert!(engine.pending_sync_smart_previews().unwrap().is_empty());
        assert!(engine.offline_coverage("relative".into()).is_err());
        assert!(engine.offline_coverage("/a/../b".into()).is_err());
    }

    /// ST-1: the sweep answers while a worker holds the catalog, because it
    /// takes the reader alone and then works on files with no engine lock.
    /// With the Sony fixture present the folder holds one RAW, so the stat
    /// and journal path is exercised; without it the query path still is.
    #[test]
    fn coverage_and_candidates_never_wait_on_the_catalog() {
        let dir = tempfile::tempdir().unwrap();
        let photos = dir.path().join("photos");
        fs::create_dir(&photos).unwrap();
        image::RgbImage::new(32, 16)
            .save(photos.join("one.jpg"))
            .unwrap();
        let fixture =
            test_fixtures::raw::file(&test_fixtures::current_test(), "sony-arw.ARW");
        if let Some(fixture) = &fixture {
            fs::copy(fixture, photos.join("one.ARW")).unwrap();
        }
        let engine = Engine::open(dir.path().join("support").to_string_lossy().into()).unwrap();
        let folder = engine
            .index_folder(photos.to_string_lossy().into_owned())
            .unwrap()
            .path;
        let raws = engine.list_images(ImageQuery::default()).unwrap();
        for row in &raws {
            if row.path.ends_with(".ARW") {
                engine
                    .set_selection(
                        row.id.clone(),
                        Selection {
                            decision: Decision::Keep,
                            grade: None,
                            mark: None,
                        },
                    )
                    .unwrap();
            }
        }
        let expected_raws = u64::from(fixture.is_some());
        std::thread::scope(|s| {
            let held = engine.lock().unwrap();
            let (send, receive) = std::sync::mpsc::channel();
            let (engine, folder) = (&engine, &folder);
            s.spawn(move || {
                let started = Instant::now();
                let coverage = engine.offline_coverage(folder.clone()).unwrap();
                let candidates = engine.offline_candidates(folder.clone(), 0).unwrap();
                let pending = engine.pending_sync_smart_previews().unwrap();
                let storage = engine.smart_preview_storage().unwrap();
                let _ = send.send((started.elapsed(), coverage, candidates, pending, storage));
            });
            let (elapsed, coverage, candidates, pending, storage) = receive
                .recv_timeout(Duration::from_secs(5))
                .expect("the sweep must not wait on the catalog lock");
            assert!(elapsed < Duration::from_secs(5));
            assert_eq!(coverage.raws, expected_raws);
            assert_eq!(coverage.ready + coverage.pending_sync + coverage.needs_attention, 0);
            assert_eq!(candidates.len(), expected_raws as usize);
            assert!(pending.is_empty());
            assert_eq!(storage, 0);
            drop(held);
        });
    }
}
