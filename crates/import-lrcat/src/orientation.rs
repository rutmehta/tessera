//! Lightroom catalog orientations are absolute, including camera orientation.
/// Convert the two corner letters to EXIF's eight dihedral transforms.
pub fn exif(code: &str) -> Option<u16> {
    Some(match code {
        "AB" => 1,
        "BA" => 2,
        "CD" => 3,
        "DC" => 4,
        "CB" => 5,
        "BC" => 6,
        "AD" => 7,
        "DA" => 8,
        _ => return None,
    })
}
