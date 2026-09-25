//! Lightroom Classic catalog import over UniFFI (docs/05 §3, docs/06 §2.1).
//!
//! `Engine::open_lrcat` reads the catalog once (through `import-lrcat`, which
//! copies it to temporary storage and opens the copy read-only) and returns an
//! `LrcatImport`. Its calls are the sheet's steps:
//!
//! - `summary()`: counts, unsupported items with reasons, a disk estimate, and
//!   whether the catalog is locked / Lightroom is running.
//! - `plan(options)`: the folder table with root relocation, the selection
//!   mapping preview (Lightroom stars/flags/labels → Decision/Grade/Mark), the
//!   keyword hierarchy, and what will be skipped. Nothing is written.
//! - `fidelity_sample(options, n, thumb_px)`: see `lrcat_fidelity`.
//! - `apply(options, listener)`: writes `.edits/<stem>.json` + XMP beside each
//!   original, merges albums/groups/smart albums/keywords into
//!   `<library folder>/library.json`, and re-indexes. Resumable and cancellable.
//!
//! Safe by construction: the catalog, its WAL/lock files and `Previews.lrdata`
//! are only ever read (from copies); Lightroom's own `<name>.xmp` sidecars are
//! never modified (Tessera writes `<name>.<ext>.xmp`, seeded from Lightroom's);
//! existing Tessera edits are kept unless the caller opts in to overwrite them.
use crate::{Decision, Engine, Result, Selection, catalog, failure, now_ms};
use engine_api::{
    id::ImageId,
    recipe::{Mark, Recipe, Selection as CoreSelection},
};
use import_lrcat::{ImportPlan, Keyword, Library, previews::PreviewIndex};
use serde::{Deserialize, Serialize};
use sidecar::{MarkPreset, RecipeDocument, Sidecar, XmpPacket};
use std::{
    collections::{BTreeMap, BTreeSet, HashMap},
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Instant,
};

/// `last_writer.machine_id` of sidecars written by the importer: lets a re-run
/// (resume) recognise and rewrite its own output, and nothing else.
pub(crate) const MACHINE: &str = "lightroom-import";

#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct LrcatIssue {
    pub category: String,
    pub reason: String,
    pub count: u32,
    /// Up to three affected names.
    pub examples: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct LrcatSummary {
    pub catalog_path: String,
    pub schema_version: String,
    pub roots: Vec<String>,
    pub folders: u32,
    /// Including virtual copies.
    pub images: u32,
    pub virtual_copies: u32,
    /// Images with develop settings.
    pub edited: u32,
    pub keywords: u32,
    pub collections: u32,
    pub collection_sets: u32,
    pub smart_collections: u32,
    pub stacks: u32,
    pub faces: u32,
    /// Images with a cached Lightroom preview (`Previews.lrdata`).
    pub previews: u32,
    pub unsupported: Vec<LrcatIssue>,
    /// Bytes the import will write (sidecars, library.json, import bundle).
    pub estimated_bytes: u64,
    /// A Lightroom lock file sits beside the catalog.
    pub catalog_locked: bool,
    pub lightroom_running: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct LrcatRelocation {
    /// A catalog root folder path, as recorded in the catalog.
    pub from: String,
    /// Where that folder is now.
    pub to: String,
}

#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct LrcatMarkMapping {
    /// Lightroom colour-label text (e.g. "Red", or a custom label).
    pub label: String,
    /// Tessera mark name; empty drops the label. Unmapped labels keep their text.
    pub mark: String,
}

#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct LrcatOptions {
    /// Folder that receives library.json (and the `.tessera-import` bundle).
    pub library_folder: String,
    pub relocations: Vec<LrcatRelocation>,
    pub marks: Vec<LrcatMarkMapping>,
    /// Replace edits made in Tessera since (or before) the import.
    pub overwrite_existing_edits: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct LrcatRootRow {
    pub catalog_path: String,
    pub path: String,
    pub exists: bool,
    pub images: u32,
    pub missing: u32,
}

#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct LrcatFolderRow {
    pub catalog_path: String,
    /// After relocation.
    pub path: String,
    pub exists: bool,
    /// Photos (masters) in this folder.
    pub images: u32,
    pub virtual_copies: u32,
    /// Photos whose original is not on disk.
    pub missing: u32,
    /// Under the library folder (so opening it shows these photos).
    pub inside_library: bool,
}

/// One line of the selection mapping: a Lightroom flag/star combination and
/// the Tessera decision and grade it becomes.
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct LrcatSelectionRow {
    pub lightroom: String,
    pub pick: i64,
    pub stars: i64,
    pub decision: Decision,
    pub grade: Option<u8>,
    pub count: u32,
}

#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct LrcatMarkRow {
    pub label: String,
    /// Mark it becomes (empty: dropped).
    pub mark: String,
    pub count: u32,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, uniffi::Record)]
pub struct LrcatSelectionCounts {
    pub rejects: u32,
    pub keeps: u32,
    pub undecided: u32,
    pub grade1: u32,
    pub grade2: u32,
    pub grade3: u32,
    pub marked: u32,
}

/// Keyword hierarchy in preorder.
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct LrcatKeywordRow {
    pub name: String,
    pub depth: u32,
    pub synonyms: Vec<String>,
    pub images: u32,
    /// Same name as an earlier keyword (Tessera names are unique): merged into it.
    pub merged: bool,
}

#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct LrcatPlanPreview {
    pub roots: Vec<LrcatRootRow>,
    pub folders: Vec<LrcatFolderRow>,
    pub selection_rows: Vec<LrcatSelectionRow>,
    pub selection: LrcatSelectionCounts,
    pub marks: Vec<LrcatMarkRow>,
    pub keywords: Vec<LrcatKeywordRow>,
    /// Photos that will get sidecars.
    pub to_import: u32,
    pub missing: u32,
    pub virtual_copies: u32,
    /// Photos skipped for another reason (see `skipped`).
    pub conflicts: u32,
    pub skipped: Vec<LrcatSkip>,
    /// Photos outside the library folder.
    pub outside_library: u32,
    pub library_path: String,
    pub library_exists: bool,
    pub unsupported: Vec<LrcatIssue>,
    pub estimated_bytes: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct LrcatSkip {
    pub name: String,
    pub path: String,
    pub reason: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum LrcatPhase {
    Preparing,
    WritingEdits,
    Library,
    Indexing,
    Finished,
}

#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct LrcatProgress {
    pub phase: LrcatPhase,
    pub done: u32,
    pub total: u32,
    pub current: String,
}

