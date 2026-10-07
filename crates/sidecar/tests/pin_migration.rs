//! SP-INT4 (REV3-SP NB2, NB3): re-keying protected recipes never deletes a
//! recipe unless the destination holds identical bytes, and scales linearly.
use sidecar::{PinOutcome, RecipeDocument, Sidecar};
use std::path::{Path, PathBuf};

struct Scratch {
    root: PathBuf,
    support: PathBuf,
}
impl Scratch {
    fn new(name: &str) -> Self {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests")
            .join(format!(".scratch-pin-{name}"));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let support = root.join("support");
        Self { root, support }
    }
    /// A Lightroom-owned Smart Preview `<uuid>.dng` in `<catalog> Smart Previews.lrdata`.
    fn proxy(&self, catalog: &str, uuid: &str, bytes: &[u8]) -> PathBuf {
        let dir = self
            .root
            .join(format!("{catalog} Smart Previews.lrdata"))
            .join(&uuid[..1])
            .join(&uuid[..4]);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(format!("{uuid}.dng"));
        std::fs::write(&path, bytes).unwrap();
        Sidecar::register_store(&dir.canonicalize().unwrap(), &self.support);
        path
    }
}
impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn edit(path: &Path, exposure: f32) {
    let mut doc = RecipeDocument::default();
    doc.recipe
        .edit(engine_api::recipe::EditMeta::user("Exposure", 1), |s| {
            s.tone.exposure = exposure
        })
        .unwrap();
    doc.record_write("tessera-app", 1).unwrap();
    Sidecar::write_recipe(Sidecar::paths(path).recipe, &doc).unwrap();
}

fn exposure_at(recipe: &Path) -> f32 {
    Sidecar::read_recipe(recipe)
        .unwrap()
        .recipe
        .settings
        .tone
        .exposure
}

fn exposure(path: &Path) -> f32 {
    exposure_at(&Sidecar::paths(path).recipe)
}

const UUID: &str = "AB12CD34-0000-4000-8000-000000000001";

#[test]
fn two_catalogs_sharing_a_file_id_with_different_edits_keep_both() {
    let s = Scratch::new("two-catalogs");
    let a = s.proxy("A", UUID, b"preview bytes from catalog A");
    let b = s.proxy("B", UUID, b"preview bytes from catalog B (rebuilt)");
    edit(&a, 1.0);
    edit(&b, 2.0);
    let identity = format!("lightroom smart preview file\0{UUID}").into_bytes();
    let mut batch = Sidecar::protected_pin_batch();
    assert_eq!(batch.pin(&a, &identity).unwrap(), PinOutcome::Migrated);
    assert_eq!(
        batch.pin(&b, &identity).unwrap(),
        PinOutcome::Conflict,
        "a different recipe already owns the key: kept separate"
    );
    batch.finish().unwrap();
    assert_eq!(exposure(&a), 1.0);
    assert_eq!(exposure(&b), 2.0, "B keeps reading its own edits");
    assert_ne!(Sidecar::paths(&a).recipe, Sidecar::paths(&b).recipe);
}

#[test]
fn two_catalogs_sharing_identical_edits_share_one_recipe_and_dedupe() {
    let s = Scratch::new("shared");
    let a = s.proxy("A", UUID, b"identical preview bytes");
    let b = s.proxy("B", UUID, b"identical preview bytes");
    edit(&a, 1.5);
    let legacy = Sidecar::paths(&a).recipe;
    assert_eq!(
        legacy,
        Sidecar::paths(&b).recipe,
        "legacy content key shared"
    );
    let identity = format!("lightroom smart preview file\0{UUID}").into_bytes();
    let mut batch = Sidecar::protected_pin_batch();
    assert_eq!(batch.pin(&a, &identity).unwrap(), PinOutcome::Migrated);
    assert_eq!(batch.pin(&b, &identity).unwrap(), PinOutcome::Shared);
    batch.finish().unwrap();
    assert_eq!(Sidecar::paths(&a).recipe, Sidecar::paths(&b).recipe);
    assert_eq!(exposure(&b), 1.5);
    assert!(
        !legacy.exists(),
        "byte-identical legacy object is deduplicated"
    );
}

