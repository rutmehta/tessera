use export::{ExportImage, ExportSettings, Format, Metadata, export_one};
use pipeline_cpu::{Image, RenderSource};
use std::{fs, io::Read};
#[path = "support/dng_tags.rs"]
mod dng_tags;

#[test]
fn embedded_original_roundtrips_block_boundaries() {
    let source = Image::new(16, 16, vec![vec![0.2; 256]; 3]).unwrap();
    let input = ExportImage {
        source: RenderSource::Rgb(&source),
        name: "embedded",
        sequence: 1,
        date: "",
        metadata: None,
    };
    for len in [1, 65535, 65536, 65537, 131073, 2_200_000] {
        let dir = tempfile::tempdir().unwrap();
        let original = dir.path().join("source.nef");
        let mut seed = 1u32;
        let bytes: Vec<u8> = (0..len)
            .map(|_| {
                seed ^= seed << 13;
                seed ^= seed >> 17;
                seed ^= seed << 5;
                seed as u8
            })
            .collect();
        fs::write(&original, &bytes).unwrap();
        let path = export_one(
            &input,
            &Default::default(),
            &ExportSettings {
                format: Format::Dng,
                original_raw: Some(original.clone()),
                output_dir: dir.path().into(),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(fs::read(&original).unwrap(), bytes);
        let decoded = raw_decode::linear_dng::read(&mut fs::File::open(&path).unwrap()).unwrap();
        assert_eq!((decoded.width, decoded.height), (16, 16));
        let tags = dng_tags::read(&path);
        assert_eq!(tags[&50827].2, b"source.nef\0");
        if len == 65537 {
            match std::process::Command::new("exiftool")
                .args(["-b", "-OriginalRawImage"])
                .arg(&path)
                .output()
            {
                Ok(output) => {
                    assert!(
                        output.status.success(),
                        "{}",
                        String::from_utf8_lossy(&output.stderr)
                    );
                    assert_eq!(output.stdout, bytes, "independent ExifTool extraction");
                }
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                    eprintln!("ExifTool is not installed; block-envelope assertions still run")
                }
                Err(e) => panic!("ExifTool: {e}"),
            }
        }
        let data = &tags[&50828].2;
        // Independent decoder for the DNG big-endian, block-zlib envelope.
        let be = |at: usize| u32::from_be_bytes(data[at..at + 4].try_into().unwrap()) as usize;
        assert_eq!(be(0), len);
        let blocks = len.div_ceil(65536);
        assert_eq!(be(4), 4 * (blocks + 2));
        let mut decoded = Vec::new();
        for i in 0..blocks {
            let start = be(4 + i * 4);
            let end = be(8 + i * 4);
            flate2::read::ZlibDecoder::new(&data[start..end])
                .read_to_end(&mut decoded)
                .unwrap();
        }
        assert_eq!(decoded, bytes);
        assert_eq!(&data[be(4 + blocks * 4)..], &[0; 28]);
    }
}

#[test]
fn embedding_rejects_privacy_reduction_and_wrong_format_without_output() {
    let dir = tempfile::tempdir().unwrap();
    let original = dir.path().join("source.nef");
    fs::write(&original, b"private source metadata").unwrap();
    let source = Image::new(16, 16, vec![vec![0.2; 256]; 3]).unwrap();
    let input = ExportImage {
        source: RenderSource::Rgb(&source),
        name: "private",
        sequence: 1,
        date: "",
        metadata: None,
    };
    for (format, metadata, person, location) in [
        (Format::Png, Metadata::All, false, false),
        (Format::Dng, Metadata::None, false, false),
        (Format::Dng, Metadata::CopyrightOnly, false, false),
        (Format::Dng, Metadata::All, true, false),
        (Format::Dng, Metadata::All, false, true),
    ] {
        let result = export_one(
            &input,
            &Default::default(),
            &ExportSettings {
                format,
                metadata,
                remove_person_info: person,
                remove_location: location,
                original_raw: Some(original.clone()),
                output_dir: dir.path().join("output"),
                ..Default::default()
            },
        );
        assert!(result.is_err());
        assert!(!dir.path().join("output").exists());
    }
}
