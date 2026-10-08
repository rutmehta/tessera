//! Exercise staged corrections when an opcode-bearing RAW fixture is available.
use test_fixtures::raw as raw_fixtures;

use pipeline_cpu::{RenderSource, render_linear_scaled};
use raw_decode::RawSource;

#[test]
fn real_opcode_fixtures_when_available() {
    const TEST: &str = "real_opcode_fixtures_when_available";
    let fixtures = raw_fixtures::all(TEST);
    let mut inspected = 0;
    let mut rendered = 0;
    for path in &fixtures {
        let mut source = RawSource::open(path).unwrap();
        let metadata = source.metadata();
        inspected += 1;
        if !metadata.opcode_lists.iter().flatten().any(|b| b.len() > 4) {
            continue;
        }
        let cfa = source.decode_cfa().unwrap();
        let output = render_linear_scaled(
            &Default::default(),
            &RenderSource::Cfa {
                image: &cfa,
                metadata: &metadata,
            },
            16,
        )
        .unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        assert!(output.planes().iter().flatten().all(|v| v.is_finite()));
        rendered += 1;
    }
    if !fixtures.is_empty() && rendered == 0 {
        raw_fixtures::notice(
            TEST,
            &format!(
                "none of the {inspected} RAW fixtures carries DNG opcode lists; staged opcode rendering was not exercised"
            ),
        );
    }
    eprintln!("opcode fixture coverage: inspected {inspected}, rendered {rendered}");
}
