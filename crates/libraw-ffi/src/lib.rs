#![allow(non_camel_case_types, non_snake_case, non_upper_case_globals)]

use std::path::Path;
use thiserror::Error;

#[allow(dead_code, clippy::upper_case_acronyms)]
mod bindings {
    include!(concat!(env!("OUT_DIR"), "/bindings.rs"));
}

#[derive(Debug, Error)]
pub enum RawError {
    #[error("LibRaw error {0}")]
    LibRaw(i32),
}
pub type Result<T> = std::result::Result<T, RawError>;

pub struct RawFile {
    raw: *mut bindings::libraw_data_t,
}
unsafe impl Send for RawFile {}

#[derive(Debug, Clone)]
pub struct CfaImage {
    pub width: u32,
    pub height: u32,
    pub data: Vec<u16>,
    pub cfa_pattern: [u8; 4],
    pub black: [f32; 4],
    pub white: u32,
    pub wb_coeffs: [f32; 4],
    pub color_matrix: [[f32; 3]; 3],
    pub crop: [u32; 4],
}
#[derive(Debug, Clone)]
pub struct Metadata {
    pub make: String,
    pub model: String,
    pub iso: f32,
    pub shutter: f32,
    pub aperture: f32,
    pub focal: f32,
    pub timestamp: i64,
    pub orientation: u16,
}

impl RawFile {
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let raw = unsafe { bindings::libraw_init(0) };
        if raw.is_null() {
            return Err(RawError::LibRaw(-1));
        }
        let path = std::ffi::CString::new(path.as_ref().to_string_lossy().as_bytes())
            .map_err(|_| RawError::LibRaw(-2))?;
        let code = unsafe { bindings::libraw_open_file(raw, path.as_ptr()) };
        if code != 0 {
            unsafe { bindings::libraw_close(raw) };
            return Err(RawError::LibRaw(code));
        }
        Ok(Self { raw })
    }
    pub fn unpack(&mut self) -> Result<()> {
        let code = unsafe { bindings::libraw_unpack(self.raw) };
        if code == 0 {
            Ok(())
        } else {
            Err(RawError::LibRaw(code))
        }
    }
    pub fn cfa_data(&self) -> CfaImage {
        unsafe {
            let data = &*self.raw;
            let sizes = &data.sizes;
            let count = (sizes.raw_width as usize).saturating_mul(sizes.raw_height as usize);
            let ptr = data.rawdata.raw_image;
            let pixels = if ptr.is_null() {
                Vec::new()
            } else {
                std::slice::from_raw_parts(ptr, count).to_vec()
            };
            CfaImage {
                width: sizes.raw_width as u32,
                height: sizes.raw_height as u32,
                data: pixels,
                cfa_pattern: [0; 4],
                black: [data.color.black as f32; 4],
                white: data.color.maximum,
                wb_coeffs: data.color.cam_mul[..4]
                    .to_vec()
                    .try_into()
                    .unwrap_or([0.; 4]),
                color_matrix: [[0.; 3]; 3],
                crop: [
                    sizes.left_margin as u32,
                    sizes.top_margin as u32,
                    sizes.width as u32,
                    sizes.height as u32,
                ],
            }
        }
    }
    pub fn embedded_preview(&mut self) -> Option<Vec<u8>> {
        unsafe {
            if bindings::libraw_unpack_thumb(self.raw) != 0 {
                return None;
            }
            let thumb = &(*self.raw).thumbnail;
            if thumb.tlength == 0 || thumb.thumb.is_null() {
                None
            } else {
                Some(
                    std::slice::from_raw_parts(thumb.thumb.cast::<u8>(), thumb.tlength as usize)
                        .to_vec(),
                )
            }
        }
    }
    pub fn metadata(&self) -> Metadata {
        unsafe {
            let i = &(*self.raw).other;
            let c = &(*self.raw).idata;
            Metadata {
                make: c
                    .make
                    .iter()
                    .take_while(|x| **x != 0)
                    .map(|x| *x as u8 as char)
                    .collect(),
                model: c
                    .model
                    .iter()
                    .take_while(|x| **x != 0)
                    .map(|x| *x as u8 as char)
                    .collect(),
                iso: i.iso_speed,
                shutter: i.shutter,
                aperture: i.aperture,
                focal: i.focal_len,
                timestamp: i.timestamp,
                orientation: (*self.raw).sizes.flip as u16,
            }
        }
    }
}
impl Drop for RawFile {
    fn drop(&mut self) {
        unsafe { bindings::libraw_close(self.raw) }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn missing_file_is_error() {
        assert!(RawFile::open("/definitely/not/a/raw/file.dng").is_err());
    }
    #[test]
    fn fixture_decode() {
        let root = Path::new("../../fixtures/raw");
        if !root.exists() {
            eprintln!("skipping RAW fixture tests: fixtures/raw is absent");
            return;
        }
        for entry in std::fs::read_dir(root).unwrap().flatten() {
            let path = entry.path();
            if !path.is_file() {
                continue;
            }
            let mut raw = RawFile::open(&path).unwrap();
            raw.unpack().unwrap();
            let image = raw.cfa_data();
            assert!(
                image.width > 1000 && image.height > 1000,
                "{}",
                path.display()
            );
        }
    }
}
