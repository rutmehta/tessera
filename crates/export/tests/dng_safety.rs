use export::{ExportImage, ExportSettings, Format, Metadata, export_one};
use pipeline_cpu::{Image, RenderSource};
use std::{fs, sync::mpsc, time::Duration};

fn render(
    dir: &std::path::Path,
    original: Option<std::path::PathBuf>,
) -> engine_api::EngineResult<std::path::PathBuf> {
    let source = Image::new(16, 16, vec![vec![0.2; 256]; 3]).unwrap();
    export_one(
        &ExportImage {
            source: RenderSource::Rgb(&source),
            name: "safe",
            sequence: 1,
            date: "",
            metadata: None,
        },
        &Default::default(),
        &ExportSettings {
            format: Format::Dng,
            metadata: if original.is_some() {
                Metadata::All
            } else {
                Metadata::None
            },
            original_raw: original,
            output_dir: dir.into(),
            ..Default::default()
        },
    )
}

#[test]
fn rejects_unsupported_backward_version() {
    let dir = tempfile::tempdir().unwrap();
    let path = render(dir.path(), None).unwrap();
    let mut bytes = fs::read(path).unwrap();
    let ifd = u32::from_le_bytes(bytes[4..8].try_into().unwrap()) as usize;
    let count = u16::from_le_bytes(bytes[ifd..ifd + 2].try_into().unwrap()) as usize;
    let entry = (0..count)
        .map(|i| ifd + 2 + i * 12)
        .find(|&at| u16::from_le_bytes(bytes[at..at + 2].try_into().unwrap()) == 50707)
        .unwrap();
    bytes[entry + 8..entry + 12].copy_from_slice(&[1, 6, 0, 0]);
    assert!(raw_decode::linear_dng::read(&mut std::io::Cursor::new(bytes)).is_err());
}

#[test]
fn opaque_original_still_requires_valid_type_range_and_unique_tag() {
    let dir = tempfile::tempdir().unwrap();
    let original = dir.path().join("original.nef");
    fs::write(&original, b"opaque original").unwrap();
    let path = render(dir.path(), Some(original)).unwrap();
    let bytes = fs::read(path).unwrap();
    let ifd = u32::from_le_bytes(bytes[4..8].try_into().unwrap()) as usize;
    let count = u16::from_le_bytes(bytes[ifd..ifd + 2].try_into().unwrap()) as usize;
    let entry = (0..count)
        .map(|i| ifd + 2 + i * 12)
        .find(|&at| u16::from_le_bytes(bytes[at..at + 2].try_into().unwrap()) == 50828)
        .unwrap();
    for case in 0..3 {
        let mut broken = bytes.clone();
        let expected = match case {
            0 => {
                broken[entry + 2..entry + 4].copy_from_slice(&2u16.to_le_bytes());
                "invalid OriginalRawFileData"
            }
            1 => {
                broken[entry + 8..entry + 12].copy_from_slice(&u32::MAX.to_le_bytes());
                "invalid OriginalRawFileData"
            }
            _ => {
                broken[ifd + 2..ifd + 14].copy_from_slice(&bytes[entry..entry + 12]);
                "duplicate TIFF tag"
            }
        };
        let error = raw_decode::linear_dng::read(&mut std::io::Cursor::new(broken)).unwrap_err();
        assert!(error.to_string().contains(expected), "{error}");
    }
}

#[test]
fn fifo_original_is_rejected_without_blocking() {
    let dir = tempfile::tempdir().unwrap();
    let fifo = dir.path().join("source.nef");
    assert!(
        std::process::Command::new("mkfifo")
            .arg(&fifo)
            .status()
            .unwrap()
            .success()
    );
    let output = dir.path().join("out");
    let (tx, rx) = mpsc::channel();
    let source = fifo.clone();
    let worker = std::thread::spawn(move || {
        tx.send(render(&output, Some(source))).unwrap();
    });
    let received = rx.recv_timeout(Duration::from_secs(3));
    if received.is_err() {
        // Release the old blocking reader, so the regression test fails cleanly
        // instead of leaving a hung test thread or subprocess behind.
        let _release = fs::OpenOptions::new().write(true).open(&fifo).unwrap();
        let _ = rx.recv_timeout(Duration::from_secs(3));
    }
    worker.join().unwrap();
    assert!(received.is_ok(), "opening a FIFO blocked the exporter");
    assert!(received.unwrap().is_err());
}