/// Called on the importing thread (throttled to ~20 Hz, plus phase changes).
#[uniffi::export(with_foreign)]
pub trait LrcatProgressListener: Send + Sync {
    fn on_progress(&self, progress: LrcatProgress);
}

#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct LrcatReport {
    pub catalog_path: String,
    pub cancelled: bool,
    /// Photos whose sidecars were written in this run.
    pub imported: u32,
    /// Photos already imported by an earlier, interrupted run.
    pub resumed: u32,
    /// Virtual copies preserved in the import bundle only.
    pub virtual_copies: u32,
    pub skipped: Vec<LrcatSkip>,
    pub unsupported: Vec<LrcatIssue>,
    pub albums: u32,
    pub album_groups: u32,
    pub smart_albums: u32,
    pub keywords: u32,
    /// Selection of the imported photos.
    pub selection: LrcatSelectionCounts,
    pub library_path: String,
    pub bundle_path: String,
    pub indexed: u32,
    pub seconds: f64,
}

/// A parsed catalog. Create with `Engine::open_lrcat`.
#[derive(uniffi::Object)]
pub struct LrcatImport {
    pub(crate) engine: Arc<Engine>,
    pub(crate) catalog: PathBuf,
    pub(crate) plan: ImportPlan,
    pub(crate) previews: Option<PreviewIndex>,
    pub(crate) cancel: AtomicBool,
    summary: LrcatSummary,
}

/// Counts and diagnostics without an engine (for a quick look at a catalog).
#[uniffi::export]
pub fn inspect_lrcat(path: String) -> Result<LrcatSummary> {
    let plan = import_lrcat::import(&path)?;
    let catalog = Path::new(&path).canonicalize()?;
    let previews = PreviewIndex::open(&catalog).ok().flatten();
    Ok(summarize(&catalog, &plan, previews.as_ref()))
}

#[uniffi::export]
impl Engine {
    /// Reads (a temporary copy of) the catalog. Blocking: call off the main thread.
    pub fn open_lrcat(self: Arc<Self>, path: String) -> Result<Arc<LrcatImport>> {
        let plan = import_lrcat::import(&path)?;
        let catalog = Path::new(&path).canonicalize()?;
        // The preview cache is optional; an unreadable one only disables fidelity.
        let previews = PreviewIndex::open(&catalog).ok().flatten();
        let summary = summarize(&catalog, &plan, previews.as_ref());
        Ok(Arc::new(LrcatImport {
            engine: self,
            catalog,
            plan,
            previews,
            cancel: AtomicBool::new(false),
            summary,
        }))
    }
}

// ---------------------------------------------------------------------------
// Resolution: relocation, marks, per-image outcome.

fn relocations(options: &LrcatOptions) -> Result<Vec<(PathBuf, PathBuf)>> {
    options
        .relocations
        .iter()
        .filter(|r| !r.to.trim().is_empty())
        .map(|r| {
            let to = PathBuf::from(r.to.trim());
            if !to.is_absolute() {
                return Err(failure(format!(
                    "relocation target must be absolute: {}",
                    r.to
                )));
            }
            Ok((PathBuf::from(&r.from), to))
        })
        .collect()
}

pub(crate) fn root_paths(plan: &ImportPlan) -> Vec<PathBuf> {
    plan.roots
        .iter()
        .filter_map(|r| r.get("absolutePath").and_then(|v| v.as_str()))
        .map(PathBuf::from)
        .collect()
}

/// Drops Lightroom's trailing separators ("/Photos/" → "/Photos").
fn display_path(p: &Path) -> String {
    p.components()
        .collect::<PathBuf>()
        .to_string_lossy()
        .into_owned()
}

/// Catalog path → current path (longest matching root wins).
fn relocate(path: &Path, roots: &[PathBuf], moves: &[(PathBuf, PathBuf)]) -> PathBuf {
    let root = roots
        .iter()
        .filter(|r| path.starts_with(r))
        .max_by_key(|r| r.components().count());
    if let Some(root) = root
        && let Some((_, to)) = moves.iter().find(|(from, _)| from == root)
        && let Ok(rest) = path.strip_prefix(root)
    {
        return to.join(rest);
    }
    path.to_path_buf()
}

fn map_mark(label: &str, options: &LrcatOptions) -> Option<String> {
    match options.marks.iter().find(|m| m.label == label) {
        Some(m) if m.mark.trim().is_empty() => None,
        Some(m) => Some(m.mark.trim().to_owned()),
        None => Some(label.to_owned()),
    }
}

fn mapped_selection(image: &import_lrcat::ImportedImage, options: &LrcatOptions) -> CoreSelection {
    let mut s = image.selection.clone();
    s.mark = image
        .color_label
        .as_deref()
        .and_then(|l| map_mark(l, options))
        .map(Mark::new);
    s.normalized()
}

/// App identity for an original: the index keys images by canonical path.
pub(crate) fn app_image_id(path: &Path) -> Option<ImageId> {
    let canonical = path.canonicalize().ok()?;
    let h = blake3::hash(canonical.to_string_lossy().as_bytes());
    Some(ImageId(u128::from_be_bytes(
        h.as_bytes()[..16].try_into().expect("16 bytes"),
    )))
}

const IMAGE_EXTENSIONS: &[&str] = &[
    "jpg", "jpeg", "png", "tif", "tiff", "heic", "dng", "cr2", "cr3", "nef", "nrw", "arw", "raf",
    "orf", "rw2", "pef", "srw", "3fr", "iiq", "x3f", "erf", "mef", "mos",
];

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Outcome {
    Import,
    VirtualCopy,
    Missing,
    Skip(String),
}

#[derive(Clone, Debug)]
pub(crate) struct Resolved {
    pub index: usize,
    pub path: PathBuf,
    pub folder: PathBuf,
    pub outcome: Outcome,
}

