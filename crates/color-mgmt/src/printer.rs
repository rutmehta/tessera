//! Output (printer) profiles installed through ColorSync, and conversion of
//! document RGB into a printer's device space for application-managed printing.
use crate::{Error, Intent, Profile, Result};
use lcms2::{ColorSpaceSignature, Flags, InfoType, Locale, PixelFormat, ProfileClassSignature};
use std::path::{Path, PathBuf};

/// One installed ICC output profile.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OutputProfileInfo {
    pub path: PathBuf,
    /// The profile's own description (falls back to the file name).
    pub name: String,
    /// "RGB", "CMYK" or "Gray".
    pub color_space: &'static str,
}

/// Folders ColorSync searches for installed profiles, most specific first:
/// the user's, the local domain's (printer drivers install under
/// `/Library/Printers/<vendor>/…` as well), the network's and the system's.
pub fn colorsync_profile_roots() -> Vec<PathBuf> {
    let mut roots = Vec::new();
    if let Some(home) = std::env::var_os("HOME") {
        roots.push(Path::new(&home).join("Library/ColorSync/Profiles"));
    }
    for root in [
        "/Library/ColorSync/Profiles",
        "/Library/Printers",
        "/Network/Library/ColorSync/Profiles",
        "/System/Library/ColorSync/Profiles",
    ] {
        roots.push(root.into());
    }
    roots
}

/// Installed output-class profiles in RGB, CMYK or gray, sorted by name.
/// Unreadable files and other profile classes are skipped silently.
pub fn installed_output_profiles() -> Vec<OutputProfileInfo> {
    output_profiles(&colorsync_profile_roots())
}

/// Output-class profiles under `roots` (recursive, following no symlinked
/// directories, at most six levels deep). Duplicates by content are dropped.
pub fn output_profiles(roots: &[PathBuf]) -> Vec<OutputProfileInfo> {
    let mut found = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for root in roots {
        walk(root, 0, &mut |path| {
            let Ok(bytes) = std::fs::read(path) else {
                return;
            };
            let Some(info) = describe_output(path, &bytes) else {
                return;
            };
            if seen.insert(*blake3::hash(&bytes).as_bytes()) {
                found.push(info);
            }
        });
    }
    found.sort_by_key(|a| a.name.to_lowercase());
    found
}

fn walk(dir: &Path, depth: usize, visit: &mut dyn FnMut(&Path)) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    let mut entries: Vec<_> = entries.flatten().collect();
    entries.sort_by_key(|e| e.file_name());
    for entry in entries {
        let path = entry.path();
        let Ok(kind) = entry.file_type() else {
            continue;
        };
        if kind.is_dir() {
            if depth < 6 {
                walk(&path, depth + 1, visit);
            }
        } else if path
            .extension()
            .and_then(|e| e.to_str())
            .is_some_and(|e| e.eq_ignore_ascii_case("icc") || e.eq_ignore_ascii_case("icm"))
        {
            visit(&path);
        }
    }
}

/// Describes `bytes` when it is an output-class profile in a printable space.
pub fn describe_output(path: &Path, bytes: &[u8]) -> Option<OutputProfileInfo> {
    // Cheap header check before handing the file to the CMM.
    if bytes.len() < 128 || &bytes[36..40] != b"acsp" || &bytes[12..16] != b"prtr" {
        return None;
    }
    let profile = lcms2::Profile::new_icc(bytes).ok()?;
    if profile.device_class() != ProfileClassSignature::OutputClass {
        return None;
    }
    let color_space = space_name(profile.color_space())?;
    let name = profile
        .info(InfoType::Description, Locale::none())
        .map(|s| s.trim().to_owned())
        .filter(|s| !s.is_empty())
        .or_else(|| path.file_stem().map(|s| s.to_string_lossy().into_owned()))?;
    Some(OutputProfileInfo {
        path: path.to_path_buf(),
        name,
        color_space,
    })
}

fn space_name(space: ColorSpaceSignature) -> Option<&'static str> {
    match space {
        ColorSpaceSignature::RgbData => Some("RGB"),
        ColorSpaceSignature::CmykData => Some("CMYK"),
        ColorSpaceSignature::GrayData => Some("Gray"),
        _ => None,
    }
}

/// Device pixels in a destination profile's own space.
#[derive(Clone, Debug)]
pub struct DeviceImage {
    /// 1 (gray), 3 (RGB) or 4 (CMYK). CMYK 0 means no ink, 255 full ink.
    pub channels: usize,
    /// Interleaved 8-bit samples, `channels` per pixel.
    pub data: Vec<u8>,
}

/// Converts interleaved RGB floats encoded in `source` into `destination`'s
/// device space (application-managed printing: the print system is then told
/// the data is already in the printer's space). Intent and black-point
/// compensation are the user's choice.
pub fn convert_rgb(
    source: &Profile,
    destination: &Profile,
    intent: Intent,
    black_point_compensation: bool,
    rgb: &[[f32; 3]],
) -> Result<DeviceImage> {
    let src = lcms2::Profile::new_icc(source.icc_bytes())?;
    let dst = lcms2::Profile::new_icc(destination.icc_bytes())?;
    if src.color_space() != ColorSpaceSignature::RgbData {
        return Err(Error::Unsupported("conversion source must be RGB"));
    }
    let (format, channels) = match dst.color_space() {
        ColorSpaceSignature::RgbData => (PixelFormat::RGB_8, 3),
        ColorSpaceSignature::CmykData => (PixelFormat::CMYK_8, 4),
        ColorSpaceSignature::GrayData => (PixelFormat::GRAY_8, 1),
        _ => {
            return Err(Error::Unsupported(
                "printer profile must be RGB, CMYK or gray",
            ));
        }
    };
    let mut flags = Flags::default();
    if black_point_compensation {
        flags = flags | Flags::BLACKPOINT_COMPENSATION;
    }
    let transform: lcms2::Transform<[f32; 3], u8> =
        lcms2::Transform::new_flags(&src, PixelFormat::RGB_FLT, &dst, format, intent, flags)?;
    let mut data = vec![0u8; rgb.len() * channels];
    // Rows of 64k pixels keep the float→byte work cache friendly.
    for (src, dst) in rgb
        .chunks(1 << 16)
        .zip(data.chunks_mut((1 << 16) * channels))
    {
        transform.transform_pixels(src, dst);
    }
    Ok(DeviceImage { channels, data })
}
