//! ENG-8: built-in maker-note lens corrections on the repo's raw fixtures.
//! Expected values are ExifTool 13.55's reading of the same tags
//! (`exiftool -FujiIFD:all fuji-raf.RAF`), an independent parser.
use raw_decode::{MakerLens, RawSource};

#[test]
fn fixtures_expose_only_fujifilm_maker_note_corrections() {
    let test = test_fixtures::current_test();
    let files = test_fixtures::raw::all(&test);
    for path in files {
        let name = test_fixtures::raw::name(&path);
        let metadata = RawSource::open(&path).unwrap().metadata();
        let is_raf = path
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("raf"));
        match (&metadata.maker_lens, is_raf) {
            (Some(MakerLens::Fujifilm(f)), true) => {
                let knots: Vec<f64> = (0..=10).map(|i| i as f64 / 10.).collect();
                let distortion = [
                    0., 0.102, 0.205, 0.307, 0.408, 0.517, 0.66, 0.879, 1.184, 1.598, 2.158,
                ];
                let red = [
                    0., 0.000103, 0.000188, 0.000238, 0.000235, 0.000183, 0.000102, 7e-06,
                    -0.000102, -0.000227, -0.000366,
                ];
                let blue = [
                    0., -3.8e-05, -6.6e-05, -7.1e-05, -4.2e-05, 3e-05, 0.00014, 0.000235, 0.000376,
                    0.00054, 0.000793,
                ];
                let vignetting = [
                    100., 99.92, 99.77, 99.46, 98.94, 98.43, 98.11, 97.29, 96.78, 95.88, 94.9,
                ];
                let close = |a: &[f64], b: &[f64]| {
                    a.len() == b.len() && a.iter().zip(b).all(|(x, y)| (x - y).abs() < 1e-9)
                };
                assert!(close(&f.knots, &knots), "{name}: {:?}", f.knots);
                assert!(
                    close(&f.distortion, &distortion),
                    "{name}: {:?}",
                    f.distortion
                );
                assert!(close(&f.ca_red, &red), "{name}: {:?}", f.ca_red);
                assert!(close(&f.ca_blue, &blue), "{name}: {:?}", f.ca_blue);
                assert!(
                    close(&f.vignetting, &vignetting),
                    "{name}: {:?}",
                    f.vignetting
                );
                assert_eq!(f.crop_factor, 1., "{name}: X-E2S has no crop mode");
            }
            (None, false) => {}
            (got, _) => panic!("{name}: unexpected maker-note correction {got:?}"),
        }
    }
}
