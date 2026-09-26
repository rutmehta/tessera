//! Decoded, color-managed RGB sources.
use engine_api::{EngineError, EngineResult};
use std::path::Path;

/// An upright, decoded image stored as planar f32 linear Rec.2020 RGB.
///
/// Untagged input is assumed to be sRGB. Embedded RGB ICC profiles are
/// authoritative; invalid/unsupported profiles return a color error rather
/// than silently changing the interpretation. Alpha is discarded, not composited.
#[derive(Clone, Debug)]
pub struct RgbSource {
    pixels: pipeline_cpu::Image,
}

impl RgbSource {
    /// Validated already-linear Rec.2020 boundary; retains exact f32 bits.
    pub fn from_linear_rec2020(pixels: pipeline_cpu::Image) -> EngineResult<Self> {
        if pixels.planes().len() != 3
            || pixels.width() == 0
            || pixels.height() == 0
            || pixels.planes().iter().flatten().any(|v| !v.is_finite())
        {
            return Err(EngineError::invalid(
                "RGB raster",
                "nonempty finite three planes required",
            ));
        }
        Ok(Self { pixels })
    }

    /// Construct from upright, planar **linear** RGB tagged with its working
    /// profile. The profile's TRCs are removed, not applied a second time.
    /// Signed/HDR samples are transformed by a matrix without clipping. A CLUT
    /// profile is rejected because it cannot describe this linear boundary.
    pub fn from_raster(
        pixels: pipeline_cpu::Image,
        profile: &color_mgmt::Profile,
    ) -> EngineResult<Self> {
        if pixels.planes().len() != 3 {
            return Err(EngineError::invalid("RGB raster", "three planes required"));
        }
        let mut registry = color_mgmt::Registry::new();
        let linear = registry
            .linearized_rgb(profile)
            .map_err(color_error)?
            .ok_or_else(|| EngineError::Unsupported {
                what: "RGB raster requires a matrix-shaper working profile".into(),
            })?;
        let output = registry
            .builtin(color_mgmt::Builtin::LinearRec2020)
            .map_err(color_error)?;
        let transform = color_mgmt::Transform::new(
            &linear,
            &output,
            color_mgmt::TransformOptions {
                black_point_compensation: false,
                ..Default::default()
            },
        )
        .map_err(color_error)?;
        let basis = [[1., 0., 0.], [0., 1., 0.], [0., 0., 1.]].map(|v| transform.apply(v));
        let matrix = engine_api::color::ColorMatrix3(std::array::from_fn(|r| {
            std::array::from_fn(|c| f64::from(basis[c][r]))
        }));
        let mut planes: Vec<_> = (0..3)
            .map(|_| Vec::with_capacity(pixels.planes()[0].len()))
            .collect();
        for i in 0..pixels.planes()[0].len() {
            let rgb = matrix.apply(std::array::from_fn(|c| f64::from(pixels.planes()[c][i])));
            for (plane, value) in planes.iter_mut().zip(rgb) {
                plane.push(value as f32);
            }
        }
        Ok(Self {
            pixels: pipeline_cpu::Image::new(pixels.width(), pixels.height(), planes)?,
        })
    }