pub(crate) fn resolve(plan: &ImportPlan, options: &LrcatOptions) -> Result<Vec<Resolved>> {
    let roots = root_paths(plan);
    let moves = relocations(options)?;
    let mut stems: HashMap<(PathBuf, String), String> = HashMap::new();
    let mut out = Vec::with_capacity(plan.images.len());
    for (index, image) in plan.images.iter().enumerate() {
        let path = relocate(&image.path, &roots, &moves);
        let folder = path.parent().map(Path::to_path_buf).unwrap_or_default();
        let outcome = if image.master_image.is_some() {
            Outcome::VirtualCopy
        } else if !path.is_file() {
            Outcome::Missing
        } else {
            let stem = path
                .file_stem()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_default();
            let name = path
                .file_name()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_default();
            if let Some(first) = stems.get(&(folder.clone(), stem.clone())) {
                Outcome::Skip(format!(
                    "shares its edit sidecar (.edits/{stem}.json) with {first}; Tessera keeps one edit per file name"
                ))
            } else {
                stems.insert((folder.clone(), stem.clone()), name.clone());
                match on_disk_sibling(&path, &stem) {
                    Some(sibling) => Outcome::Skip(format!(
                        "shares its edit sidecar (.edits/{stem}.json) with {sibling} on disk; Tessera keeps one edit per file name"
                    )),
                    None => Outcome::Import,
                }
            }
        };
        out.push(Resolved {
            index,
            path,
            folder,
            outcome,
        });
    }
    Ok(out)
}

/// Another image file in the folder with the same stem (e.g. a RAW+JPEG pair).
fn on_disk_sibling(path: &Path, stem: &str) -> Option<String> {
    let dir = path.parent()?;
    std::fs::read_dir(dir).ok()?.flatten().find_map(|e| {
        let p = e.path();
        let ext = p.extension()?.to_string_lossy().to_lowercase();
        (p != path
            && p.file_stem().is_some_and(|s| s.to_string_lossy() == stem)
            && IMAGE_EXTENSIONS.contains(&ext.as_str())
            && p.is_file())
        .then(|| p.file_name().map(|n| n.to_string_lossy().into_owned()))
        .flatten()
    })
}

/// Existing `.edits/<stem>.json` that the import must not replace.
fn existing_edit_conflict(path: &Path, id: ImageId, overwrite: bool) -> Option<String> {
    let recipe = Sidecar::paths(path).recipe;
    if !recipe.exists() {
        return None;
    }
    match Sidecar::read_recipe(&recipe) {
        Ok(doc) if doc.recipe.image_id != Some(id) => Some(format!(
            "{} belongs to another file; left untouched",
            recipe.display()
        )),
        Ok(doc) if doc.last_writer.machine_id == MACHINE || overwrite => None,
        Ok(_) => Some("already has Tessera edits; kept them (enable overwrite to replace)".into()),
        Err(e) => Some(format!(
            "existing edit sidecar is unreadable ({e}); left untouched"
        )),
    }
}

// ---------------------------------------------------------------------------
// Summary and plan preview.

fn issue(category: &str, reason: String, count: usize, examples: Vec<String>) -> LrcatIssue {
    LrcatIssue {
        category: category.into(),
        reason,
        count: count as u32,
        examples: examples.into_iter().take(3).collect(),
    }
}

fn keyword_names(list: &[Keyword], out: &mut BTreeMap<i64, String>) {
    for k in list {
        out.insert(k.id, k.name.clone());
        keyword_names(&k.children, out);
    }
}

fn duplicate_keywords(plan: &ImportPlan) -> BTreeMap<String, usize> {
    let mut names = BTreeMap::new();
    keyword_names(&plan.library.keywords, &mut names);
    let mut counts: BTreeMap<String, usize> = BTreeMap::new();
    for name in names.values() {
        *counts.entry(name.clone()).or_default() += 1;
    }
    counts.retain(|_, n| *n > 1);
    counts
}

/// Unsupported or partially supported catalog content, grouped by reason.
fn unsupported(plan: &ImportPlan) -> Vec<LrcatIssue> {
    let name_of: HashMap<i64, &str> = plan
        .images
        .iter()
        .map(|i| (i.catalog_id, i.display_name.as_str()))
        .collect();
    let mut groups: BTreeMap<(String, String), Vec<String>> = BTreeMap::new();
    for line in &plan.report {
        let (category, reason, example) = match line
            .strip_prefix("image ")
            .and_then(|rest| rest.split_once(": "))
        {
            Some((id, reason)) => (
                "Develop settings",
                reason.to_owned(),
                id.parse::<i64>()
                    .ok()
                    .and_then(|id| name_of.get(&id).map(|s| s.to_string()))
                    .unwrap_or_else(|| format!("image {id}")),
            ),
            None => match line.strip_prefix("missing table ") {
                // Optional tables absent in older catalogs: one informational line.
                Some(table) => (
                    "Catalog",
                    "optional tables are not in this catalog (older Lightroom version); nothing to import from them".to_owned(),
                    table.to_owned(),
                ),
                None => ("Catalog", line.clone(), String::new()),
            },
        };
        groups
            .entry((category.into(), reason))
            .or_default()
            .push(example);
    }
    let mut out: Vec<LrcatIssue> = groups
        .into_iter()
        .map(|((category, reason), examples)| {
            let n = examples.len();
            let examples = examples.into_iter().filter(|e| !e.is_empty()).collect();
            issue(&category, reason, n, examples)
        })
        .collect();
    let copies: Vec<String> = plan
        .images
        .iter()
        .filter(|i| i.master_image.is_some())
        .map(|i| i.display_name.clone())
        .collect();
    if !copies.is_empty() {
        out.push(issue(
            "Virtual copies",
            "Tessera keeps one edit per file: virtual copies are preserved in the import bundle (import-plan.json), and their album memberships point at the master photo".into(),
            copies.len(),
            copies,
        ));
    }
    if !plan.stacks.is_empty() {
        out.push(issue(
            "Stacks",
            "stacks are preserved in the import bundle; Tessera groups bursts itself and does not show Lightroom stacks".into(),
            plan.stacks.len(),
            vec![],
        ));
    }
    let faces: Vec<String> = plan
        .images
        .iter()
        .filter(|i| !i.faces.is_empty())
        .map(|i| i.display_name.clone())
        .collect();
    if !faces.is_empty() {
        out.push(issue(
            "Faces",
            "face regions are preserved in the import bundle; named people become keywords and library people".into(),
            plan.images.iter().map(|i| i.faces.len()).sum(),
            faces,
        ));
    }
    let history: usize = plan
        .images
        .iter()
        .map(|i| i.history.len() + i.snapshots.len())
        .sum();
    if history > 0 {
        out.push(issue(
            "History",
            "develop history steps and snapshots are kept as source rows in each recipe; they are not replayable Tessera history".into(),
            history,
            vec![],
        ));
    }
    for smart in &plan.library.smart_albums {
        if let Err(e) = plan.library.compile_search(&smart.search) {
            out.push(issue(
                "Smart collections",
                format!("rule is kept but cannot run in Tessera: {e}"),
                1,
                vec![smart.name.clone()],
            ));
        }
    }
    for (name, n) in duplicate_keywords(plan) {
        out.push(issue(
            "Keywords",
            format!("“{name}” appears {n} times in the hierarchy; Tessera keyword names are unique, so they are merged into the first"),
            n,
            vec![name],
        ));
    }
    out
}

