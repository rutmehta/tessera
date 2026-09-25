use engine_api::recipe::DevelopSettings;
use pipeline_cpu::{RenderSource, render_scaled};
use raw_decode::RawSource;
use std::{
    fs::{self, File},
    io::{BufReader, BufWriter},
    path::{Path, PathBuf},
};

#[test]
fn raw_fixture_goldens() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures");
    let raw = std::env::var_os("PIPELINE_RAW_FIXTURES")
        .map(PathBuf::from)
        .unwrap_or_else(|| root.join("raw"));
    if !raw.exists() {
        eprintln!("skipping pipeline goldens: fixtures/raw is absent");
        return;
    }
    let mut files: Vec<_> = fs::read_dir(&raw)
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| {
            p.is_file()
                && p.extension().is_some_and(|e| {
                    matches!(
                        e.to_string_lossy().to_ascii_lowercase().as_str(),
                        "cr3" | "arw" | "nef" | "raf" | "dng"
                    )
                })
        })
        .collect();
    files.sort();
    for ext in ["cr3", "arw", "nef", "raf", "dng"] {
        assert!(
            files
                .iter()
                .any(|p| p.extension().unwrap().eq_ignore_ascii_case(ext)),
            "missing {ext} fixture"
        );
    }
    fs::create_dir_all(root.join("golden")).unwrap();
    for path in files {
        let mut source = RawSource::open(&path).unwrap();
        let cfa = source.decode_cfa().unwrap();
        let metadata = source.metadata();
        let rendered = render_scaled(
            &DevelopSettings::default(),
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
        if !golden.exists() {
            let file = File::options()
                .write(true)
                .create_new(true)
                .open(&golden)
                .unwrap();
            let mut encoder =
                png::Encoder::new(BufWriter::new(file), rendered.width(), rendered.height());
            encoder.set_color(png::ColorType::Rgb);
            encoder.set_depth(png::BitDepth::Eight);
            encoder.set_source_srgb(png::SrgbRenderingIntent::Perceptual);
            let mut writer = encoder.write_header().unwrap();
            writer.write_image_data(rendered.as_raw()).unwrap();
            writer.finish().unwrap();
            eprintln!(
                "created missing golden {} (review and commit)",
                golden.display()
            );
        }
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
        let max = expected
            .iter()
            .zip(rendered.as_raw())
            .map(|(a, b)| a.abs_diff(*b))
            .max()
            .unwrap();
        assert!(
            max <= 2,
            "{}: max absolute error {max}/255 exceeds 2/255",
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