    /// Decode JPEG, PNG or TIFF without reducing integer/float sample precision.
    /// EXIF orientation is consumed here exactly once, before renderer geometry.
    /// HEIC/HEIF uses ImageIO on macOS when the `imageio` feature is enabled.
    pub fn open(path: impl AsRef<Path>) -> EngineResult<Self> {
        let path = path.as_ref();
        let bytes = std::fs::read(path).map_err(|e| EngineError::io_at(path, &e))?;
        let heic = path
            .extension()
            .and_then(|s| s.to_str())
            .is_some_and(|s| s.eq_ignore_ascii_case("heic") || s.eq_ignore_ascii_case("heif"));
        let (bytes, format) = if heic {
            #[cfg(all(target_os = "macos", feature = "imageio"))]
            {
                (
                    color_mgmt::decode_to_tiff(&bytes).map_err(|e| EngineError::Unsupported {
                        what: e.to_string(),
                    })?,
                    image::ImageFormat::Tiff,
                )
            }
            #[cfg(not(all(target_os = "macos", feature = "imageio")))]
            {
                return Err(EngineError::Unsupported {
                    what: "HEIC requires macOS ImageIO feature".into(),
                });
            }
        } else {
            (
                bytes,
                image::ImageFormat::from_path(path).map_err(decode_error)?,
            )
        };
        let reader = image::ImageReader::with_format(std::io::Cursor::new(&bytes), format);
        use image::ImageDecoder;
        let is_tiff = reader.format() == Some(image::ImageFormat::Tiff);
        let mut decoder = reader.into_decoder().map_err(decode_error)?;
        // image 0.25 limits TIFF tag allocations to the decoded pixel byte
        // count, silently dropping ICC profiles on small images. Read TIFF
        // metadata separately with the TIFF decoder's bounded default limits.
        let icc = if is_tiff {
            let error = |e: tiff::TiffError| EngineError::Decode {
                format: "TIFF".into(),
                message: e.to_string(),
            };
            let mut metadata =
                tiff::decoder::Decoder::new(std::io::Cursor::new(&bytes)).map_err(error)?;
            metadata
                .find_tag(tiff::tags::Tag::IccProfile)
                .map_err(error)?
                .map(|value| value.into_u8_vec())
                .transpose()
                .map_err(error)?
        } else {
            decoder.icc_profile().map_err(decode_error)?
        };
        let orientation = decoder.orientation().map_err(decode_error)?;
        let mut decoded = image::DynamicImage::from_decoder(decoder).map_err(decode_error)?;
        decoded.apply_orientation(orientation);
        let decoded = decoded.to_rgb32f();
        let mut registry = color_mgmt::Registry::new();
        let input = match icc {
            Some(bytes) => registry.load_bytes(&bytes),
            None => registry.builtin(color_mgmt::Builtin::Srgb),
        }
        .map_err(color_error)?;
        let output = registry
            .builtin(color_mgmt::Builtin::LinearRec2020)
            .map_err(color_error)?;
        let transform =
            color_mgmt::Transform::new(&input, &output, Default::default()).map_err(color_error)?;
        let mut planes: Vec<_> = (0..3)
            .map(|_| Vec::with_capacity(decoded.len() / 3))
            .collect();
        for pixel in decoded.pixels() {
            for (plane, value) in planes.iter_mut().zip(transform.apply(pixel.0)) {
                plane.push(value);
            }
        }
        Ok(Self {
            pixels: pipeline_cpu::Image::new(decoded.width(), decoded.height(), planes)?,
        })
    }
    pub fn pixels(&self) -> &pipeline_cpu::Image {
        &self.pixels
    }
    pub fn into_pixels(self) -> pipeline_cpu::Image {
        self.pixels
    }
    pub fn recognizes(path: impl AsRef<Path>) -> bool {
        path.as_ref()
            .extension()
            .and_then(|s| s.to_str())
            .is_some_and(|ext| {
                ["jpg", "jpeg", "jpe", "png", "tif", "tiff", "heic", "heif"]
                    .iter()
                    .any(|candidate| ext.eq_ignore_ascii_case(candidate))
            })
    }
}