fn lightroom_running() -> bool {
    std::process::Command::new("/bin/ps")
        .args(["-Aco", "comm="])
        .output()
        .map(|o| {
            String::from_utf8_lossy(&o.stdout)
                .lines()
                .any(|l| l.trim().starts_with("Adobe Lightroom") || l.trim() == "Lightroom")
        })
        .unwrap_or(false)
}

fn catalog_locked(catalog: &Path) -> bool {
    let name = catalog.as_os_str().to_owned();
    [".lock", "-lock"].iter().any(|suffix| {
        let mut p = name.clone();
        p.push(suffix);
        Path::new(&p).exists()
    })
}

fn estimated_bytes(plan: &ImportPlan) -> u64 {
    let recipes: usize = plan
        .images
        .iter()
        .filter(|i| i.master_image.is_none())
        .map(|i| {
            serde_json::to_vec_pretty(&RecipeDocument {
                recipe: i.recipe.clone(),
                ..Default::default()
            })
            .map_or(0, |v| v.len())
                + 2048 // XMP packet
        })
        .sum();
    let bundle = serde_json::to_vec(plan).map_or(0, |v| v.len());
    let library = serde_json::to_vec_pretty(&plan.library).map_or(0, |v| v.len());
    (recipes + bundle + library) as u64
}

fn summarize(catalog: &Path, plan: &ImportPlan, previews: Option<&PreviewIndex>) -> LrcatSummary {
    let mut names = BTreeMap::new();
    keyword_names(&plan.library.keywords, &mut names);
    LrcatSummary {
        catalog_path: catalog.to_string_lossy().into_owned(),
        schema_version: plan.schema_version.clone(),
        roots: root_paths(plan)
            .iter()
            .map(|p| p.to_string_lossy().into_owned())
            .collect(),
        folders: plan.folders.len() as u32,
        images: plan.images.len() as u32,
        virtual_copies: plan
            .images
            .iter()
            .filter(|i| i.master_image.is_some())
            .count() as u32,
        edited: plan
            .images
            .iter()
            .filter(|i| !i.recipe.history.entries.is_empty())
            .count() as u32,
        keywords: names.len() as u32,
        collections: plan.library.albums.len() as u32,
        collection_sets: plan.library.album_groups.len() as u32,
        smart_collections: plan.library.smart_albums.len() as u32,
        stacks: plan.stacks.len() as u32,
        faces: plan.images.iter().map(|i| i.faces.len()).sum::<usize>() as u32,
        previews: previews.map_or(0, |p| {
            plan.images
                .iter()
                .filter(|i| p.lrprev_path(i.catalog_id).is_some_and(|f| f.is_file()))
                .count() as u32
        }),
        unsupported: unsupported(plan),
        estimated_bytes: estimated_bytes(plan),
        catalog_locked: catalog_locked(catalog),
        lightroom_running: lightroom_running(),
    }
}

fn describe_lightroom(pick: i64, stars: i64) -> String {
    if pick == -1 || stars == -1 {
        return "Rejected".into();
    }
    let flag = if pick == 1 { "Picked" } else { "Unflagged" };
    match stars {
        0 => format!("{flag}, no stars"),
        1 => format!("{flag}, 1 star"),
        n => format!("{flag}, {n} stars"),
    }
}

fn count_selection(counts: &mut LrcatSelectionCounts, s: &CoreSelection) {
    use engine_api::recipe::{Decision as D, Grade as G};
    match s.decision {
        D::Reject => counts.rejects += 1,
        D::Keep => counts.keeps += 1,
        D::Undecided => counts.undecided += 1,
    }
    match s.grade {
        Some(G::One) => counts.grade1 += 1,
        Some(G::Two) => counts.grade2 += 1,
        Some(G::Three) => counts.grade3 += 1,
        None => {}
    }
    if s.mark.is_some() {
        counts.marked += 1;
    }
}

fn keyword_rows(plan: &ImportPlan) -> Vec<LrcatKeywordRow> {
    let mut usage: HashMap<i64, u32> = HashMap::new();
    for i in &plan.images {
        for k in &i.keywords {
            *usage.entry(*k).or_default() += 1;
        }
    }
    fn walk(
        list: &[Keyword],
        depth: u32,
        usage: &HashMap<i64, u32>,
        seen: &mut BTreeSet<String>,
        out: &mut Vec<LrcatKeywordRow>,
    ) {
        for k in list {
            out.push(LrcatKeywordRow {
                name: k.name.clone(),
                depth,
                synonyms: k.synonyms.clone(),
                images: usage.get(&k.id).copied().unwrap_or(0),
                merged: !seen.insert(k.name.clone()),
            });
            walk(&k.children, depth + 1, usage, seen, out);
        }
    }
    let mut out = Vec::new();
    walk(
        &plan.library.keywords,
        0,
        &usage,
        &mut BTreeSet::new(),
        &mut out,
    );
    out
}

fn common_ancestor(paths: &[PathBuf]) -> Option<PathBuf> {
    let mut iter = paths.iter();
    let mut common = iter.next()?.clone();
    for p in iter {
        while !p.starts_with(&common) {
            if !common.pop() {
                return None;
            }
        }
    }
    Some(common)
}

