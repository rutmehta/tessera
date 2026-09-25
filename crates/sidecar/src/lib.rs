//! XMP metadata sidecar synchronization.
mod develop;
pub use develop::ImportedRecipe;
mod xml;
mod xmp;
use engine_api::error::{EngineError, EngineResult};
use std::collections::BTreeMap;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
pub use xmp::Metadata;

use engine_api::recipe::{
    Recipe, Selection,
    crs::CrsKey,
    selection::{Decision, Grade, Mark},
};

use serde::{Deserialize, Serialize};

static TEMP_ID: AtomicU64 = AtomicU64::new(0);

fn atomic_write(path: &Path, bytes: &[u8]) -> EngineResult<()> {
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent).map_err(|e| EngineError::io_at(parent, &e))?;
    let (temp, mut file) = loop {
        let id = TEMP_ID.fetch_add(1, Ordering::Relaxed);
        let temp = parent.join(format!(".sidecar-{}-{id}.tmp", std::process::id()));
        match fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp)
        {
            Ok(file) => break (temp, file),
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(EngineError::io_at(&temp, &e)),
        }
    };
    let result = (|| -> io::Result<()> {
        file.write_all(bytes)?;
        file.sync_all()?;
        drop(file);
        fs::rename(&temp, path)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temp);
    }
    result.map_err(|e| EngineError::io_at(path, &e))
}

/// Paths associated with one image.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SidecarPaths {
    pub xmp: PathBuf,
    pub recipe: PathBuf,
}

/// Recipe envelope includes synchronization metadata without changing engine-api's contract.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct RecipeDocument {
    pub recipe: Recipe,
    #[serde(default)]
    pub vector_clock: BTreeMap<String, u64>,
    #[serde(default)]
    pub last_writer: WriteStamp,
}

/// Stable last-writer order, independent of the merged vector clock.
#[derive(Debug, Clone, Default, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct WriteStamp {
    pub timestamp_ms: i64,
    pub machine_id: String,
    pub counter: u64,
}
impl RecipeDocument {
    /// Advance synchronization metadata once for a logical edit, not for serialization.
    /// The logical timestamp also advances when the wall clock moves backwards.
    pub fn record_write(&mut self, machine_id: &str, timestamp_ms: i64) -> EngineResult<()> {
        if machine_id.is_empty() {
            return Err(EngineError::invalid("machine_id", "empty"));
        }
        let counter = self
            .vector_clock
            .get(machine_id)
            .copied()
            .unwrap_or(0)
            .checked_add(1)
            .ok_or_else(|| EngineError::invalid("vector_clock", "counter overflow"))?;
        let timestamp_ms = timestamp_ms.max(
            self.last_writer
                .timestamp_ms
                .checked_add(1)
                .ok_or_else(|| EngineError::invalid("timestamp", "overflow"))?,
        );
        self.vector_clock.insert(machine_id.into(), counter);
        self.last_writer = WriteStamp {
            timestamp_ms,
            machine_id: machine_id.into(),
            counter,
        };
        Ok(())
    }
}

/// Sidecar helpers.
pub struct Sidecar;

impl Sidecar {
    /// Derive XMP and recipe paths for an image.
    pub fn paths(image_path: impl AsRef<Path>) -> SidecarPaths {
        let image = image_path.as_ref();
        let mut xmp = image.as_os_str().to_os_string();
        xmp.push(".xmp");
        let recipe = image
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .join(".edits")
            .join(format!(
                "{}.json",
                image
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or("image")
            ));
        SidecarPaths {
            xmp: PathBuf::from(xmp),
            recipe,
        }
    }

    /// Read a recipe envelope.
    pub fn read_recipe(path: impl AsRef<Path>) -> EngineResult<RecipeDocument> {
        let path = path.as_ref();
        let mut document: RecipeDocument =
            serde_json::from_slice(&fs::read(path).map_err(|e| EngineError::io_at(path, &e))?)?;
        document.recipe.selection = document.recipe.selection.normalized();
        document.recipe.validate()?;
        Ok(document)
    }

    /// Atomically write a recipe envelope.
    pub fn write_recipe(path: impl AsRef<Path>, document: &RecipeDocument) -> EngineResult<()> {
        document.recipe.to_json()?;
        document.recipe.validate()?;
        atomic_write(path.as_ref(), &serde_json::to_vec_pretty(document)?)
    }

    pub fn read_xmp(path: impl AsRef<Path>) -> EngineResult<XmpPacket> {
        let path = path.as_ref();
        XmpPacket::parse(fs::read_to_string(path).map_err(|e| EngineError::io_at(path, &e))?)
    }

    pub fn write_xmp(path: impl AsRef<Path>, packet: &XmpPacket) -> EngineResult<()> {
        xml::Tree::parse(&packet.xml)?;
        atomic_write(path.as_ref(), packet.xml.as_bytes())
    }

