//! Roots remembered by volume (CS-1). See `volume.rs` for the identity and
//! `010_volumes.sql` for the columns.
use crate::{Core, Index, IndexError, Result, RootLocation, Volume, VolumeResolver};
use engine_api::error::{EngineError, EngineResult};
use rusqlite::{OptionalExtension, params};
use std::path::{Path, PathBuf};

/// One indexed root. `path` is where it was last seen; `location` is what
/// identifies it across mount points.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RootRecord {
    pub id: i64,
    pub path: PathBuf,
    pub location: RootLocation,
    /// Indexed images under this root (by path prefix, whatever root indexed them).
    pub images: u64,
}
impl RootRecord {
    /// Where the root is right now, given the mounted volumes; `None` while
    /// its volume is not attached.
    pub fn locate(&self, mounted: &[Volume]) -> Option<PathBuf> {
        self.location.locate(mounted)
    }
}

fn sql(error: rusqlite::Error) -> EngineError {
    IndexError::Sql(error).into()
}

impl Core {
    /// Upserts the root row for a canonical directory. A root known by the
    /// same volume and relative path but a different mount point is relocated
    /// first (every folder and file path under it is rewritten), so a drive
    /// that mounts elsewhere keeps its catalog rows and image ids.
    pub(crate) fn register_root(&self, root: &Path) -> Result<i64> {
        let path = root.to_string_lossy().into_owned();
        let location = RootLocation::resolve(self.volumes.as_ref(), root)?;
        if let Some(uuid) = &location.volume_uuid {
            let stale: Option<String> = self
                .conn
                .query_row(
                    "SELECT path FROM root WHERE volume_uuid=? AND relative_path=? AND path!=?",
                    params![uuid, location.relative_path, path],
                    |r| r.get(0),
                )
                .optional()?;
            if let Some(old) = stale {
                self.relocate_paths(Path::new(&old), root)?;
            }
        }
        let mount_hint = location.volume_uuid.as_ref().map(|_| path.clone());
        self.conn.execute(
            "INSERT INTO root(path,volume_uuid,relative_path,mount_hint) VALUES(?1,?2,?3,?4)
             ON CONFLICT(path) DO UPDATE SET volume_uuid=excluded.volume_uuid,
             relative_path=excluded.relative_path,mount_hint=excluded.mount_hint
             WHERE root.volume_uuid IS NOT excluded.volume_uuid
             OR root.relative_path IS NOT excluded.relative_path
             OR root.mount_hint IS NOT excluded.mount_hint",
            params![path, location.volume_uuid, location.relative_path, mount_hint],
        )?;
        Ok(self
            .conn
            .query_row("SELECT id FROM root WHERE path=?", [&path], |r| r.get(0))?)
    }

    /// Rewrites every root, folder and file path under `old` to live under
    /// `new`. File-path triggers report each image as moved.
    pub(crate) fn relocate_paths(&self, old: &Path, new: &Path) -> Result<()> {
        let old = old.to_string_lossy().into_owned();
        let new = new.to_string_lossy().into_owned();
        if old == new {
            return Ok(());
        }
        let tx = self.conn.unchecked_transaction()?;
        for table in ["file", "folder", "root"] {
            tx.execute(
                &format!(
                    "UPDATE {table} SET path=?2 || substr(path,length(?1)+1)
                     WHERE path=?1 OR substr(path,1,length(?1)+1)=?1 || '/'"
                ),
                params![old, new],
            )?;
        }
        tx.execute(
            "UPDATE root SET mount_hint=path WHERE volume_uuid IS NOT NULL AND (path=?1 OR substr(path,1,length(?1)+1)=?1 || '/')",
            [&new],
        )?;
        tx.commit()?;
        Ok(())
    }
}

impl Index {
    /// Replaces the volume resolver (tests, or a host that knows its volumes).
    pub fn set_volume_resolver(&mut self, volumes: Box<dyn VolumeResolver>) {
        self.0.volumes = volumes;
    }
    pub fn volumes(&self) -> &dyn VolumeResolver {
        self.0.volumes.as_ref()
    }

    /// Every indexed root, in path order, with its volume identity and image count.
    pub fn roots(&self) -> EngineResult<Vec<RootRecord>> {
        let mut stmt = self
            .0
            .conn
            .prepare(
                "SELECT r.id,r.path,r.volume_uuid,r.relative_path,
                 (SELECT count(*) FROM file f WHERE f.path=r.path OR substr(f.path,1,length(r.path)+1)=r.path || '/')
                 FROM root r ORDER BY r.path",
            )
            .map_err(sql)?;
        let rows = stmt
            .query_map([], |r| {
                Ok((
                    r.get::<_, i64>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, Option<String>>(2)?,
                    r.get::<_, Option<String>>(3)?,
                    r.get::<_, i64>(4)?,
                ))
            })
            .map_err(sql)?;
        rows.map(|row| {
            let (id, path, volume_uuid, relative_path, images) = row.map_err(sql)?;
            Ok(RootRecord {
                id,
                location: RootLocation {
                    // Rows indexed before migration 10 have no volume columns yet.
                    relative_path: match &volume_uuid {
                        Some(_) => relative_path.unwrap_or_default(),
                        None => path.clone(),
                    },
                    volume_uuid,
                },
                path: PathBuf::from(path),
                images: images.max(0) as u64,
            })
        })
        .collect()
    }