impl LrcatImport {
    fn skip_row(&self, r: &Resolved, reason: String) -> LrcatSkip {
        LrcatSkip {
            name: self.plan.images[r.index].display_name.clone(),
            path: r.path.to_string_lossy().into_owned(),
            reason,
        }
    }

    /// Skips known before writing: missing originals, name collisions and
    /// existing edits (virtual copies are reported separately).
    fn skips(&self, resolved: &[Resolved], options: &LrcatOptions) -> Vec<LrcatSkip> {
        resolved
            .iter()
            .filter_map(|r| match &r.outcome {
                Outcome::Missing => Some(self.skip_row(
                    r,
                    "original not found (relocate its folder if the drive moved)".into(),
                )),
                Outcome::Skip(reason) => Some(self.skip_row(r, reason.clone())),
                Outcome::Import => app_image_id(&r.path)
                    .and_then(|id| {
                        existing_edit_conflict(&r.path, id, options.overwrite_existing_edits)
                    })
                    .map(|reason| self.skip_row(r, reason)),
                Outcome::VirtualCopy => None,
            })
            .collect()
    }
}

#[uniffi::export]
impl LrcatImport {
    pub fn summary(&self) -> LrcatSummary {
        self.summary.clone()
    }

    /// Identity relocations, identity mark names (every label in the catalog)
    /// and the photos' common folder as the library folder.
    pub fn default_options(&self) -> LrcatOptions {
        let roots = root_paths(&self.plan);
        let library = common_ancestor(&roots)
            .filter(|p| p.components().count() > 1)
            .or_else(|| self.catalog.parent().map(Path::to_path_buf))
            .unwrap_or_default();
        let labels: BTreeSet<&str> = self
            .plan
            .images
            .iter()
            .filter_map(|i| i.color_label.as_deref())
            .collect();
        LrcatOptions {
            library_folder: display_path(&library),
            relocations: roots
                .iter()
                .map(|r| LrcatRelocation {
                    from: r.to_string_lossy().into_owned(),
                    to: display_path(r),
                })
                .collect(),
            marks: labels
                .into_iter()
                .map(|l| LrcatMarkMapping {
                    label: l.into(),
                    mark: l.into(),
                })
                .collect(),
            overwrite_existing_edits: false,
        }
    }

    /// What `apply` would do with these options. Reads the disk; writes nothing.
    pub fn plan(&self, options: LrcatOptions) -> Result<LrcatPlanPreview> {
        let resolved = resolve(&self.plan, &options)?;
        let library_folder = PathBuf::from(&options.library_folder);
        let roots = root_paths(&self.plan);
        let moves = relocations(&options)?;
        let root_rows = roots
            .iter()
            .map(|root| {
                let path = relocate(root, &roots, &moves);
                let mine: Vec<&Resolved> = resolved
                    .iter()
                    .filter(|r| self.plan.images[r.index].path.starts_with(root))
                    .filter(|r| r.outcome != Outcome::VirtualCopy)
                    .collect();
                LrcatRootRow {
                    catalog_path: root.to_string_lossy().into_owned(),
                    exists: path.is_dir(),
                    path: display_path(&path),
                    images: mine.len() as u32,
                    missing: mine
                        .iter()
                        .filter(|r| r.outcome == Outcome::Missing)
                        .count() as u32,
                }
            })
            .collect();
        let root_by_id: HashMap<i64, PathBuf> = self
            .plan
            .roots
            .iter()
            .filter_map(|r| {
                Some((
                    r.get("id_local")?.as_i64()?,
                    PathBuf::from(r.get("absolutePath")?.as_str()?),
                ))
            })
            .collect();
        let mut folders: Vec<LrcatFolderRow> = self
            .plan
            .folders
            .iter()
            .filter_map(|f| {
                let root = root_by_id.get(&f.get("rootFolder")?.as_i64()?)?;
                let catalog_path = root.join(f.get("pathFromRoot")?.as_str()?);
                let path = relocate(&catalog_path, &roots, &moves);
                let here: Vec<&Resolved> = resolved.iter().filter(|r| r.folder == path).collect();
                let copies = here
                    .iter()
                    .filter(|r| r.outcome == Outcome::VirtualCopy)
                    .count();
                Some(LrcatFolderRow {
                    catalog_path: catalog_path.to_string_lossy().into_owned(),
                    exists: path.is_dir(),
                    inside_library: path.starts_with(&library_folder),
                    path: display_path(&path),
                    images: (here.len() - copies) as u32,
                    virtual_copies: copies as u32,
                    missing: here
                        .iter()
                        .filter(|r| r.outcome == Outcome::Missing)
                        .count() as u32,
                })
            })
            .collect();
        folders.sort_by(|a, b| a.catalog_path.cmp(&b.catalog_path));

        let mut rows: BTreeMap<(i64, i64), LrcatSelectionRow> = BTreeMap::new();
        let mut counts = LrcatSelectionCounts::default();
        let mut marks: BTreeMap<String, LrcatMarkRow> = BTreeMap::new();
        for image in self.plan.images.iter().filter(|i| i.master_image.is_none()) {
            let s = mapped_selection(image, &options);
            count_selection(&mut counts, &s);
            let pick = image.pick.unwrap_or(0).clamp(-1, 1);
            let stars = image.rating.unwrap_or(0);
            let key = if pick == -1 || stars == -1 {
                (-1, -1)
            } else {
                (pick, stars)
            };
            let ffi: Selection = s.clone().into();
            rows.entry(key)
                .or_insert_with(|| LrcatSelectionRow {
                    lightroom: describe_lightroom(key.0, key.1),
                    pick: key.0,
                    stars: key.1,
                    decision: ffi.decision,
                    grade: ffi.grade,
                    count: 0,
                })
                .count += 1;
            if let Some(label) = &image.color_label {
                marks
                    .entry(label.clone())
                    .or_insert_with(|| LrcatMarkRow {
                        label: label.clone(),
                        mark: map_mark(label, &options).unwrap_or_default(),
                        count: 0,
                    })
                    .count += 1;
            }
        }
        // Rejects first, then by stars descending, picks before unflagged.
        let mut selection_rows: Vec<LrcatSelectionRow> = rows.into_values().collect();
        selection_rows.sort_by_key(|r| (r.pick != -1, -r.stars, -r.pick));

        let skipped = self.skips(&resolved, &options);
        let skipped_paths: BTreeSet<&str> = skipped.iter().map(|s| s.path.as_str()).collect();
        let to_import = resolved
            .iter()
            .filter(|r| {
                r.outcome == Outcome::Import
                    && !skipped_paths.contains(r.path.to_str().unwrap_or(""))
            })
            .count();
        let missing = resolved
            .iter()
            .filter(|r| r.outcome == Outcome::Missing)
            .count();
        let library_path = library_folder.join("library.json");
        Ok(LrcatPlanPreview {
            roots: root_rows,
            folders,
            selection_rows,
            selection: counts,
            marks: marks.into_values().collect(),
            keywords: keyword_rows(&self.plan),
            to_import: to_import as u32,
            missing: missing as u32,
            virtual_copies: resolved
                .iter()
                .filter(|r| r.outcome == Outcome::VirtualCopy)
                .count() as u32,
            conflicts: (skipped.len() - missing) as u32,
            skipped,
            outside_library: resolved
                .iter()
                .filter(|r| {
                    r.outcome != Outcome::VirtualCopy && !r.path.starts_with(&library_folder)
                })
                .count() as u32,
            library_exists: library_path.is_file(),
            library_path: library_path.to_string_lossy().into_owned(),
            unsupported: self.summary.unsupported.clone(),
            estimated_bytes: self.summary.estimated_bytes,
        })
    }