    /// LWW by (logical timestamp, machine id, counter), with deterministic content tie-break.
    /// Clocks merge componentwise; the winner stamp never changes just because clocks merge.
    pub fn merge_last_writer_wins(
        left: &RecipeDocument,
        right: &RecipeDocument,
    ) -> EngineResult<RecipeDocument> {
        if left.recipe.image_id != right.recipe.image_id {
            return Err(EngineError::invalid(
                "image_id",
                "cannot merge different images",
            ));
        }
        left.recipe.validate()?;
        right.recipe.validate()?;
        let mut clock = left.vector_clock.clone();
        for (machine, value) in &right.vector_clock {
            clock
                .entry(machine.clone())
                .and_modify(|v| *v = (*v).max(*value))
                .or_insert(*value);
        }
        let order = left.last_writer.cmp(&right.last_writer);
        let choose_left = if order.is_eq() {
            left.recipe.to_json()? >= right.recipe.to_json()?
        } else {
            order.is_gt()
        };
        let mut result = if choose_left {
            left.clone()
        } else {
            right.clone()
        };
        result.vector_clock = clock;
        Ok(result)
    }

    /// Convert selection state to interoperability rating and flag.
    pub fn selection_xmp(
        selection: &Selection,
        label: Option<&MarkPreset>,
    ) -> (i32, Option<&'static str>, Option<String>) {
        let s = selection.clone().normalized();
        let (rating, flag) = match (s.decision, s.grade) {
            (Decision::Reject, _) => (-1, Some("reject")),
            (Decision::Undecided, _) => (0, None),
            (Decision::Keep, None) => (1, Some("pick")),
            (Decision::Keep, Some(Grade::One)) => (2, Some("pick")),
            (Decision::Keep, Some(Grade::Two)) => (3, Some("pick")),
            (Decision::Keep, Some(Grade::Three)) => (5, Some("pick")),
        };
        let text = s
            .mark
            .map(|m| label.map_or(m.0.clone(), |p| p.label_for(&m.0)));
        (rating, flag, text)
    }

    /// Parse selection properties. `flag` is the Dynamic Media pick value.
    pub fn selection_from_xmp(
        rating: Option<i32>,
        flag: Option<&str>,
        label: Option<&str>,
    ) -> Selection {
        let rating = rating.unwrap_or(0);
        let decision = if rating < 0 || matches!(flag, Some("reject" | "-1")) {
            Decision::Reject
        } else if matches!(flag, Some("pick" | "1")) || (1..=5).contains(&rating) {
            Decision::Keep
        } else {
            Decision::Undecided
        };
        let grade = if decision != Decision::Keep {
            None
        } else {
            match rating {
                2 => Some(Grade::One),
                3..=4 => Some(Grade::Two),
                5 => Some(Grade::Three),
                _ => None,
            }
        };
        Selection {
            decision,
            grade,
            mark: label
                .filter(|s| !s.is_empty())
                .map(|s| Mark::new(s.to_owned())),
        }
    }
}

/// Mapping of mark names to Lightroom's default color labels.
#[derive(Debug, Clone, Default)]
pub struct MarkPreset {
    pub labels: BTreeMap<String, String>,
}
impl MarkPreset {
    /// Lightroom default label text for a named color.
    pub fn lightroom() -> Self {
        Self {
            labels: ["Red", "Yellow", "Green", "Blue", "Purple"]
                .into_iter()
                .map(|s| (s.to_lowercase(), s.to_owned()))
                .collect(),
        }
    }
    fn label_for(&self, name: &str) -> String {
        self.labels
            .get(name)
            .cloned()
            .unwrap_or_else(|| name.to_owned())
    }
}

/// XMP contents retained as an XML packet, including unrecognized properties.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct XmpPacket {
    pub xml: String,
}
impl XmpPacket {
    /// Parse an XMP packet with quick-xml, rejecting malformed XML.
    pub fn parse(xml: impl Into<String>) -> EngineResult<Self> {
        let xml = xml.into();
        xml::Tree::parse(&xml)?;
        Ok(Self { xml })
    }
    /// Return original packet bytes unchanged; unknown namespace properties are preserved verbatim.
    pub fn serialize(&self) -> &str {
        &self.xml
    }

    /// Convert XMP rating, flag and label to selection state.
    pub fn selection(&self) -> EngineResult<Selection> {
        self.read_selection()
    }

    /// Extract recognized Camera Raw element values according to the engine-api table.
    pub fn crs_values(&self) -> EngineResult<BTreeMap<CrsKey, String>> {
        self.read_crs()
    }

    /// Create a basic standard XMP packet for selection and IPTC metadata.
    pub fn from_selection(selection: &Selection, preset: &MarkPreset) -> Self {
        Self {
            xml: xml::packet(&xmp::metadata_body(selection, &Metadata::default(), preset)),
        }
    }
}
