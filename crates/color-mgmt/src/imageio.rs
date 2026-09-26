use crate::{Error, Result};
use std::{ffi::c_void, ptr};

type Ref = *const c_void;

#[link(name = "CoreFoundation", kind = "framework")]
extern "C" {
    fn CFDataCreate(allocator: Ref, bytes: *const u8, length: isize) -> Ref;
    fn CFDataCreateMutable(allocator: Ref, capacity: isize) -> Ref;
    fn CFDataGetLength(data: Ref) -> isize;
    fn CFDataGetBytePtr(data: Ref) -> *const u8;
    fn CFStringCreateWithCString(
        allocator: Ref,
        text: *const std::ffi::c_char,
        encoding: u32,
    ) -> Ref;
    fn CFRelease(value: Ref);
}

#[link(name = "ImageIO", kind = "framework")]
extern "C" {
    fn CGImageSourceCreateWithData(data: Ref, options: Ref) -> Ref;
    fn CGImageSourceGetCount(source: Ref) -> usize;
    fn CGImageDestinationCreateWithData(data: Ref, kind: Ref, count: usize, options: Ref) -> Ref;
    fn CGImageDestinationAddImageFromSource(
        destination: Ref,
        source: Ref,
        index: usize,
        properties: Ref,
    );
    fn CGImageDestinationFinalize(destination: Ref) -> bool;
}

// Create-rule handles are checked for null and released exactly once.
struct Owned(Ref);
impl Owned {
    fn new(value: Ref) -> Result<Self> {
        if value.is_null() {
            Err(Error::Unsupported("ImageIO could not create image"))
        } else {
            Ok(Self(value))
        }
    }
}
impl Drop for Owned {
    fn drop(&mut self) {
        // SAFETY: this non-null handle was returned with +1 ownership.
        unsafe { CFRelease(self.0) }
    }
}

/// Decode the primary ImageIO image into a lossless TIFF container, preserving
/// its profile and orientation for the shared RGB decoder.
pub fn decode_to_tiff(bytes: &[u8]) -> Result<Vec<u8>> {
    let length =
        isize::try_from(bytes.len()).map_err(|_| Error::Unsupported("ImageIO input too large"))?;
    // SAFETY: input is copied into CFData. All arguments are live retained
    // objects of the documented types, null optional dictionaries, or valid
    // indices. Output is copied only after Finalize and while data is retained.
    unsafe {
        let input = Owned::new(CFDataCreate(ptr::null(), bytes.as_ptr(), length))?;
        let source = Owned::new(CGImageSourceCreateWithData(input.0, ptr::null()))?;
        if CGImageSourceGetCount(source.0) == 0 {
            return Err(Error::Unsupported("ImageIO source is empty"));
        }
        let data = Owned::new(CFDataCreateMutable(ptr::null(), 0))?;
        let kind = Owned::new(CFStringCreateWithCString(
            ptr::null(),
            c"public.tiff".as_ptr(),
            0x08000100,
        ))?;
        let destination = Owned::new(CGImageDestinationCreateWithData(
            data.0,
            kind.0,
            1,
            ptr::null(),
        ))?;
        CGImageDestinationAddImageFromSource(destination.0, source.0, 0, ptr::null());
        if !CGImageDestinationFinalize(destination.0) {
            return Err(Error::Unsupported("ImageIO decode failed"));
        }
        let n = CFDataGetLength(data.0);
        let pointer = CFDataGetBytePtr(data.0);
        if n <= 0 || pointer.is_null() {
            return Err(Error::Unsupported("ImageIO output is empty"));
        }
        Ok(std::slice::from_raw_parts(pointer, n as usize).to_vec())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn heic_decodes_with_imageio() {
        let bytes = include_bytes!("../../image-core/tests/fixtures/rgb.heic");
        let output = decode_to_tiff(bytes).unwrap();
        assert!(output.starts_with(b"II") || output.starts_with(b"MM"));
    }
    #[test]
    fn invalid_data_is_rejected() {
        assert!(decode_to_tiff(b"invalid").is_err());
    }
}
