//! Narrow, non-fatal substitution for imported Adobe-named LinearRaw proxies.
use crate::{dcp::DcpProfile, names_adobe_profile};
use engine_api::recipe::DevelopSettings;
use pipeline_cpu::CameraLinearProxy;

pub const SUBSTITUTED_PROFILE_NOTICE: &str = "profile substituted (embedded DNG profile)";
pub const UNAVAILABLE_PROFILE_NOTICE: &str =
    "Embedded DNG profile unavailable; using the previous rendering behaviour.";

/// Call only from an Adobe process-family renderer. Installed profiles take
/// precedence and must bypass this helper. No profile identity is a file path.
/// Invalid metadata keeps the previous no-profile render available and yields
/// a value-free informational note for the host's per-photo status display.
pub fn embedded_profile_fallback(
    proxy: &CameraLinearProxy,
    settings: &DevelopSettings,
    bytes: Option<&[u8]>,
) -> (Option<DcpProfile>, Option<&'static str>) {
    if !proxy.is_external_dng() || !names_adobe_profile(settings) {
        return (None, None);
    }
    let planned = proxy.render_plan(settings, false).0;
    let profile = bytes
        .and_then(|bytes| DcpProfile::parse_embedded(bytes).ok())
        .filter(|profile| {
            profile
                .resolve_white_balance(&planned.white_balance, proxy.original_metadata().as_shot_wb)
                .is_ok()
        });
    let note = if profile.is_some() {
        SUBSTITUTED_PROFILE_NOTICE
    } else {
        UNAVAILABLE_PROFILE_NOTICE
    };
    (profile, Some(note))
}
