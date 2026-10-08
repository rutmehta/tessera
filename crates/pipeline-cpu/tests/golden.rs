use test_fixtures::raw as raw_fixtures;

use engine_api::recipe::DevelopSettings;
use pipeline_cpu::{RenderSource, render_scaled};
use raw_decode::RawSource;
use std::{
    fs::{self, File},
    io::BufReader,
    path::{Path, PathBuf},
};

#[test]
fn raw_fixture_goldens() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures");
    let files = raw_fixtures::all("raw_fixture_goldens");
    if files.is_empty() {
        return;
    }
    for ext in raw_fixtures::EXTENSIONS {
        assert!(
            files
                .iter()
                .any(|p| p.extension().unwrap().eq_ignore_ascii_case(ext)),
            "missing {ext} fixture"
        );
    }

    for path in files {
        let mut source = RawSource::open(&path).unwrap();
        let cfa = source.decode_cfa().unwrap();
        let metadata = source.metadata();
        let mut settings = DevelopSettings::default();
        // Immutable M1/M2-08 goldens predate optics. Explicitly test the off path.
        settings.lens.profile = engine_api::recipe::settings::LensProfileSource::None;
        settings.lens.remove_chromatic_aberration = false;
        let rendered = render_scaled(
            &settings,
            &RenderSource::Cfa {
                image: &cfa,
                metadata: &metadata,
            },
            8,
        )
        .unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        assert_eq!(
            rendered.dimensions(),
            (
                metadata.default_crop[2].div_ceil(8),
                metadata.default_crop[3].div_ceil(8)
            )
        );
        let golden = root.join("golden").join(format!(
            "{}.png",
            path.file_stem().unwrap().to_string_lossy()
        ));
        assert!(
            golden.exists(),
            "missing immutable golden {}",
            golden.display()
        );
        let mut reader = png::Decoder::new(BufReader::new(File::open(&golden).unwrap()))
            .read_info()
            .unwrap();
        let mut expected = vec![0; reader.output_buffer_size()];
        let info = reader.next_frame(&mut expected).unwrap();
        assert_eq!((info.width, info.height), rendered.dimensions());
        assert_eq!(info.color_type, png::ColorType::Rgb);
        assert_eq!(info.bit_depth, png::BitDepth::Eight);
        let expected = &expected[..info.buffer_size()];
        assert_eq!(expected.len(), rendered.as_raw().len());
        if let Some(dir) = std::env::var_os("ENG1_CAPTURE") {
            fs::write(
                PathBuf::from(dir).join(format!(
                    "{}.rgb8",
                    path.file_stem().unwrap().to_string_lossy()
                )),
                rendered.as_raw(),
            )
            .unwrap();
        }
        let changed = expected
            .as_chunks::<3>()
            .0
            .iter()
            .zip(rendered.as_raw().as_chunks::<3>().0)
            .filter(|(a, b)| a != b)
            .count();
        eprintln!(
            "{}: changed pixels {changed}/{}",
            golden.display(),
            expected.len() / 3
        );
        let max = expected
            .iter()
            .zip(rendered.as_raw())
            .map(|(a, b)| a.abs_diff(*b))
            .max()
            .unwrap();
        assert!(
            max == 0,
            "{}: lens-off render is not bit-identical (max error {max}/255)",
            golden.display()
        );
        eprintln!(
            "{}: {}x{}, max abs error {max}/255",
            golden.display(),
            info.width,
            info.height
        );
    }
}