    /// Stops a running `apply` or `fidelity_sample` at the next photo.
    pub fn cancel(&self) {
        self.cancel.store(true, Ordering::SeqCst);
    }

    /// Writes sidecars, merges library.json and re-indexes. Re-running after a
    /// cancel or crash resumes: photos recorded as done with the same options
    /// are skipped, and the library is merged once.
    pub fn apply(
        &self,
        options: LrcatOptions,
        listener: Option<Arc<dyn LrcatProgressListener>>,
    ) -> Result<LrcatReport> {
        self.cancel.store(false, Ordering::SeqCst);
        let started = Instant::now();
        let mut progress = Progress::new(listener);
        progress.phase(LrcatPhase::Preparing, 0, 0, "Reading the library");
        let library_folder = PathBuf::from(&options.library_folder);
        if !library_folder.is_absolute() {
            return Err(failure("library folder must be an absolute path"));
        }
        if library_folder.starts_with(import_lrcat::previews::previews_dir(&self.catalog))
            || library_folder == self.catalog
        {
            return Err(failure(
                "the library folder must not be inside the Lightroom catalog files",
            ));
        }
        std::fs::create_dir_all(&library_folder)?;
        let library_path = library_folder.join("library.json");
        let bundle = bundle_dir(&library_folder, &self.catalog);
        std::fs::create_dir_all(&bundle)?;
        let plan_file = bundle.join("import-plan.json");
        if !plan_file.exists() {
            write_atomic(
                &plan_file,
                &serde_json::to_vec(&self.plan).map_err(failure)?,
            )?;
        }
        let state_file = bundle.join("state.json");
        let fingerprint = options_fingerprint(&options);
        let mut state = State::read(&state_file);
        if state.options != fingerprint {
            // Different mapping or relocation: rewrite (our own) sidecars.
            state.done.clear();
            state.options = fingerprint;
        }
        state.catalog = self.catalog.to_string_lossy().into_owned();

        let resolved = resolve(&self.plan, &options)?;
        let existing = Library::read(&library_path)?;
        let ids: HashMap<ImageId, ImageId> = self.app_ids(&resolved);
        let merge = merge_library(
            existing,
            &self.plan,
            &self.catalog,
            &ids,
            &resolved,
            &options,
        );
        let mut names = BTreeMap::new();
        keyword_names(&self.plan.library.keywords, &mut names);

        let mut report = LrcatReport {
            catalog_path: self.catalog.to_string_lossy().into_owned(),
            cancelled: false,
            imported: 0,
            resumed: 0,
            virtual_copies: resolved
                .iter()
                .filter(|r| r.outcome == Outcome::VirtualCopy)
                .count() as u32,
            skipped: vec![],
            unsupported: self.summary.unsupported.clone(),
            albums: merge.albums,
            album_groups: merge.groups,
            smart_albums: merge.smart_albums,
            keywords: merge.keywords,
            selection: LrcatSelectionCounts::default(),
            library_path: library_path.to_string_lossy().into_owned(),
            bundle_path: bundle.to_string_lossy().into_owned(),
            indexed: 0,
            seconds: 0.0,
        };
        let work: Vec<&Resolved> = resolved
            .iter()
            .filter(|r| r.outcome == Outcome::Import)
            .collect();
        report
            .skipped
            .extend(resolved.iter().filter_map(|r| match &r.outcome {
                Outcome::Missing => Some(self.skip_row(
                    r,
                    "original not found (relocate its folder if the drive moved)".into(),
                )),
                Outcome::Skip(reason) => Some(self.skip_row(r, reason.clone())),
                _ => None,
            }));
        let total = work.len() as u32;
        // Test aid for the acceptance walk: slow the per-photo loop down so the
        // non-modal progress and Cancel can be exercised on the small fixture.
        let delay = std::env::var("TESSERA_LRCAT_IMPORT_DELAY_MS")
            .ok()
            .and_then(|v| v.parse::<u64>().ok())
            .map(std::time::Duration::from_millis);
        for (n, r) in work.iter().enumerate() {
            if self.cancel.load(Ordering::SeqCst) {
                report.cancelled = true;
                break;
            }
            let image = &self.plan.images[r.index];
            progress.tick(
                LrcatPhase::WritingEdits,
                n as u32,
                total,
                &image.display_name,
            );
            let selection = mapped_selection(image, &options);
            if state.done.contains(&image.catalog_id) {
                report.resumed += 1;
                count_selection(&mut report.selection, &selection);
                continue;
            }
            if let Some(delay) = delay {
                std::thread::sleep(delay);
            }
            let Some(id) = app_image_id(&r.path) else {
                report
                    .skipped
                    .push(self.skip_row(r, "original disappeared during import".into()));
                continue;
            };
            if let Some(reason) =
                existing_edit_conflict(&r.path, id, options.overwrite_existing_edits)
            {
                report.skipped.push(self.skip_row(r, reason));
                continue;
            }
            let keywords: Vec<(String, String)> = image
                .keywords
                .iter()
                .filter_map(|k| names.get(k))
                .map(|name| {
                    let path = merge
                        .library
                        .keyword_path(name)
                        .map(|p| p.join("|"))
                        .unwrap_or_else(|| name.clone());
                    (name.clone(), path)
                })
                .collect();
            match write_image(&r.path, id, &image.recipe, &selection, &keywords) {
                Ok(()) => {
                    report.imported += 1;
                    count_selection(&mut report.selection, &selection);
                    state.done.insert(image.catalog_id);
                    if report.imported.is_multiple_of(50) {
                        state.write(&state_file)?;
                    }
                }
                Err(e) => report
                    .skipped
                    .push(self.skip_row(r, format!("could not write sidecars: {}", e))),
            }
        }
        state.write(&state_file)?;
        if report.cancelled {
            // library.json is merged only when every photo is done.
            report.albums = 0;
            report.album_groups = 0;
            report.smart_albums = 0;
            report.keywords = 0;
            report.seconds = started.elapsed().as_secs_f64();
            progress.phase(LrcatPhase::Finished, total, total, "Cancelled");
            return Ok(report);
        }

        progress.phase(LrcatPhase::Library, 0, 1, "Writing library.json");
        if !merge.already_merged || merge.roots_added {
            merge.library.write(&library_path)?;
        }
        if merge.already_merged {
            report.albums = 0;
            report.album_groups = 0;
            report.smart_albums = 0;
            report.keywords = 0;
        }
        state.library_merged = true;
        state.write(&state_file)?;

        // Index the photo folders so the import shows up without a manual rescan.
        let mut scan_roots: Vec<PathBuf> = Vec::new();
        for r in &resolved {
            if r.outcome == Outcome::Import && !scan_roots.iter().any(|s| r.folder.starts_with(s)) {
                scan_roots.retain(|s| !s.starts_with(&r.folder));
                scan_roots.push(r.folder.clone());
            }
        }
        let scan_total = scan_roots.len() as u32;
        for (n, folder) in scan_roots.iter().enumerate() {
            if self.cancel.load(Ordering::SeqCst) {
                break; // Sidecars and library are complete; the app rescans on open.
            }
            progress.phase(
                LrcatPhase::Indexing,
                n as u32,
                scan_total,
                &folder.to_string_lossy(),
            );
            let mut c = self.engine.lock()?;
            report.indexed +=
                c.index
                    .scan(folder, &catalog::Sidecars, &catalog::EmbeddedMetadata)?
                    as u32;
        }
        self.engine
            .lock()?
            .index
            .sync_keyword_tree(&merge.library.keyword_pairs())?;
        report.seconds = started.elapsed().as_secs_f64();
        progress.phase(LrcatPhase::Finished, total, total, "Done");
        Ok(report)
    }
}

