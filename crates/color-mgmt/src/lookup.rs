use crate::{Error, Lut3d, Result};
use lcms2::{Flags, Intent, PixelFormat, Profile, Transform};

/// Sample an ICC abstract or RGB-to-RGB device-link look into a red-fastest cube.
///
/// Abstract profiles are evaluated as encoded sRGB -> PCS -> abstract -> sRGB
/// using relative colorimetry. Device links are evaluated directly: their RGB
/// encoding is the caller's responsibility (the link defines both endpoints).
/// Display, input, output, named-color and non-RGB device-link profiles are rejected.
/// Sizes must be 2..=256; 33 is a practical SDR default. Inputs span [0, 1].
/// Finite float CMM outputs are retained without clipping, including excursions
/// outside [0, 1]. A sampled cube approximates the original ICC transform.
pub fn sample_color_lookup_icc(bytes: &[u8], size: u32) -> Result<Lut3d> {
    use lcms2::{ColorSpaceSignature as Space, ProfileClassSignature as Class};
    if !(2..=256).contains(&size) {
        return Err(Error::Unsupported("ICC lookup cube size must be 2..=256"));
    }
    let profile = Profile::new_icc(bytes)?;
    let srgb = Profile::new_srgb();
    let profiles = match profile.device_class() {
        Class::AbstractClass => {
            if !matches!(profile.color_space(), Space::LabData | Space::XYZData)
                || !matches!(profile.pcs(), Space::LabData | Space::XYZData)
            {
                return Err(Error::Unsupported(
                    "abstract ICC lookup must use Lab or XYZ PCS",
                ));
            }
            vec![&srgb, &profile, &srgb]
        }
        Class::LinkClass => {
            if profile.color_space() != Space::RgbData || profile.pcs() != Space::RgbData {
                return Err(Error::Unsupported(
                    "ICC lookup device link must be RGB-to-RGB",
                ));
            }
            vec![&profile]
        }
        _ => {
            return Err(Error::Unsupported(
                "ICC lookup requires an abstract or device-link profile",
            ));
        }
    };
    let transform: Transform<[f32; 3], [f32; 3]> = Transform::new_multiprofile(
        &profiles,
        PixelFormat::RGB_FLT,
        PixelFormat::RGB_FLT,
        Intent::RelativeColorimetric,
        Flags::NO_OPTIMIZE,
    )?;
    let n = size as usize;
    let mut values = Vec::with_capacity(n.pow(3));
    for b in 0..n {
        for g in 0..n {
            for r in 0..n {
                values.push([r, g, b].map(|v| v as f32 / (n - 1) as f32));
            }
        }
    }
    transform.transform_in_place(&mut values);
    if values.iter().flatten().any(|v| !v.is_finite()) {
        return Err(Error::Unsupported("ICC lookup produced non-finite samples"));
    }
    Ok(Lut3d { size: n, values })
}

#[cfg(test)]
mod tests {
    use super::*;
    use lcms2::{Flags, GlobalContext, Intent, PixelFormat, Profile, Transform};

    #[test]
    fn rejects_non_lookup_profiles() {
        let bytes = Profile::new_srgb().icc().unwrap();
        assert!(matches!(
            sample_color_lookup_icc(&bytes, 3),
            Err(crate::Error::Unsupported(_))
        ));
    }

    #[test]
    fn rejects_invalid_grid_sizes_before_allocating() {
        let profile =
            Profile::new_bchsw_abstract_context(GlobalContext::new(), 5, 0.0, 1.0, 0.0, 0.0, None)
                .unwrap()
                .icc()
                .unwrap();
        for size in [0, 1, 257, u32::MAX] {
            assert!(matches!(
                sample_color_lookup_icc(&profile, size),
                Err(crate::Error::Unsupported(_))
            ));
        }
    }

    #[test]
    fn rejects_non_rgb_device_links() {
        let cmyk = Profile::ink_limiting_context(
            GlobalContext::new(),
            lcms2::ColorSpaceSignature::CmykData,
            200.0,
        )
        .unwrap();
        assert!(matches!(
            sample_color_lookup_icc(&cmyk.icc().unwrap(), 3),
            Err(crate::Error::Unsupported(_))
        ));
    }

    #[test]
    fn rejects_malformed_bytes() {
        for bytes in [&b""[..], &b"not an ICC"[..], &[0u8; 128][..]] {
            assert!(sample_color_lookup_icc(bytes, 3).is_err());
        }
    }

    #[test]
    fn rgb_device_link_is_sampled_directly() {
        let srgb = Profile::new_srgb();
        let mut registry = crate::Registry::new();
        let dst = registry.builtin(crate::Builtin::AdobeRgb).unwrap();
        let dst = Profile::new_icc(dst.icc_bytes()).unwrap();
        let transform: Transform<[f32; 3], [f32; 3]> = Transform::new(
            &srgb,
            PixelFormat::RGB_FLT,
            &dst,
            PixelFormat::RGB_FLT,
            Intent::RelativeColorimetric,
        )
        .unwrap();
        let link = Profile::new_device_link(&transform, 4.3, Flags::default()).unwrap();
        let bytes = link.icc().unwrap();
        let loaded = Profile::new_icc(&bytes).unwrap();
        let reference: Transform<[f32; 3], [f32; 3]> = Transform::new_multiprofile(
            &[&loaded],
            PixelFormat::RGB_FLT,
            PixelFormat::RGB_FLT,
            Intent::RelativeColorimetric,
            Flags::NO_OPTIMIZE,
        )
        .unwrap();
        let lut = sample_color_lookup_icc(&bytes, 5).unwrap();
        let mut expected = [[0.0; 3]];
        reference.transform_pixels(&[[0.25, 0.5, 0.75]], &mut expected);
        let actual = lut.values[1 + 5 * (2 + 5 * 3)];
        for c in 0..3 {
            assert!((actual[c] - expected[0][c]).abs() < 1e-5);
        }
        assert!((actual[0] - 0.25).abs() > 0.01);
    }

    #[test]
    fn abstract_profile_is_sampled_through_srgb() {
        let profile = Profile::new_bchsw_abstract_context(
            GlobalContext::new(),
            17,
            10.0,
            1.0,
            0.0,
            0.0,
            None,
        )
        .unwrap();
        let bytes = profile.icc().unwrap();
        let lut = sample_color_lookup_icc(&bytes, 5).unwrap();
        assert_eq!(lut.size, 5);
        assert_eq!(lut.values.len(), 125);
        let srgb = Profile::new_srgb();
        let loaded = Profile::new_icc(&bytes).unwrap();
        let transform: Transform<[f32; 3], [f32; 3]> = Transform::new_multiprofile(
            &[&srgb, &loaded, &srgb],
            PixelFormat::RGB_FLT,
            PixelFormat::RGB_FLT,
            Intent::RelativeColorimetric,
            Flags::NO_OPTIMIZE,
        )
        .unwrap();
        let input = [[0.25, 0.5, 0.75]];
        let mut expected = [[0.0; 3]];
        transform.transform_pixels(&input, &mut expected);
        let actual = lut.values[1 + 5 * (2 + 5 * 3)];
        for c in 0..3 {
            assert!((actual[c] - expected[0][c]).abs() < 1e-5);
        }
        assert!(
            lut.sample([0.5; 3])[0] > 0.55,
            "abstract brightness must be applied"
        );
    }
}
