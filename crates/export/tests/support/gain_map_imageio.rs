//! Test-only independent ImageIO readback. A non-null image is insufficient:
//! force provider materialization, then draw actual pixels into linear float RGBA.
use std::{ffi::c_void, ptr};
type Ref = *const c_void;
#[repr(C)]
struct Point {
    x: f64,
    y: f64,
}
#[repr(C)]
struct Size {
    width: f64,
    height: f64,
}
#[repr(C)]
struct Rect {
    origin: Point,
    size: Size,
}
#[link(name = "CoreFoundation", kind = "framework")]
unsafe extern "C" {
    fn CFDataCreate(allocator: Ref, bytes: *const u8, length: isize) -> Ref;
    fn CFDataGetLength(data: Ref) -> isize;
    fn CFDictionaryCreate(
        allocator: Ref,
        keys: *const Ref,
        values: *const Ref,
        count: isize,
        key_callbacks: Ref,
        value_callbacks: Ref,
    ) -> Ref;
    fn CFRelease(value: Ref);
    static kCFTypeDictionaryKeyCallBacks: c_void;
    static kCFTypeDictionaryValueCallBacks: c_void;
}
#[link(name = "ImageIO", kind = "framework")]
unsafe extern "C" {
    fn CGImageSourceCreateWithData(data: Ref, options: Ref) -> Ref;
    fn CGImageSourceCreateImageAtIndex(source: Ref, index: usize, options: Ref) -> Ref;
    fn CGImageSourceCopyAuxiliaryDataInfoAtIndex(source: Ref, index: usize, kind: Ref) -> Ref;
    static kCGImageSourceDecodeRequest: Ref;
    static kCGImageSourceDecodeToHDR: Ref;
    static kCGImageSourceDecodeToSDR: Ref;
    static kCGImageAuxiliaryDataTypeISOGainMap: Ref;
}
#[link(name = "CoreGraphics", kind = "framework")]
unsafe extern "C" {
    fn CGImageGetWidth(image: Ref) -> usize;
    fn CGImageGetHeight(image: Ref) -> usize;
    fn CGImageGetDataProvider(image: Ref) -> Ref;
    fn CGDataProviderCopyData(provider: Ref) -> Ref;
    fn CGColorSpaceCreateWithName(name: Ref) -> Ref;
    fn CGBitmapContextCreate(
        data: *mut c_void,
        width: usize,
        height: usize,
        bits: usize,
        stride: usize,
        space: Ref,
        info: u32,
    ) -> Ref;
    fn CGContextDrawImage(context: Ref, rect: Rect, image: Ref);
    static kCGColorSpaceExtendedLinearSRGB: Ref;
}
struct Owned(Ref);
impl Owned {
    fn new(value: Ref) -> Self {
        assert!(!value.is_null(), "native pixel decoder returned null");
        Self(value)
    }
}
impl Drop for Owned {
    fn drop(&mut self) {
        // SAFETY: each Owned wraps a successful Create/Copy result exactly once.
        unsafe {
            CFRelease(self.0);
        }
    }
}
pub fn decode(bytes: &[u8], hdr: bool, expected_iso: bool) -> Vec<[f32; 4]> {
    // SAFETY: input remains live through the CFData copy; every owned CF/CG
    // object is retained until after its last use. The bitmap buffer is sized
    // for checked image dimensions and outlives the drawing context.
    unsafe {
        let data = Owned::new(CFDataCreate(
            ptr::null(),
            bytes.as_ptr(),
            bytes.len().try_into().unwrap(),
        ));
        let source = Owned::new(CGImageSourceCreateWithData(data.0, ptr::null()));
        let aux = CGImageSourceCopyAuxiliaryDataInfoAtIndex(
            source.0,
            0,
            kCGImageAuxiliaryDataTypeISOGainMap,
        );
        assert_eq!(!aux.is_null(), expected_iso);
        if !aux.is_null() {
            CFRelease(aux);
        }
        let key = kCGImageSourceDecodeRequest;
        let value = if hdr {
            kCGImageSourceDecodeToHDR
        } else {
            kCGImageSourceDecodeToSDR
        };
        let options = Owned::new(CFDictionaryCreate(
            ptr::null(),
            &key,
            &value,
            1,
            ptr::addr_of!(kCFTypeDictionaryKeyCallBacks).cast(),
            ptr::addr_of!(kCFTypeDictionaryValueCallBacks).cast(),
        ));
        let image = Owned::new(CGImageSourceCreateImageAtIndex(source.0, 0, options.0));
        let provider = CGImageGetDataProvider(image.0);
        assert!(!provider.is_null());
        let decoded = Owned::new(CGDataProviderCopyData(provider));
        assert!(
            CFDataGetLength(decoded.0) > 0,
            "image metadata alone is not pixel decode"
        );
        let (width, height) = (CGImageGetWidth(image.0), CGImageGetHeight(image.0));
        assert_eq!((width, height), (80, 16));
        let mut pixels = vec![[0f32; 4]; width * height];
        let space = Owned::new(CGColorSpaceCreateWithName(kCGColorSpaceExtendedLinearSRGB));
        // Float components | 32-bit little endian | premultiplied alpha last.
        let context = Owned::new(CGBitmapContextCreate(
            pixels.as_mut_ptr().cast(),
            width,
            height,
            32,
            width * 16,
            space.0,
            0x100 | 0x2000 | 1,
        ));
        CGContextDrawImage(
            context.0,
            Rect {
                origin: Point { x: 0., y: 0. },
                size: Size {
                    width: width as f64,
                    height: height as f64,
                },
            },
            image.0,
        );
        for pixel in &pixels {
            assert!(pixel.iter().all(|v| v.is_finite()));
            assert!(
                pixel[3] > 0.99,
                "failed drawing must not look like valid black pixels"
            );
        }
        pixels
    }
}