/// Write `exposure` with an explicit recorded edit time.
fn write_at(recipe: &Path, exposure: f32, time_ms: i64) {
    let mut doc = RecipeDocument::default();
    doc.recipe
        .edit(engine_api::recipe::EditMeta::user("Exposure", 1), |s| {
            s.tone.exposure = exposure
        })
        .unwrap();
    doc.record_write("tessera-app", time_ms).unwrap();
    std::fs::create_dir_all(recipe.parent().unwrap()).unwrap();
    std::fs::write(recipe, serde_json::to_vec(&doc).unwrap()).unwrap();
}

/// Set a file's mtime `seconds` from now (negative: in the past).
fn set_mtime(path: &Path, seconds: i64) {
    let now = std::time::SystemTime::now();
    let time = if seconds < 0 {
        now - std::time::Duration::from_secs(seconds.unsigned_abs())
    } else {
        now + std::time::Duration::from_secs(seconds as u64)
    };
    std::fs::File::options()
        .write(true)
        .open(path)
        .unwrap()
        .set_modified(time)
        .unwrap();
}

fn backups(support: &Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    let mut stack = vec![support.join(".edits/lightroom/objects")];
    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(dir).into_iter().flatten().flatten() {
            let p = entry.path();
            if p.is_dir() {
                stack.push(p);
            } else if p
                .file_name()
                .unwrap()
                .to_string_lossy()
                .contains(".backup-")
            {
                found.push(p);
            }
        }
    }
    found
}

/// REV4-SP S1/N9: an interrupted migration left a diverged copy. The winner
/// is decided by the edit time recorded in each recipe, not by file mtime
/// (falsified here to point the other way); in both directions the loser is
/// moved to a `.backup-` file that no lookup resolves to; the plan preview
/// classifies the same way.
#[test]
fn crash_after_copy_before_key_save_keeps_the_newer_recipe_and_a_backup() {
    for legacy_newer in [true, false] {
        let s = Scratch::new(if legacy_newer {
            "crash-legacy"
        } else {
            "crash-dest"
        });
        let p = s.proxy("A", UUID, b"preview");
        let identity = format!("lightroom smart preview file\0{UUID}").into_bytes();
        let destination = Sidecar::protected_identity_recipe(&p, &identity);
        edit(&p, 1.0);
        let legacy = Sidecar::paths(&p).recipe;
        let (legacy_time, destination_time) = if legacy_newer {
            (2_000_000, 1_000_000)
        } else {
            (1_000_000, 2_000_000)
        };
        write_at(&legacy, 2.0, legacy_time);
        write_at(&destination, 3.0, destination_time);
        // File times say the opposite of the recorded edit times.
        set_mtime(&legacy, if legacy_newer { -3600 } else { 3600 });
        set_mtime(&destination, if legacy_newer { 3600 } else { -3600 });
        let mut preview = Sidecar::protected_pin_preview();
        assert_eq!(preview.pin(&p, &identity).unwrap(), PinOutcome::Recovered);
        let mut batch = Sidecar::protected_pin_batch();
        assert_eq!(batch.pin(&p, &identity).unwrap(), PinOutcome::Recovered);
        batch.finish().unwrap();
        assert_eq!(Sidecar::paths(&p).recipe, destination);
        let (winner, loser) = if legacy_newer { (2.0, 3.0) } else { (3.0, 2.0) };
        assert_eq!(exposure(&p), winner, "the recorded newer edit wins");
        let kept = backups(&s.support);
        assert_eq!(kept.len(), 1, "exactly one labelled backup: {kept:?}");
        assert_eq!(exposure_at(&kept[0]), loser);
        assert!(!legacy.exists(), "no unlabelled loser at the legacy key");
        // A never-pinned source with the legacy bytes resolves to the winner,
        // never to the backup.
        let twin = s.proxy("B", "CD12CD34-0000-4000-8000-000000000002", b"preview");
        assert_eq!(exposure(&twin), winner);
        assert!(
            !Sidecar::paths(&twin)
                .recipe
                .to_string_lossy()
                .contains(".backup-")
        );
    }
}