impl LrcatImport {
    /// Import identity (catalog-derived) → app identity (path-derived). Virtual
    /// copies map to their master's photo.
    fn app_ids(&self, resolved: &[Resolved]) -> HashMap<ImageId, ImageId> {
        let mut by_catalog: HashMap<i64, ImageId> = HashMap::new();
        for r in resolved.iter().filter(|r| r.outcome == Outcome::Import) {
            if let Some(id) = app_image_id(&r.path) {
                by_catalog.insert(self.plan.images[r.index].catalog_id, id);
            }
        }
        self.plan
            .images
            .iter()
            .filter_map(|i| {
                let target = by_catalog.get(&i.master_image.unwrap_or(i.catalog_id))?;
                Some((i.recipe.image_id?, *target))
            })
            .collect()
    }
}

// ---------------------------------------------------------------------------
// Writing.

fn write_atomic(path: &Path, bytes: &[u8]) -> Result<()> {
    let parent = path.parent().unwrap_or(Path::new("."));
    let mut temp = tempfile::NamedTempFile::new_in(parent)?;
    std::io::Write::write_all(&mut temp, bytes)?;
    temp.as_file().sync_all()?;
    temp.persist(path).map_err(|e| failure(e.error))?;
    Ok(())
}

fn bundle_dir(library_folder: &Path, catalog: &Path) -> PathBuf {
    let stem = catalog
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "catalog".into());
    let hash = blake3::hash(catalog.to_string_lossy().as_bytes()).to_hex();
    library_folder
        .join(".tessera-import")
        .join(format!("{stem}-{}", &hash[..8]))
}

fn options_fingerprint(options: &LrcatOptions) -> String {
    let text = format!(
        "{:?}|{:?}|{}",
        options.relocations, options.marks, options.overwrite_existing_edits
    );
    blake3::hash(text.as_bytes()).to_hex()[..16].to_owned()
}

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(default)]
struct State {
    catalog: String,
    options: String,
    done: BTreeSet<i64>,
    library_merged: bool,
}
impl State {
    fn read(path: &Path) -> Self {
        std::fs::read(path)
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default()
    }
    fn write(&self, path: &Path) -> Result<()> {
        write_atomic(path, &serde_json::to_vec_pretty(self).map_err(failure)?)
    }
}

/// Recipe to `.edits/<stem>.json`, selection + keywords to `<file>.xmp`.
fn write_image(
    path: &Path,
    id: ImageId,
    recipe: &Recipe,
    selection: &CoreSelection,
    keywords: &[(String, String)],
) -> Result<()> {
    let mut recipe = recipe.clone();
    recipe.image_id = Some(id);
    recipe.selection = selection.clone();
    recipe.validate()?;
    let mut doc = RecipeDocument {
        recipe,
        ..Default::default()
    };
    doc.record_write(MACHINE, now_ms())?;
    // Tessera's own XMP; seeded from Lightroom's `<name>.xmp` (never modified).
    let ours = Sidecar::paths(path).xmp;
    let lightroom = path.with_extension("xmp");
    let base = if ours.exists() {
        Some(Sidecar::read_xmp(&ours)?)
    } else if lightroom.exists() {
        Some(Sidecar::read_xmp(&lightroom)?)
    } else {
        None
    };
    let mut meta = match &base {
        Some(p) => p.metadata()?,
        None => Default::default(),
    };
    for (name, hierarchy) in keywords {
        if !meta.keywords.contains(name) {
            meta.keywords.push(name.clone());
        }
        if hierarchy.contains('|') && !meta.hierarchical_keywords.contains(hierarchy) {
            meta.hierarchical_keywords.push(hierarchy.clone());
        }
    }
    let preset = MarkPreset::default();
    let packet = base
        .unwrap_or_else(|| XmpPacket::from_selection(selection, &preset))
        .with_metadata(selection, &meta, &preset)?;
    Sidecar::write_recipe(Sidecar::paths(path).recipe, &doc)?;
    Sidecar::write_xmp(&ours, &packet)?;
    Ok(())
}