fn decode_error(error: image::ImageError) -> EngineError {
    if matches!(error, image::ImageError::Unsupported(_)) {
        return EngineError::Unsupported {
            what: error.to_string(),
        };
    }
    EngineError::Decode {
        format: "RGB".into(),
        message: error.to_string(),
    }
}
fn color_error(error: color_mgmt::Error) -> EngineError {
    EngineError::Color {
        message: error.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use color_mgmt::{Builtin, Registry, Transform, TransformOptions};

    fn expected(rgb: [f32; 3], builtin: Builtin) -> [f32; 3] {
        let mut registry = Registry::new();
        let input = registry.builtin(builtin).unwrap();
        let output = registry.builtin(Builtin::LinearRec2020).unwrap();
        Transform::new(&input, &output, TransformOptions::default())
            .unwrap()
            .apply(rgb)
    }

    #[test]
    fn raster_constructor_preserves_linear_hdr_and_rejects_monochrome() {
        let profile = Registry::new().builtin(Builtin::LinearRec2020).unwrap();
        let planes = vec![vec![-0.25, 2.5], vec![0.25, 1.5], vec![0.75, 3.5]];
        let pixels = pipeline_cpu::Image::new(2, 1, planes.clone()).unwrap();
        let source = RgbSource::from_raster(pixels, &profile).unwrap();
        for (actual, expected) in source.pixels().planes().iter().zip(&planes) {
            for (a, b) in actual.iter().zip(expected) {
                assert!((a - b).abs() < 1e-5, "{a} != {b}");
            }
        }
        let mono = pipeline_cpu::Image::new(2, 1, vec![vec![0.5; 2]]).unwrap();
        assert!(RgbSource::from_raster(mono, &profile).is_err());
    }

    #[test]
    fn raster_constructor_uses_linear_profile_primaries_not_encoded_trcs() {
        let mut registry = Registry::new();
        let profile = registry.builtin(Builtin::DisplayP3).unwrap();
        let linear = registry.linearized_rgb(&profile).unwrap().unwrap();
        let output = registry.builtin(Builtin::LinearRec2020).unwrap();
        let transform = Transform::new(
            &linear,
            &output,
            TransformOptions {
                black_point_compensation: false,
                ..Default::default()
            },
        )
        .unwrap();
        let samples = [[0.15, 0.45, 0.8], [-0.2, 1.5, 3.0]];
        let planes = (0..3)
            .map(|c| samples.iter().map(|v| v[c]).collect())
            .collect();
        let pixels = pipeline_cpu::Image::new(2, 1, planes).unwrap();
        let source = RgbSource::from_raster(pixels, &profile).unwrap();
        for (i, sample) in samples.into_iter().enumerate() {
            for (plane, expected) in source.pixels().planes().iter().zip(transform.apply(sample)) {
                assert!((plane[i] - expected).abs() < 1e-5);
            }
        }
        let encoded = expected(samples[0], Builtin::DisplayP3);
        assert!((source.pixels().planes()[0][0] - encoded[0]).abs() > 0.01);
    }

    #[test]
    #[cfg(all(target_os = "macos", feature = "imageio"))]
    fn heic_pixels_are_decoded_through_imageio() {
        let source = RgbSource::open(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/rgb.heic"
        ))
        .unwrap();
        assert_eq!(
            (source.pixels().width(), source.pixels().height()),
            (32, 24)
        );
        assert!(source.pixels().planes()[0][0] > 0.1);
    }

    #[test]
    fn heic_reports_unsupported_instead_of_raw_decode_failure() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("photo.heic");
        std::fs::write(&path, b"unsupported HEIC fixture").unwrap();
        assert!(matches!(
            RgbSource::open(path),
            Err(EngineError::Unsupported { .. })
        ));
    }

    #[test]
    fn corrupt_input_and_invalid_icc_are_errors() {
        use image::ImageEncoder;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("broken.png");
        std::fs::write(&path, b"not an image").unwrap();
        assert!(matches!(
            RgbSource::open(&path),
            Err(EngineError::Decode { .. })
        ));
        assert!(matches!(
            RgbSource::open(dir.path().join("missing.jpg")),
            Err(EngineError::Io { .. })
        ));
        let mut bytes = Vec::new();
        let mut encoder = image::codecs::jpeg::JpegEncoder::new(&mut bytes);
        encoder.set_icc_profile(vec![0; 128]).unwrap();
        encoder
            .encode(&[120, 80, 40], 1, 1, image::ExtendedColorType::Rgb8)
            .unwrap();
        let path = dir.path().join("bad-icc.jpg");
        std::fs::write(&path, bytes).unwrap();
        assert!(matches!(
            RgbSource::open(path),
            Err(EngineError::Color { .. })
        ));
    }

    #[test]
    fn jpeg_exif_rotation_swaps_dimensions_once() {
        use image::ImageEncoder;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("oriented.jpg");
        // Little-endian TIFF IFD containing only Orientation = 6 (90° CW).
        let exif = vec![
            b'I', b'I', 42, 0, 8, 0, 0, 0, 1, 0, 0x12, 1, 3, 0, 1, 0, 0, 0, 6, 0, 0, 0, 0, 0, 0, 0,
        ];
        let mut bytes = Vec::new();
        let mut encoder = image::codecs::jpeg::JpegEncoder::new(&mut bytes);
        encoder.set_exif_metadata(exif).unwrap();
        encoder
            .encode(
                &[120, 80, 40].repeat(6),
                2,
                3,
                image::ExtendedColorType::Rgb8,
            )
            .unwrap();
        std::fs::write(&path, bytes).unwrap();
        let source = RgbSource::open(path).unwrap();
        assert_eq!((source.pixels().width(), source.pixels().height()), (3, 2));
    }

    #[test]
    fn recognizes_supported_extensions_case_insensitively() {
        for ext in ["jpg", "JPEG", "jpe", "Png", "tif", "TIFF", "heic", "HEIF"] {
            assert!(RgbSource::recognizes(format!("photo.{ext}")), "{ext}");
        }
        for path in ["photo", "photo.cr3", "photo.jpg.xmp", "photo.gif"] {
            assert!(!RgbSource::recognizes(path));
        }
    }

    #[test]
    fn tiff_float_with_linear_icc_retains_hdr_and_sub_byte_precision() {
        use tiff::{
            encoder::{TiffEncoder, colortype::RGB32Float},
            tags::Tag,
        };
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("float.tiff");
        let profile = Registry::new().builtin(Builtin::LinearRec2020).unwrap();
        let samples = [0.123456f32, 0.500001, 2.0, 0.123466, 0.500011, 4.0];
        let mut encoder = TiffEncoder::new(std::fs::File::create(&path).unwrap()).unwrap();
        let mut image = encoder.new_image::<RGB32Float>(2, 1).unwrap();
        image
            .encoder()
            .write_tag(Tag::IccProfile, profile.icc_bytes())
            .unwrap();
        image.write_data(&samples).unwrap();
        let source = RgbSource::open(path).unwrap();
        for (i, rgb) in samples.as_chunks::<3>().0.iter().enumerate() {
            for (plane, value) in source.pixels().planes().iter().zip(rgb) {
                assert!((plane[i] - value).abs() < 1e-5, "{} != {value}", plane[i]);
            }
        }
    }

    #[test]
    fn tiff_16_bit_preserves_precision_and_applies_each_orientation_once() {
        use tiff::{
            encoder::{TiffEncoder, colortype::RGB16},
            tags::Tag,
        };
        let dir = tempfile::tempdir().unwrap();
        let values = [30000u16, 30001, 30002, 30003, 30004, 30005];
        let samples: Vec<u16> = values.iter().flat_map(|v| [*v; 3]).collect();
        let mappings = [
            [0, 1, 2, 3, 4, 5],
            [1, 0, 3, 2, 5, 4],
            [5, 4, 3, 2, 1, 0],
            [4, 5, 2, 3, 0, 1],
            [0, 2, 4, 1, 3, 5],
            [4, 2, 0, 5, 3, 1],
            [5, 3, 1, 4, 2, 0],
            [1, 3, 5, 0, 2, 4],
        ];
        for (i, mapping) in mappings.iter().enumerate() {
            let path = dir.path().join(format!("orientation-{i}.tiff"));
            let file = std::fs::File::create(&path).unwrap();
            let mut encoder = TiffEncoder::new(file).unwrap();
            let mut image = encoder.new_image::<RGB16>(2, 3).unwrap();
            image
                .encoder()
                .write_tag(Tag::Orientation, (i + 1) as u16)
                .unwrap();
            image.write_data(&samples).unwrap();
            let source = RgbSource::open(path).unwrap();
            let pixels = source.pixels();
            assert_eq!(
                (pixels.width(), pixels.height()),
                if i < 4 { (2, 3) } else { (3, 2) }
            );
            for (j, original) in mapping.iter().enumerate() {
                let target = expected([values[*original] as f32 / 65535.; 3], Builtin::Srgb);
                for (plane, value) in pixels.planes().iter().zip(target) {
                    assert!(
                        (plane[j] - value).abs() < 1e-6,
                        "orientation {} at {j}: {} != {value}",
                        i + 1,
                        plane[j]
                    );
                }
            }
            assert_ne!(pixels.planes()[0][0], pixels.planes()[0][1]);
        }
    }

    #[test]
    fn jpeg_embedded_adobe_rgb_is_not_treated_as_srgb() {
        use image::ImageEncoder;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("adobe.jpg");
        let profile = Registry::new().builtin(Builtin::AdobeRgb).unwrap();
        let mut bytes = Vec::new();
        let mut encoder = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut bytes, 100);
        encoder
            .set_icc_profile(profile.icc_bytes().to_vec())
            .unwrap();
        encoder
            .encode(
                &[80, 190, 110].repeat(64),
                8,
                8,
                image::ExtendedColorType::Rgb8,
            )
            .unwrap();
        std::fs::write(&path, &bytes).unwrap();
        let encoded = image::load_from_memory(&bytes)
            .unwrap()
            .to_rgb32f()
            .get_pixel(0, 0)
            .0;
        let target = expected(encoded, Builtin::AdobeRgb);
        let srgb = expected(encoded, Builtin::Srgb);
        assert!(target.iter().zip(srgb).any(|(a, b)| (a - b).abs() > 0.01));
        let source = RgbSource::open(path).unwrap();
        for (plane, value) in source.pixels().planes().iter().zip(target) {
            assert!((plane[0] - value).abs() < 1e-6, "{} != {value}", plane[0]);
        }
    }

    #[test]
    fn png_defaults_to_srgb_and_exposes_linear_planes() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("sample.png");
        image::RgbImage::from_pixel(2, 1, image::Rgb([128, 64, 32]))
            .save(&path)
            .unwrap();
        let source = RgbSource::open(&path).unwrap();
        let target = expected([128. / 255., 64. / 255., 32. / 255.], Builtin::Srgb);
        assert_eq!((source.pixels().width(), source.pixels().height()), (2, 1));
        for (plane, value) in source.pixels().planes().iter().zip(target) {
            assert!((plane[0] - value).abs() < 1e-6);
        }
        assert_eq!(source.into_pixels().planes().len(), 3);
    }
}
