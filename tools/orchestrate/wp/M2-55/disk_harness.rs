// Isolated disk tests, useful while the full FFI dependency graph compiles.
use std::{
    fs,
    path::{Path, PathBuf},
    sync::Mutex,
};
type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;
type Bytes = Vec<u8>;
#[path = "../../../../crates/previews/src/disk.rs"]
mod disk;
