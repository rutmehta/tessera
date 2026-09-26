//! MCP-local persistent brush presets. IDs never enter engine-api.
use brush::Brush;
use engine_api::{EngineError, EngineResult};
use serde::{Deserialize, Serialize};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

/// An application-local preset identifier, deliberately not an engine-api ID.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct BrushPresetId(pub u64);

/// A named, complete engine brush snapshot.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BrushPreset {
    /// Stable ID in this application directory.
    pub id: BrushPresetId,
    /// Display name, never used as a filename.
    pub name: String,
    /// All brush settings, including sampled tips, texture and dynamics.
    pub brush: Brush,
}

/// A brush decoded from ABR before assigning a persistent ID.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NamedBrush {
    /// Imported name or sample ID.
    pub name: String,
    /// Imported tip, natural diameter and spacing; other settings use defaults.
    pub brush: Brush,
}

/// Read-only ABR listing, including unsupported/damaged-record warnings.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AbrListing {
    /// ABR major version.
    pub version: u16,
    /// ABR minor version.
    pub subversion: u16,
    /// Usable brushes in file order.
    pub brushes: Vec<NamedBrush>,
    /// Features not imported or damaged records skipped.
    pub warnings: Vec<String>,
}

/// Result of persisting all usable brushes in an ABR file in one transaction.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AbrImportResult {
    /// Newly saved presets.
    pub presets: Vec<BrushPreset>,
    /// Import warnings, never silently discarded.
    pub warnings: Vec<String>,
}

#[derive(Serialize, Deserialize)]
struct StoreFile {
    version: u32,
    presets: Vec<BrushPreset>,
}

// Serializes read/modify/write across store handles within the MCP process.
static STORE_WRITE: Mutex<()> = Mutex::new(());

/// Persistent store at `<app-dir>/brush-presets.json`.
#[derive(Debug, Clone)]
pub struct BrushPresetStore {
    path: PathBuf,
}

impl BrushPresetStore {
    /// Opens a store without discarding corrupt or newer-version data.
    pub fn open(app_dir: impl AsRef<Path>) -> EngineResult<Self> {
        std::fs::create_dir_all(app_dir.as_ref())?;
        let store = Self {
            path: app_dir.as_ref().join("brush-presets.json"),
        };
        store.list()?;
        Ok(store)
    }

    /// Reads all full presets in stable ID order (empty for a new store).
    pub fn list(&self) -> EngineResult<Vec<BrushPreset>> {
        let data = match std::fs::read(&self.path) {
            Ok(data) => data,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(e) => return Err(e.into()),
        };
        let mut file: StoreFile = serde_json::from_slice(&data)?;
        if file.version != 1 {
            return Err(EngineError::SchemaVersion {
                document: "brush presets".into(),
                found: file.version,
                supported: 1,
            });
        }
        file.presets.sort_by_key(|p| p.id);
        let mut previous = BrushPresetId(0);
        for p in &file.presets {
            if p.id <= previous {
                return Err(EngineError::invalid("preset.id", "zero or duplicate ID"));
            }
            validate_name(&p.name)?;
            p.brush.validate()?;
            previous = p.id;
        }
        Ok(file.presets)
    }

    /// Resolves an ID into its full saved brush.
    pub fn get(&self, id: BrushPresetId) -> EngineResult<BrushPreset> {
        self.list()?
            .into_iter()
            .find(|p| p.id == id)
            .ok_or_else(|| EngineError::not_found("brush preset", id.0))
    }

    /// Saves a new immutable preset. Existing presets are never overwritten.
    pub fn save(&self, name: impl Into<String>, brush: &Brush) -> EngineResult<BrushPreset> {
        let name = name.into();
        validate_name(&name)?;
        brush.validate()?;
        let mut added = self.append(vec![(name, brush.clone())])?;
        Ok(added.remove(0))
    }

