//! Read-only lookup of unbaked Lightroom Smart Preview DNGs.
//!
//! The key is **AgLibraryFile.id_global**, never Adobe_images.id_global.
//! No database, directory walk, copying, or writes are needed for lookup.
use std::path::{Path, PathBuf};

#[derive(Clone, Debug)]
pub struct SmartPreviewIndex {
    root: PathBuf,
}

impl SmartPreviewIndex {
    pub fn new(catalog: impl AsRef<Path>) -> Self {
        let catalog = catalog.as_ref();
        let mut name = catalog.file_stem().unwrap_or_default().to_os_string();
        name.push(" Smart Previews.lrdata");
        Self {
            root: catalog.with_file_name(name),
        }
    }

    /// Read-only explicit bundle root when the catalog itself is a scratch copy.
    pub fn from_bundle(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Pure derivation. Validate ASCII UUID syntax before byte slicing or
    /// joining any catalog-derived component; preserve Lightroom's letter case.
    pub fn expected_path(&self, file_uuid: &str) -> Option<PathBuf> {
        let valid = match file_uuid.len() {
            32 => file_uuid.bytes().all(|b| b.is_ascii_hexdigit()),
            36 => file_uuid.bytes().enumerate().all(|(i, b)| {
                if matches!(i, 8 | 13 | 18 | 23) {
                    b == b'-'
                } else {
                    b.is_ascii_hexdigit()
                }
            }),
            _ => false,
        };
        valid.then(|| {
            self.root
                .join(&file_uuid[..1])
                .join(&file_uuid[..4])
                .join(format!("{file_uuid}.dng"))
        })
    }

    /// The only filesystem operation is checking the derived path is a file.
    pub fn find(&self, file_uuid: &str) -> Option<PathBuf> {
        self.expected_path(file_uuid).filter(|p| p.is_file())
    }
}