    /// Brings a root's stored paths to where its volume is mounted now and
    /// returns that path, or `None` while the volume is absent. Boot-volume
    /// roots are returned unchanged.
    pub fn locate_root(&self, root: &RootRecord) -> EngineResult<Option<PathBuf>> {
        let Some(current) = root.locate(&self.0.volumes.mounted()?) else {
            return Ok(None);
        };
        if current != root.path {
            self.0
                .relocate_paths(&root.path, &current)
                .map_err(EngineError::from)?;
        }
        Ok(Some(current))
    }

    /// Forgets a root row (its images stay indexed until pruned).
    pub fn forget_root(&self, id: i64) -> EngineResult<()> {
        self.0
            .conn
            .execute("DELETE FROM root WHERE id=?", [id])
            .map_err(sql)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{FixedVolumes, NoopMetadataProvider, NoopSidecarReader, Query};

    fn volume(uuid: &str, mount: &Path) -> Volume {
        Volume {
            uuid: uuid.into(),
            mount_point: mount.to_path_buf(),
        }
    }

    #[test]
    fn roots_follow_their_volume_to_a_new_mount_point() {
        let dir = tempfile::tempdir().unwrap();
        let dir = dir.path().canonicalize().unwrap();
        let first = dir.join("Rut photo");
        let second = dir.join("Rut photo-1");
        std::fs::create_dir_all(first.join("2026/wedding")).unwrap();
        std::fs::write(first.join("2026/wedding/a.jpg"), b"jpeg").unwrap();
        let mut index = Index::open(dir.join("index.sqlite")).unwrap();
        index.set_volume_resolver(Box::new(FixedVolumes(vec![volume("DRIVE-1", &first)])));
        let shoot = first.join("2026/wedding");
        index
            .scan(&shoot, &NoopSidecarReader, &NoopMetadataProvider)
            .unwrap();
        let roots = index.roots().unwrap();
        assert_eq!(roots.len(), 1);
        assert_eq!(roots[0].path, shoot);
        assert_eq!(roots[0].location.volume_uuid.as_deref(), Some("DRIVE-1"));
        assert_eq!(roots[0].location.relative_path, "2026/wedding");
        assert_eq!(roots[0].images, 1);
        let id = index.search(&Query::default()).unwrap()[0];

        // The drive comes back at another mount point: the root is located and
        // its rows move with it; no second root appears.
        std::fs::rename(&first, &second).unwrap();
        index.set_volume_resolver(Box::new(FixedVolumes(vec![volume("DRIVE-1", &second)])));
        let located = index.locate_root(&roots[0]).unwrap().unwrap();
        assert_eq!(located, second.join("2026/wedding"));
        assert_eq!(
            index.image_info(id).unwrap().path,
            second.join("2026/wedding/a.jpg")
        );
        let roots = index.roots().unwrap();
        assert_eq!(roots.len(), 1);
        assert_eq!(roots[0].path, located);
        assert_eq!(
            index
                .scan(&located, &NoopSidecarReader, &NoopMetadataProvider)
                .unwrap(),
            0,
            "relocated rows are up to date"
        );
        // Scanning the new mount point directly (without locate) relocates too.
        std::fs::rename(&second, &first).unwrap();
        index.set_volume_resolver(Box::new(FixedVolumes(vec![volume("DRIVE-1", &first)])));
        index
            .scan(&shoot, &NoopSidecarReader, &NoopMetadataProvider)
            .unwrap();
        assert_eq!(index.roots().unwrap().len(), 1);
        assert_eq!(index.image_info(id).unwrap().path, shoot.join("a.jpg"));
        // Detached: not located, remembered with its counts.
        index.set_volume_resolver(Box::new(FixedVolumes(vec![])));
        let roots = index.roots().unwrap();
        assert_eq!(index.locate_root(&roots[0]).unwrap(), None);
        assert_eq!(roots[0].images, 1);
        assert_eq!(roots[0].path, shoot);
    }

    #[test]
    fn boot_roots_keep_plain_paths() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a.jpg"), b"jpeg").unwrap();
        let mut index = Index::open(":memory:").unwrap();
        index.set_volume_resolver(Box::new(FixedVolumes(vec![])));
        index
            .scan(dir.path(), &NoopSidecarReader, &NoopMetadataProvider)
            .unwrap();
        let roots = index.roots().unwrap();
        let canonical = dir.path().canonicalize().unwrap();
        assert_eq!(roots[0].location.volume_uuid, None);
        assert_eq!(
            roots[0].location.relative_path,
            canonical.to_string_lossy()
        );
        assert_eq!(index.locate_root(&roots[0]).unwrap(), Some(canonical));
    }
}
