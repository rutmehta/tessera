//! Explicit process-revision golden update. Never run from regression tests.
use engine_api::recipe::{DevelopSettings, ProcessVersion};
use pipeline_cpu::{RenderSource, render_scaled};
use raw_decode::RawSource;
use std::{fs::File, io::BufWriter, path::Path};

fn main() {
    assert_eq!(ProcessVersion::NATIVE_CURRENT.revision, 2);
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures");
    for name in [
        "canon-cr3.CR3",
        "fuji-raf.RAF",
        "nikon-nef.NEF",
        "sample.dng",
        "sony-arw.ARW",
    ] {
        let path = root.join("raw").join(name);
        let mut source = RawSource::open(&path).unwrap();
        let image = source.decode_cfa().unwrap();
        let metadata = source.metadata();
        let rgb = render_scaled(
            &DevelopSettings::default(),
            &RenderSource::Cfa {
                image: &image,
                metadata: &metadata,
            },
            8,
        )
        .unwrap();
        let destination = root
            .join("golden")
            .join(path.file_stem().unwrap())
            .with_extension("png");
        let mut encoder = png::Encoder::new(
            BufWriter::new(File::create(&destination).unwrap()),
            rgb.width(),
            rgb.height(),
        );
        encoder.set_color(png::ColorType::Rgb);
        encoder.set_depth(png::BitDepth::Eight);
        encoder.set_source_srgb(png::SrgbRenderingIntent::Perceptual);
        encoder
            .write_header()
            .unwrap()
            .write_image_data(rgb.as_raw())
            .unwrap();
        println!(
            "{}: {}x{} revision 2",
            destination.display(),
            rgb.width(),
            rgb.height()
        );
    }
}
