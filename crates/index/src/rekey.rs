//! Image-id migration (CS-1, engine-api 1.8). Files on external volumes are
//! identified by volume UUID and relative path; a catalog that assigned
//! path-derived ids re-keys them in place, keeping every row (decisions,
//! people, scores, understanding, exports, stacks) rather than recomputing.
use crate::{Core, Index, IndexError, Result, Volume, VolumeResolver, relative_path};
use engine_api::{
    error::{EngineError, EngineResult},
    id::ImageId,
};
use rusqlite::{OptionalExtension, params};
use std::path::{Path, PathBuf};

/// Tables keyed by `image_id` besides `image` itself. `photo_stack_member` is
/// host-created and may be absent.
const KEYED: &[&str] = &[
    "metadata",
    "image_keyword",
    "selection",
    "recipe_hash",
    "score",
    "export_log",
    "face",
    "face_person",
    "accepted_keyword",
    "understanding",
    "fts",
    "drive_stamp",
    "photo_stack_member",
];

/// A legacy (path-derived) image found on an external volume, with the id it
/// derives now.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LegacyImage {
    pub old: ImageId,
    pub new: ImageId,
    pub path: PathBuf,
}

/// The id a file derives from where it is: volume-relative on an external
/// volume, path-derived on the boot volume (CONTRACTS.md invariant 20).
pub fn image_id_for(volumes: &dyn VolumeResolver, canonical: &Path) -> EngineResult<ImageId> {
    Ok(image_id_on(
        volumes.volume_of(canonical)?.as_ref(),
        canonical,
    ))
}

pub(crate) fn image_id_on(volume: Option<&Volume>, canonical: &Path) -> ImageId {
    match volume {
        Some(v) => ImageId::for_volume_path(&v.uuid, &relative_path(&v.mount_point, canonical)),
        None => ImageId::for_path(&canonical.to_string_lossy()),
    }
}

fn sql(error: rusqlite::Error) -> EngineError {
    IndexError::Sql(error).into()
}

impl Core {
    pub(crate) fn meta(&self, key: &str) -> Result<Option<String>> {
        Ok(self
            .conn
            .query_row("SELECT value FROM catalog_meta WHERE key=?", [key], |r| {
                r.get(0)
            })
            .optional()?)
    }
    pub(crate) fn set_meta(&self, key: &str, value: &str) -> Result<()> {
        self.conn.execute(
            "INSERT INTO catalog_meta(key,value) VALUES(?,?) ON CONFLICT(key) DO UPDATE SET value=excluded.value",
            params![key, value],
        )?;
        Ok(())
    }

    /// Re-keys images in one transaction with foreign keys suspended (child
    /// rows are updated alongside their parent). The change feed reports each
    /// old id as removed and each new id as added.
    pub(crate) fn rekey_images(&self, pairs: &[(ImageId, ImageId)]) -> Result<()> {
        if pairs.is_empty() {
            return Ok(());
        }
        let has_stacks: bool = self.conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='photo_stack_member')",
            [],
            |r| r.get(0),
        )?;
        self.conn.pragma_update(None, "foreign_keys", "OFF")?;
        let result = (|| -> Result<()> {
            let tx = self.conn.unchecked_transaction()?;
            for (old, new) in pairs {
                let (old, new) = (old.to_string(), new.to_string());
                // The image triggers would log the update as an unrelated change.
                tx.execute("UPDATE image SET id=? WHERE id=?", params![new, old])?;
                for table in KEYED {
                    if *table == "photo_stack_member" && !has_stacks {
                        continue;
                    }
                    tx.execute(
                        &format!("UPDATE {table} SET image_id=? WHERE image_id=?"),
                        params![new, old],
                    )?;
                }
            }
            // Sessions see the old id leave and the new one arrive.
            for (old, new) in pairs {
                tx.execute(
                    "INSERT INTO change_log(image_id,kind) VALUES(?,2),(?,1)",
                    params![old.to_string(), new.to_string()],
                )?;
            }
            tx.commit()?;
            Ok(())
        })();
        self.conn.pragma_update(None, "foreign_keys", "ON")?;
        result
    }
}