    /// Lists usable ABR brushes without writing anything. Supports the brush
    /// parser's v1/v2 computed/sampled and v6/v10 sampled formats.
    pub fn list_abr(bytes: &[u8]) -> EngineResult<AbrListing> {
        let file = brush::abr::parse(bytes)?;
        let mut warnings = file.warnings;
        if file.descriptor.is_some() {
            warnings.push(
                "ABR descriptors are not interpreted; only tips and available spacing are imported"
                    .into(),
            );
        }
        let mut brushes = Vec::new();
        for (index, imported) in file.brushes.into_iter().enumerate() {
            let (tip, size) = imported.to_tip();
            let brush = Brush {
                tip,
                size,
                spacing: imported.spacing.unwrap_or(0.25),
                ..Brush::default()
            };
            let name = if imported.name.trim().is_empty() {
                format!("ABR brush {}", index + 1)
            } else {
                imported.name
            };
            match validate_name(&name).and_then(|_| brush.validate()) {
                Ok(()) => brushes.push(NamedBrush { name, brush }),
                Err(e) => warnings.push(format!("Skipped ABR brush {}: {e}", index + 1)),
            }
        }
        Ok(AbrListing {
            version: file.version,
            subversion: file.subversion,
            brushes,
            warnings,
        })
    }

    /// Imports a byte buffer atomically; invalid headers leave the store alone.
    pub fn import_abr(&self, bytes: &[u8]) -> EngineResult<AbrImportResult> {
        let listing = Self::list_abr(bytes)?;
        let presets = self.append(
            listing
                .brushes
                .into_iter()
                .map(|b| (b.name, b.brush))
                .collect(),
        )?;
        Ok(AbrImportResult {
            presets,
            warnings: listing.warnings,
        })
    }

    /// Imports an ABR file from disk.
    pub fn import_abr_file(&self, path: impl AsRef<Path>) -> EngineResult<AbrImportResult> {
        self.import_abr(&std::fs::read(path)?)
    }

    fn append(&self, items: Vec<(String, Brush)>) -> EngineResult<Vec<BrushPreset>> {
        let _guard = STORE_WRITE
            .lock()
            .map_err(|_| EngineError::internal("preset store lock poisoned"))?;
        let mut presets = self.list()?;
        let mut next = presets.last().map_or(0, |p| p.id.0);
        let mut added = Vec::new();
        for (name, brush) in items {
            validate_name(&name)?;
            brush.validate()?;
            next = next
                .checked_add(1)
                .ok_or_else(|| EngineError::internal("preset IDs exhausted"))?;
            let preset = BrushPreset {
                id: BrushPresetId(next),
                name,
                brush,
            };
            presets.push(preset.clone());
            added.push(preset);
        }
        if !added.is_empty() {
            self.persist(&StoreFile {
                version: 1,
                presets,
            })?;
        }
        Ok(added)
    }

    fn persist(&self, file: &StoreFile) -> EngineResult<()> {
        let bytes = serde_json::to_vec_pretty(file)?;
        let tmp = self
            .path
            .with_extension(format!("{}.tmp", std::process::id()));
        let mut out = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&tmp)?;
        let result = (|| -> std::io::Result<()> {
            out.write_all(&bytes)?;
            out.sync_all()?;
            std::fs::rename(&tmp, &self.path)
        })();
        if result.is_err() {
            let _ = std::fs::remove_file(&tmp);
        }
        result.map_err(Into::into)
    }
}