/// NB3: the reference scan reads each store's alias directories once, and
/// each pin reads a constant number of aliases, so a pass is linear in the
/// number of recipes. Counted operations, not wall-clock: every recipe write
/// is a durable F_FULLFSYNC on macOS (about 7 ms each, three per migrated
/// photo), which dominates the time and is itself linear.
#[test]
fn migration_reads_each_alias_a_bounded_number_of_times() {
    let s = Scratch::new("scale");
    let n = 600;
    let proxies: Vec<PathBuf> = (0..n)
        .map(|i| {
            let uuid = format!("{:08X}-0000-4000-8000-{i:012X}", i);
            let p = s.proxy("Big", &uuid, format!("preview {i}").as_bytes());
            edit(&p, 0.5);
            p
        })
        .collect();
    let aliases = std::fs::read_dir(s.support.join(".edits/lightroom/paths"))
        .unwrap()
        .count()
        + std::fs::read_dir(s.support.join(".edits/lightroom/content"))
            .unwrap()
            .count();
    let before = Sidecar::alias_reads();
    let started = std::time::Instant::now();
    let mut batch = Sidecar::protected_pin_batch();
    for (i, p) in proxies.iter().enumerate() {
        let identity = format!("lightroom smart preview file\0{i}").into_bytes();
        assert_eq!(batch.pin(p, &identity).unwrap(), PinOutcome::Migrated);
    }
    batch.finish().unwrap();
    let reads = Sidecar::alias_reads() - before;
    eprintln!(
        "migrated {n} recipes in {:?}; {reads} alias reads for {aliases} alias files",
        started.elapsed()
    );
    // One scan of every alias file plus at most four alias reads per pin.
    // The quadratic scan read every alias file once per migrated object.
    assert!(
        reads <= (aliases + 4 * n) as u64,
        "{reads} alias reads for {n} pins over {aliases} aliases"
    );
    assert_eq!(exposure(&proxies[n - 1]), 0.5);
    assert!(
        !std::fs::read_dir(s.support.join(".edits/lightroom/objects"))
            .unwrap()
            .flatten()
            .flat_map(|d| std::fs::read_dir(d.path()).unwrap().flatten())
            .any(|f| f.file_name().to_string_lossy().contains("backup")),
        "no backups for clean migrations"
    );
}

/// REV4-SP N8: a plan refresh before the first apply resolves every in-place
/// proxy again; a library larger than the old 8,192-entry hash cache must not
/// re-read every proxy file each time.
#[test]
fn repeated_lookups_over_a_large_library_hash_each_proxy_once() {
    let s = Scratch::new("hash-cache");
    let n = 9_000;
    let proxies: Vec<PathBuf> = (0..n)
        .map(|i| {
            let uuid = format!("{:08X}-0000-4000-8000-{i:012X}", i);
            s.proxy("Big", &uuid, format!("preview {i}").as_bytes())
        })
        .collect();
    let first = Sidecar::content_hashes();
    for p in &proxies {
        Sidecar::paths(p);
    }
    let after_first = Sidecar::content_hashes();
    assert!(
        after_first - first >= n as u64,
        "first pass hashes each proxy"
    );
    for p in &proxies {
        Sidecar::paths(p);
    }
    let again = Sidecar::content_hashes() - after_first;
    assert!(
        again < 100,
        "a second pass re-hashed {again} unchanged proxies"
    );
}
