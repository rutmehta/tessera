#![allow(non_camel_case_types, non_snake_case, non_upper_case_globals)]

use std::path::Path;
use thiserror::Error;
mod sensor;
#[cfg(test)]
mod sensor_tests;
pub use sensor::CfaLayout;

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
    unpacked: bool,
}
// This handle owns its allocations and stream exclusively; no references escape.
unsafe impl Send for RawFile {}

#[derive(Debug, Clone)]
pub struct CfaImage {
    pub width: u32,
    pub height: u32,
    pub data: Vec<u16>,
    pub cfa_layout: CfaLayout,
    pub black: [f32; 4],
    pub white: u32,
    pub wb_coeffs: [f32; 4],
    pub color_matrix: [[f32; 3]; 3],
    /// Original LibRaw color.cam_xyz: XYZ -> camera, D65; fourth sensor row retained.
    pub cam_xyz: [[f32; 3]; 4],
    /// Original LibRaw color.rgb_cam: white-balanced camera -> linear sRGB.
    pub rgb_cam: [[f32; 4]; 3],
    pub crop: [u32; 4],
}
#[derive(Debug, Clone)]
pub struct Metadata {
    pub make: String,
    pub model: String,
    pub lens: Option<String>,
    pub iso: f32,
    pub shutter: f32,
    pub aperture: f32,
    pub focal: f32,
    pub timestamp: i64,
    pub orientation: u16,
    pub has_opcode_list: bool,
    pub has_gain_map: bool,
}

impl RawFile {
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        use std::os::unix::ffi::OsStrExt;
        let path = std::ffi::CString::new(path.as_ref().as_os_str().as_bytes())
            .map_err(|_| RawError::LibRaw(-2))?;
        let raw = unsafe { bindings::libraw_init(0) };
        if raw.is_null() {
            return Err(RawError::LibRaw(-1));
        }
        let code = unsafe { bindings::libraw_open_file(raw, path.as_ptr()) };
        if code != 0 {
            unsafe { bindings::libraw_close(raw) };
            return Err(RawError::LibRaw(code));
        }
        Ok(Self {
            raw,
            unpacked: false,
        })
    }
    pub fn unpack(&mut self) -> Result<()> {
        if self.unpacked {
            return Ok(());
        }
        let code = unsafe { bindings::libraw_unpack(self.raw) };
        if code == 0 {
            self.unpacked = true;
            Ok(())
        } else {
            Err(RawError::LibRaw(code))
        }
    }
    pub fn cfa_data(&self) -> CfaImage {
        let mut image = self.sensor_info();
        if !self.unpacked {
            return image;
        }
        unsafe {
            let data = &*self.raw;
            let sizes = &data.sizes;
            let ptr = data.rawdata.raw_image;
            let stride = sizes.raw_pitch as usize / std::mem::size_of::<u16>();
            if !ptr.is_null() && stride >= image.width as usize {
                image
                    .data
                    .reserve(image.width as usize * image.height as usize);
                for y in 0..image.height as usize {
                    image.data.extend_from_slice(std::slice::from_raw_parts(
                        ptr.add(y * stride),
                        image.width as usize,
                    ));
                }
            }
        }
        image
    }
    pub fn embedded_preview(&mut self) -> Option<Vec<u8>> {
        unsafe {
            let list = &(*self.raw).thumbs_list;
            let mut candidates: Vec<_> = list.thumblist.iter()
                .take(list.thumbcount.max(0) as usize)
                .enumerate()
                .filter(|(_, t)| t.tformat == bindings::LibRaw_internal_thumbnail_formats_LIBRAW_INTERNAL_THUMBNAIL_JPEG)
                .map(|(i, t)| (u64::from(t.twidth) * u64::from(t.theight), t.tlength, i))
                .collect();
            candidates.sort_unstable_by(|a, b| b.cmp(a));
            for (_, _, i) in candidates {
                if bindings::libraw_unpack_thumb_ex(self.raw, i as i32) != 0 {
                    continue;
                }
                let thumb = &(*self.raw).thumbnail;
                if thumb.tformat != bindings::LibRaw_thumbnail_formats_LIBRAW_THUMBNAIL_JPEG
                    || thumb.tlength == 0
                    || thumb.thumb.is_null()
                {
                    continue;
                }
                let bytes =
                    std::slice::from_raw_parts(thumb.thumb.cast::<u8>(), thumb.tlength as usize);
                if bytes.starts_with(&[0xff, 0xd8]) && bytes.ends_with(&[0xff, 0xd9]) {
                    return Some(bytes.to_vec());
                }
            }
            None
        }
    }
    pub fn metadata(&self) -> Metadata {
        unsafe {
            let i = &(*self.raw).other;
            let c = &(*self.raw).idata;
            let levels = &(*self.raw).color.dng_levels;
            let mut lens = sensor::c_string(&(*self.raw).lens.Lens);
            if lens.is_empty() {
                lens = sensor::c_string(&(*self.raw).lens.makernotes.Lens);
            }
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
                lens: if lens.is_empty() { None } else { Some(lens) },
                iso: i.iso_speed,
                shutter: i.shutter,
                aperture: i.aperture,
                focal: i.focal_len,
                timestamp: i.timestamp,
                orientation: sensor::exif_orientation((*self.raw).sizes.flip),
                has_opcode_list: levels.parsedfields & ((1 << 7) | (1 << 16) | (1 << 17)) != 0,
                has_gain_map: levels.rawopcodes.iter().any(|op| {
                    !op.data.is_null()
                        && op.len > 0
                        && sensor::opcode_has_gain_map(std::slice::from_raw_parts(
                            op.data.cast::<u8>(),
                            op.len as usize,
                        ))
                }),
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
        let root = std::env::var_os("RAW_DECODE_FIXTURES")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/raw"));
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