fn validate_name(name: &str) -> EngineResult<()> {
    if name.trim().is_empty() || name.len() > 1024 {
        Err(EngineError::invalid(
            "preset.name",
            "must be nonempty and at most 1024 bytes",
        ))
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use brush::{Brush, SampledTip, Tip};

    #[test]
    fn abr_listing_and_import_preserve_tips_without_mutating_on_bad_input() {
        let dir = tempfile::tempdir().unwrap();
        let store = BrushPresetStore::open(dir.path()).unwrap();
        let tips = [
            SampledTip::new("one", 2, 1, vec![0.0, 1.0]).unwrap(),
            SampledTip::new("two", 1, 2, vec![1.0, 0.0]).unwrap(),
        ];
        let bytes = brush::abr::write_v6(&tips, 6, 2, true).unwrap();
        let listing = BrushPresetStore::list_abr(&bytes).unwrap();
        assert_eq!(listing.brushes.len(), 2);
        assert!(store.list().unwrap().is_empty());
        let path = dir.path().join("tips.abr");
        std::fs::write(&path, &bytes).unwrap();
        let result = store.import_abr_file(&path).unwrap();
        assert_eq!(result.presets.len(), 2);
        for (preset, tip) in result.presets.iter().zip(&tips) {
            assert_eq!(preset.name, tip.name);
            assert_eq!(preset.brush.tip, Tip::sampled(tip.clone()));
            assert_eq!(preset.brush.size, 2.0);
        }
        let before = std::fs::read(dir.path().join("brush-presets.json")).unwrap();
        assert!(store.import_abr(b"invalid abr").is_err());
        assert_eq!(
            std::fs::read(dir.path().join("brush-presets.json")).unwrap(),
            before
        );
    }

    #[test]
    fn corrupt_store_and_invalid_settings_never_replace_saved_data() {
        let dir = tempfile::tempdir().unwrap();
        let store = BrushPresetStore::open(dir.path()).unwrap();
        let saved = store.save("safe", &Brush::default()).unwrap();
        let path = dir.path().join("brush-presets.json");
        let before = std::fs::read(&path).unwrap();
        let invalid = Brush {
            tip: Tip::sampled(SampledTip {
                name: "bad".into(),
                width: 4,
                height: 4,
                data: vec![1.0],
            }),
            ..Default::default()
        };
        assert!(store.save("bad", &invalid).is_err());
        assert!(store.save(" ", &Brush::default()).is_err());
        assert_eq!(std::fs::read(&path).unwrap(), before);
        for bytes in [
            b"not json".to_vec(),
            serde_json::to_vec(&serde_json::json!({"version":2,"presets":[]})).unwrap(),
            serde_json::to_vec(&serde_json::json!({"version":1,"presets":[saved.clone(), saved]}))
                .unwrap(),
        ] {
            std::fs::write(&path, &bytes).unwrap();
            assert!(BrushPresetStore::open(dir.path()).is_err());
            assert!(store.save("new", &Brush::default()).is_err());
            assert_eq!(std::fs::read(&path).unwrap(), bytes);
        }
    }

    #[test]
    fn concurrent_handles_allocate_distinct_ids() {
        let dir = tempfile::tempdir().unwrap();
        let store = BrushPresetStore::open(dir.path()).unwrap();
        std::thread::scope(|s| {
            for i in 0..8 {
                let store = store.clone();
                s.spawn(move || store.save(format!("brush {i}"), &Brush::default()).unwrap());
            }
        });
        let list = store.list().unwrap();
        assert_eq!(list.len(), 8);
        assert!(list.windows(2).all(|w| w[0].id < w[1].id));
    }

    #[test]
    fn full_brush_persists_across_store_reopen() {
        let dir = tempfile::tempdir().unwrap();
        let store = BrushPresetStore::open(dir.path()).unwrap();
        let mut brush = Brush {
            tip: Tip::sampled(SampledTip::new("custom", 2, 1, vec![0.25, 1.0]).unwrap()),
            wet_edges: true,
            ..Default::default()
        };
        brush.dynamics.scatter = 0.8;
        let saved = store.save("My brush", &brush).unwrap();
        let reopened = BrushPresetStore::open(dir.path()).unwrap();
        let loaded = reopened.get(saved.id).unwrap();
        assert_eq!(loaded.name, "My brush");
        assert_eq!(
            serde_json::to_value(&loaded.brush).unwrap(),
            serde_json::to_value(brush).unwrap()
        );
        assert_eq!(reopened.list().unwrap().len(), 1);
        assert!(dir.path().join("brush-presets.json").is_file());
        assert!(reopened.get(BrushPresetId(999)).is_err());
        let next = reopened.save("Another", &Brush::default()).unwrap();
        assert_ne!(next.id, saved.id);
    }
}
