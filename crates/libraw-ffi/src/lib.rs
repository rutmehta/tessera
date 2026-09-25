use std::{ffi::CString, path::Path};
use thiserror::Error;

#[allow(non_camel_case_types, non_snake_case, non_upper_case_globals)]
mod bindings {
    include!(concat!(env!("OUT_DIR"), "/bindings.rs"));
}

#[derive(Debug, Error)]
pub enum Error {
    #[error("path contains an interior NUL byte")]
    InvalidPath,
    #[error("LibRaw error: {0}")]
    LibRaw(i32),
}

pub type Result<T> = std::result::Result<T, Error>;

pub struct RawFile {
    raw: *mut bindings::libraw_data_t,
}

impl RawFile {
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref().to_string_lossy();
        let path = CString::new(path.as_bytes()).map_err(|_| Error::InvalidPath)?;
        // SAFETY: LibRaw owns the returned handle until libraw_close.
        let raw = unsafe { bindings::libraw_init(0) };
        if raw.is_null() { return Err(Error::LibRaw(-1)); }
        // SAFETY: raw is initialized and path is NUL-terminated.
        let status = unsafe { bindings::libraw_open_file(raw, path.as_ptr()) };
        if status != 0 {
            // SAFETY: raw is a valid initialized handle.
            unsafe { bindings::libraw_close(raw) };
            return Err(Error::LibRaw(status));
        }
        Ok(Self { raw })
    }

    pub fn unpack(&mut self) -> Result<()> {
        // SAFETY: raw remains valid until drop.
        let code = unsafe { bindings::libraw_unpack(self.raw) };
        if code == 0 { Ok(()) } else { Err(Error::LibRaw(code)) }
    }
}

impl Drop for RawFile {
    fn drop(&mut self) {
        // SAFETY: this object uniquely owns its LibRaw handle.
        unsafe { bindings::libraw_close(self.raw) };
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn nonexistent_file_returns_error() {
        assert!(RawFile::open("/this/path/does/not/exist.raw").is_err());
    }

    #[test]
    fn raw_fixtures_decode_when_available() {
        let root = Path::new("../../fixtures/raw");
        if !root.is_dir() { eprintln!("skipping RAW fixtures: fixtures/raw is absent"); return; }
        let entries = std::fs::read_dir(root).unwrap();
        for entry in entries.flatten().filter(|e| e.path().is_file()) {
            let mut raw = RawFile::open(entry.path()).unwrap();
            raw.unpack().unwrap();
        }
    }
}