pub(crate) struct Merge {
    pub library: Library,
    pub already_merged: bool,
    pub roots_added: bool,
    pub albums: u32,
    pub groups: u32,
    pub smart_albums: u32,
    pub keywords: u32,
}

fn find_keyword_mut<'a>(list: &'a mut [Keyword], name: &str) -> Option<&'a mut Keyword> {
    for k in list {
        if k.name == name {
            return Some(k);
        }
        if let Some(found) = find_keyword_mut(&mut k.children, name) {
            return Some(found);
        }
    }
    None
}

/// Merge the catalog's collections and keywords into an existing library.
/// IDs are offset past the library's, album names that already exist get a
/// "(Lightroom)" suffix, keywords merge by name, and imported albums carry a
/// `lightroom` tag so a second run does not duplicate them.
pub(crate) fn merge_library(
    mut library: Library,
    plan: &ImportPlan,
    catalog: &Path,
    ids: &HashMap<ImageId, ImageId>,
    resolved: &[Resolved],
    options: &LrcatOptions,
) -> Merge {
    let catalog_s = catalog.to_string_lossy().into_owned();
    let mut roots_added = false;
    let roots = root_paths(plan);
    let moves = relocations(options).unwrap_or_default();
    for root in &roots {
        let path = relocate(root, &roots, &moves)
            .components()
            .collect::<PathBuf>();
        if resolved.iter().any(|r| r.path.starts_with(&path)) && !library.roots.contains(&path) {
            library.roots.push(path);
            roots_added = true;
        }
    }
    let already = library.albums.values().any(|a| {
        a.unknown
            .get("lightroom")
            .and_then(|v| v.get("catalog"))
            .and_then(|v| v.as_str())
            == Some(catalog_s.as_str())
    });
    let mut merge = Merge {
        library,
        already_merged: already,
        roots_added,
        albums: 0,
        groups: 0,
        smart_albums: 0,
        keywords: 0,
    };
    if already {
        return merge;
    }
    let lib = &mut merge.library;
    let offset = lib.next_id().unwrap_or(1);
    let map = |id: i64| id + offset;
    for g in &plan.library.album_groups {
        lib.album_groups.push(import_lrcat::AlbumGroup {
            id: map(g.id),
            name: g.name.clone(),
            parent: g.parent.map(map),
        });
        merge.groups += 1;
    }
    for s in &plan.library.smart_albums {
        lib.smart_albums.push(import_lrcat::SmartAlbum {
            id: map(s.id),
            name: s.name.clone(),
            parent: s.parent.map(map),
            search: s.search.clone(),
            scoped: s.scoped,
        });
        merge.smart_albums += 1;
    }
    for album in plan.library.albums.values() {
        let mut key = album.name.clone();
        let mut n = 1;
        while lib.albums.contains_key(&key) {
            key = if n == 1 {
                format!("{} (Lightroom)", album.name)
            } else {
                format!("{} (Lightroom {n})", album.name)
            };
            n += 1;
        }
        let mut images = Vec::new();
        for id in album.images.iter().filter_map(|i| ids.get(i)) {
            if !images.contains(id) {
                images.push(*id);
            }
        }
        let mut unknown = album.unknown.clone();
        unknown.insert(
            "lightroom".into(),
            serde_json::json!({"catalog": catalog_s, "collection": album.id}),
        );
        lib.albums.insert(
            key.clone(),
            import_lrcat::Album {
                id: map(album.id),
                name: key,
                parent: album.parent.map(map),
                images,
                unknown,
            },
        );
        merge.albums += 1;
    }
    fn add(list: &[Keyword], parent: Option<&str>, lib: &mut Library, added: &mut u32) {
        for k in list {
            let name = k.name.trim();
            if lib.keyword_path(name).is_none() && lib.add_keyword(name, parent).is_ok() {
                *added += 1;
            }
            if let Some(existing) = find_keyword_mut(&mut lib.keywords, name) {
                for s in &k.synonyms {
                    if !existing.synonyms.contains(s) {
                        existing.synonyms.push(s.clone());
                    }
                }
            }
            let next = lib.keyword_path(name).is_some().then_some(name);
            add(&k.children, next.or(parent), lib, added);
        }
    }
    add(&plan.library.keywords, None, lib, &mut merge.keywords);
    for person in &plan.library.people {
        if !lib.people.contains(person) {
            lib.people.push(person.clone());
        }
    }
    for m in &options.marks {
        if !m.mark.trim().is_empty() && m.mark.trim() != m.label {
            lib.marks_preset
                .insert(m.mark.trim().to_owned(), m.label.clone());
        }
    }
    merge
}

/// Throttled progress: the first update of each phase, then at most every 50 ms.
struct Progress {
    listener: Option<Arc<dyn LrcatProgressListener>>,
    last: Option<(LrcatPhase, Instant)>,
}
impl Progress {
    fn new(listener: Option<Arc<dyn LrcatProgressListener>>) -> Self {
        Self {
            listener,
            last: None,
        }
    }
    fn phase(&mut self, phase: LrcatPhase, done: u32, total: u32, current: &str) {
        if let Some(l) = &self.listener {
            l.on_progress(LrcatProgress {
                phase,
                done,
                total,
                current: current.into(),
            });
        }
        self.last = Some((phase, Instant::now()));
    }
    fn tick(&mut self, phase: LrcatPhase, done: u32, total: u32, current: &str) {
        let due = self
            .last
            .is_none_or(|(p, t)| p != phase || t.elapsed() >= std::time::Duration::from_millis(50));
        if due {
            self.phase(phase, done, total, current);
        }
    }
}
