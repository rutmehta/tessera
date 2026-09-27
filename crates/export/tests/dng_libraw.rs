//! Independent processing through the vendored LibRaw C API, not Tessera's
//! linear-DNG reader. Keep these declarations aligned with libraw.h/types.h.
use export::{ExportImage, ExportSettings, Format, Metadata, export_one};
use pipeline_cpu::{Image, RenderSource};
use std::{
    ffi::{CString, c_char, c_int, c_void},
    os::unix::ffi::OsStrExt,
};

#[repr(C)]
struct ProcessedImage {
    kind: c_int,
    height: u16,
    width: u16,
    colors: u16,
    bits: u16,
    size: u32,
    data: [u8; 1],
}
unsafe extern "C" {
    fn libraw_init(flags: u32) -> *mut c_void;
    fn libraw_open_file(raw: *mut c_void, path: *const c_char) -> c_int;
    fn libraw_unpack(raw: *mut c_void) -> c_int;
    fn libraw_set_output_color(raw: *mut c_void, value: c_int);
    fn libraw_set_output_bps(raw: *mut c_void, value: c_int);
    fn libraw_set_gamma(raw: *mut c_void, index: c_int, value: f32);
    fn libraw_set_no_auto_bright(raw: *mut c_void, value: c_int);
    fn libraw_set_adjust_maximum_thr(raw: *mut c_void, value: f32);
    fn libraw_set_user_mul(raw: *mut c_void, index: c_int, value: f32);

    fn libraw_dcraw_process(raw: *mut c_void) -> c_int;
    fn libraw_dcraw_make_mem_image(raw: *mut c_void, error: *mut c_int) -> *mut ProcessedImage;
    fn libraw_dcraw_clear_mem(image: *mut ProcessedImage);
    fn libraw_close(raw: *mut c_void);
}
struct Raw(*mut c_void);
impl Drop for Raw {
    fn drop(&mut self) {
        // SAFETY: this guard uniquely owns the initialized LibRaw handle.
        unsafe { libraw_close(self.0) }
    }
}
struct Bitmap(*mut ProcessedImage);
impl Drop for Bitmap {
    fn drop(&mut self) {
        // SAFETY: this guard uniquely owns the LibRaw-allocated bitmap.
        unsafe { libraw_dcraw_clear_mem(self.0) }
    }
}

fn decode(path: &std::path::Path) -> (u16, u16, Vec<f32>) {
    // Ensure the existing Rust dependency brings in the vendored native library.
    let _ = libraw_ffi::RawFile::open(path).unwrap();
    let path = CString::new(path.as_os_str().as_bytes()).unwrap();
    // SAFETY: only the C API is used, with live owned handles and checked return
    // codes; the flexible array is read only after validating its byte count.
    unsafe {
        let raw = Raw(libraw_init(0));
        assert!(!raw.0.is_null());
        assert_eq!(libraw_open_file(raw.0, path.as_ptr()), 0);
        libraw_set_output_color(raw.0, 1); // linear sRGB, independent color matrix
        libraw_set_output_bps(raw.0, 16);
        libraw_set_gamma(raw.0, 0, 1.0);
        libraw_set_gamma(raw.0, 1, 1.0);
        libraw_set_no_auto_bright(raw.0, 1);
        libraw_set_adjust_maximum_thr(raw.0, 0.0);
        for i in 0..4 {
            libraw_set_user_mul(raw.0, i, 1.0);
        }
        assert_eq!(libraw_unpack(raw.0), 0);

        assert_eq!(libraw_dcraw_process(raw.0), 0);
        let mut error = 0;
        let image = Bitmap(libraw_dcraw_make_mem_image(raw.0, &mut error));
        assert_eq!(error, 0);
        assert!(!image.0.is_null());
        let info = &*image.0;
        assert_eq!(info.kind, 2); // LIBRAW_IMAGE_BITMAP
        assert_eq!(info.colors, 3);
        assert_eq!(info.bits, 16);
        assert_eq!(
            info.size as usize,
            usize::from(info.width) * usize::from(info.height) * 6
        );
        let bytes = std::slice::from_raw_parts(info.data.as_ptr(), info.size as usize);
        let pixels = bytes
            .as_chunks::<2>()
            .0
            .iter()
            .map(|b| f32::from(u16::from_ne_bytes(*b)) / 65535.0)
            .collect();
        (info.width, info.height, pixels)
    }
}

#[test]
fn developed_dng_libraw_rgb_matches_engine_render() {
    // Non-neutral patches exercise the matrix, and a ramp catches exposure and
    // transfer errors. Avoid clipping so no decoder tone-map can hide a mismatch.
    let mut planes = vec![Vec::new(); 3];
    for y in 0..32 {
        for x in 0..48 {
            let pixel = [0.15 + x as f32 / 480.0, 0.15 + y as f32 / 480.0, 0.18];
            for (plane, value) in planes.iter_mut().zip(pixel) {
                plane.push(value);
            }
        }
    }
    let source = Image::new(48, 32, planes).unwrap();
    let input = ExportImage {
        source: RenderSource::Rgb(&source),
        name: "libraw",
        sequence: 1,
        date: "",
        metadata: None,
    };
    let recipe = engine_api::recipe::Recipe::default();
    let expected =
        pipeline_cpu::render_output_linear_scaled(&recipe.settings, &input.source, 1).unwrap();
    let dir = tempfile::tempdir().unwrap();
    let path = export_one(
        &input,
        &recipe,
        &ExportSettings {
            format: Format::Dng,
            metadata: Metadata::None,
            output_dir: dir.path().into(),
            ..Default::default()
        },
    )
    .unwrap();
    let (width, height, actual) = decode(&path);
    assert_eq!((width, height), (48, 32));
    let mut maximum = 0.0_f32;

    for (src, actual) in expected.pixels().zip(actual.as_chunks::<3>().0) {
        // Linear Rec.2020 -> linear sRGB, both D65. No fitted gain or offset.
        let [r, g, b] = src.0;
        let expected = [
            1.660491 * r - 0.587641 * g - 0.07285 * b,
            -0.12455 * r + 1.1329 * g - 0.008349 * b,
            -0.018151 * r - 0.100579 * g + 1.11873 * b,
        ];
        for (a, e) in actual.iter().zip(expected) {
            assert!((0.0..1.0).contains(&e), "fixture must stay in sRGB gamut");
            maximum = maximum.max((a - e).abs());
        }
    }
    assert!(maximum < 0.003, "maximum absolute RGB error {maximum}");
}
