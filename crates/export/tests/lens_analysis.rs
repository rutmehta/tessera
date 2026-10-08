//! The sparse sensor lens analysis used by GPU export resolves the same
//! corrections as the reference's whole-frame analysis on the five real RAW
//! fixtures (`test_fixtures::raw::root()`).
use engine_api::recipe::DevelopSettings;
use pipeline_cpu::{DemosaicAlgorithm, Image};
use std::time::Instant;

#[test]
fn sparse_lens_analysis_matches_reference_on_fixtures() {
    let names = [
        "canon-cr3.CR3",
        "sony-arw.ARW",
        "nikon-nef.NEF",
        "fuji-raf.RAF",
        "sample.dng",
    ];
    let Some(paths) = test_fixtures::raw::files(&test_fixtures::current_test(), &names) else {
        return;
    };
    let settings = DevelopSettings::default();
    for (name, path) in names.into_iter().zip(paths) {
        let mut raw = raw_decode::RawSource::open(path).unwrap();
        let cfa = raw.decode_cfa().unwrap();
        let m = raw.metadata();
        let plane = Image::from_pyramid(cfa.pyramid()).unwrap();
        let period = if matches!(m.cfa_layout, raw_decode::CfaLayout::XTrans(_)) {
            6
        } else {
            2
        };
        let start = Instant::now();
        let (w, h) = (plane.width(), plane.height());
        let mut recovered = Image::new(w, h, vec![vec![0.; (w * h) as usize]]).unwrap();
        for c in plane.coords() {
            recovered
                .put(
                    &pipeline_cpu::reconstruct_highlights(
                        &plane.tile(c, 4, period).unwrap(),
                        m.cfa_layout,
                        settings.linearize.highlight_reconstruction,
                    )
                    .unwrap(),
                )
                .unwrap();
        }
        let mut rgb = Image::new(w, h, vec![vec![0.; (w * h) as usize]; 3]).unwrap();
        for c in recovered.coords() {
            rgb.put(
                &pipeline_cpu::demosaic(
                    &recovered.tile(c, 3, period).unwrap(),
                    m.cfa_layout,
                    DemosaicAlgorithm::MalvarHeCutler,
                )
                .unwrap(),
            )
            .unwrap();
        }
        let whole = pipeline_cpu::resolve_lens(
            &rgb.downsample_crop(m.default_crop, 1).unwrap(),
            &settings.lens,
            Some(&m),
            &Default::default(),
        )
        .unwrap();
        let reference = start.elapsed();
        let start = Instant::now();
        let sparse = pipeline_cpu::resolve_lens_sensor(
            &plane.planes()[0],
            &m,
            &settings,
            &Default::default(),
        )
        .unwrap();
        let fast = start.elapsed();
        assert_eq!(format!("{whole:?}"), format!("{sparse:?}"), "{name}");
        println!(
            "LENS {name}: {:?} sample={:?}; whole-frame {reference:?}, sparse {fast:?}",
            sparse.source(),
            sparse
                .sample()
                .map(|s| (s.distortion.k1, s.ca_red, s.ca_blue, s.vignette)),
        );
    }
}
