//! Synthetic TIFF/DNG profile admission; no camera/vendor profile files.
use pipeline_adobe::dcp::DcpProfile;
#[allow(dead_code)]
#[path = "../../raw-decode/tests/support/mod.rs"]
mod support;

#[test]
fn lr10_reads_embedded_profile_from_linear_raw_subifd_in_both_byte_orders() {
    for be in [false, true] {
        let bytes = support::lossy_dng(be, false);
        let profile = DcpProfile::parse(&bytes).expect("embedded DNG profile");
        let rgb = profile.apply_tone([0.25; 3]);
        for channel in rgb {
            assert!((channel - 0.52069).abs() < 0.0001);
        }
    }
}