impl Index {
    /// `"path"` for a catalog whose images still carry pre-1.8 ids,
    /// `"volume"` once they were migrated (or for a catalog born on 1.8).
    pub fn id_scheme(&self) -> EngineResult<String> {
        Ok(self
            .0
            .meta("id_scheme")
            .map_err(EngineError::from)?
            .unwrap_or_else(|| "volume".into()))
    }
    pub fn set_id_scheme(&self, scheme: &str) -> EngineResult<()> {
        self.0.set_meta("id_scheme", scheme).map_err(Into::into)
    }

    /// Indexed files on attached external volumes whose stored id is the
    /// legacy path id (and differs from the volume id they derive now).
    /// Files whose volume is absent are not listed; the next scan that finds
    /// them re-keys them.
    pub fn legacy_images(&self) -> EngineResult<Vec<LegacyImage>> {
        let mounted = self.0.volumes.mounted()?;
        let mut out = Vec::new();
        let mut stmt = self
            .0
            .conn
            .prepare(
                "SELECT i.id,f.path FROM image i JOIN file f ON f.id=i.file_id ORDER BY f.path",
            )
            .map_err(sql)?;
        let rows = stmt
            .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))
            .map_err(sql)?;
        for row in rows {
            let (stored, path) = row.map_err(sql)?;
            let Some(volume) = mounted
                .iter()
                .filter(|v| Path::new(&path).starts_with(&v.mount_point))
                .max_by_key(|v| v.mount_point.as_os_str().len())
            else {
                continue;
            };
            let old = crate::parse_id(&stored).map_err(EngineError::from)?;
            let new = image_id_on(Some(volume), Path::new(&path));
            if old != new && old == ImageId::for_path(&path) {
                out.push(LegacyImage {
                    old,
                    new,
                    path: PathBuf::from(path),
                });
            }
        }
        Ok(out)
    }

    /// Re-keys `(old, new)` pairs in place (see `Core::rekey_images`).
    pub fn rekey_images(&self, pairs: &[(ImageId, ImageId)]) -> EngineResult<()> {
        self.0.rekey_images(pairs).map_err(Into::into)
    }

    /// The `(old, new)` pairs scans re-keyed since the last call: legacy ids
    /// found on a volume. Hosts re-key their own id references (albums).
    pub fn take_rekeyed(&mut self) -> Vec<(ImageId, ImageId)> {
        std::mem::take(&mut self.0.rekeyed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{FixedVolumes, NoopMetadataProvider, NoopSidecarReader, Query, Score};
    use engine_api::recipe::{Grade, Selection};

    fn drive(uuid: &str, mount: &Path) -> Box<FixedVolumes> {
        Box::new(FixedVolumes(vec![Volume {
            uuid: uuid.into(),
            mount_point: mount.to_path_buf(),
        }]))
    }

    #[test]
    fn legacy_ids_rekey_in_place_keeping_rows_and_the_scan_rekeys_stragglers() {
        let dir = tempfile::tempdir().unwrap();
        let mount = dir.path().canonicalize().unwrap().join("Drive");
        std::fs::create_dir_all(mount.join("shoot")).unwrap();
        for name in ["a.jpg", "b.jpg"] {
            std::fs::write(mount.join("shoot").join(name), name).unwrap();
        }
        let db = dir.path().join("index.sqlite");
        // Indexed as a boot folder: path ids (what a pre-1.8 catalog holds).
        let mut index = Index::open(&db).unwrap();
        index.set_volume_resolver(Box::new(FixedVolumes(vec![])));
        index
            .scan(
                mount.join("shoot"),
                &NoopSidecarReader,
                &NoopMetadataProvider,
            )
            .unwrap();
        let a = mount.join("shoot/a.jpg");
        let ids = [index.image_at(&a).unwrap().unwrap()];
        assert_eq!(ids[0], ImageId::for_path(&a.to_string_lossy()));
        index.set_id_scheme("path").unwrap();
        let selection = Selection::keep(Some(Grade::Two));
        index.set_selection(ids[0], &selection).unwrap();
        index
            .set_score(
                ids[0],
                &Score {
                    signal: "focus".into(),
                    value: 0.7,
                    model: "m".into(),
                },
            )
            .unwrap();
        index
            .replace_faces(
                ids[0],
                &[crate::FaceRecord {
                    id: 0,
                    bbox: [1., 1., 10., 10.],
                    landmarks5: [[1.; 2]; 5],
                    confidence: 1.,
                    embedding: None,
                    sharpness: 0.5,
                    eyes_open: None,
                }],
            )
            .unwrap();
        index.create_person("ada", Some("Ada"), None).unwrap();
        index
            .assign_face(
                crate::FaceKey {
                    image_id: ids[0],
                    ordinal: 0,
                },
                "ada",
            )
            .unwrap();
        let head = index.change_head().unwrap();

        // The folder turns out to be on a drive: migrate the attached files.
        index.set_volume_resolver(drive("DRV-1", &mount));
        let legacy = index.legacy_images().unwrap();
        assert_eq!(legacy.len(), 2);
        assert_eq!(legacy[0].old, ids[0]);
        assert_eq!(
            legacy[0].new,
            ImageId::for_volume_path("DRV-1", "shoot/a.jpg")
        );
        let pairs: Vec<_> = legacy.iter().map(|l| (l.old, l.new)).collect();
        index.rekey_images(&pairs).unwrap();
        index.set_id_scheme("volume").unwrap();
        let new = legacy[0].new;
        assert_eq!(index.selection(new).unwrap(), Some(selection));
        assert_eq!(index.scores(new).unwrap().len(), 3, "focus plus face rows");
        assert_eq!(index.faces(new).unwrap().len(), 1);
        assert_eq!(index.face_assignments(new).unwrap()[0].person_id, "ada");
        assert_eq!(index.image_info(new).unwrap().path, a);
        assert!(index.image_info(ids[0]).is_err());
        let feed = index.changes_since(head).unwrap();
        assert!(!feed.reset);
        assert!(
            feed.changes
                .iter()
                .any(|c| c.id == ids[0] && c.kind == crate::ChangeKind::Removed)
        );
        assert!(
            feed.changes
                .iter()
                .any(|c| c.id == new && c.kind == crate::ChangeKind::Added)
        );
        assert!(index.legacy_images().unwrap().is_empty());
        assert_eq!(
            index
                .search(&Query {
                    text: Some("a".into()),
                    ..Default::default()
                })
                .unwrap(),
            vec![new],
            "FTS follows the new id"
        );
        // A rescan at the same mount point changes nothing.
        assert_eq!(
            index
                .scan(
                    mount.join("shoot"),
                    &NoopSidecarReader,
                    &NoopMetadataProvider
                )
                .unwrap(),
            0
        );
        assert!(index.take_rekeyed().is_empty());

        // A file the migration could not see (its drive was absent) is re-keyed
        // by the scan that finds it, and reported for the host's albums.
        let c = mount.join("shoot/c.jpg");
        std::fs::write(&c, b"c").unwrap();
        index.set_volume_resolver(Box::new(FixedVolumes(vec![])));
        index
            .scan(
                mount.join("shoot"),
                &NoopSidecarReader,
                &NoopMetadataProvider,
            )
            .unwrap();
        let legacy_c = ImageId::for_path(&c.to_string_lossy());
        assert!(index.image_info(legacy_c).is_ok());
        index.set_volume_resolver(drive("DRV-1", &mount));
        index
            .scan(
                mount.join("shoot"),
                &NoopSidecarReader,
                &NoopMetadataProvider,
            )
            .unwrap();
        let volume_c = ImageId::for_volume_path("DRV-1", "shoot/c.jpg");
        assert_eq!(index.take_rekeyed(), vec![(legacy_c, volume_c)]);
        assert!(index.image_info(legacy_c).is_err());
        assert_eq!(index.image_info(volume_c).unwrap().path, c);
        assert_eq!(index.search(&Query::default()).unwrap().len(), 3);
    }
}
