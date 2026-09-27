//! Finalize developed DNGs only (never used to rewrite source originals).
//! The shared merge writer supplies the float32 LinearRaw image. Append a new
//! directory without relocating its samples, then publish through PreparedExport.
use crate::{ExportSettings, Format, Metadata, encode_error};
use engine_api::{EngineResult, jobs::CancellationToken};
use std::{
    fs::File,
    io::{Read, Seek, SeekFrom, Write},
    path::Path,
};

pub(crate) fn validate(settings: &ExportSettings) -> EngineResult<()> {
    if settings.original_raw.is_some()
        && (!matches!(settings.format, Format::Dng)
            || !matches!(settings.metadata, Metadata::All)
            || settings.remove_person_info
            || settings.remove_location
            || !settings.keywords_as_hierarchy)
    {
        return Err(encode_error(
            "original raw embedding requires DNG and unrestricted metadata: the embedded original retains private metadata",
        ));
    }
    Ok(())
}

pub(crate) fn finish(
    file: &mut File,
    original: Option<&Path>,
    cancel: &CancellationToken,
) -> EngineResult<()> {
    let result = finish_io(file, original, cancel);
    cancel.check()?;
    result.map_err(encode_error)
}

fn finish_io(
    file: &mut File,
    original: Option<&Path>,
    cancel: &CancellationToken,
) -> std::io::Result<()> {
    let invalid = || std::io::Error::other("invalid developed DNG directory");
    file.rewind()?;
    let mut header = [0; 8];
    file.read_exact(&mut header)?;
    if &header[..4] != b"II\x2a\0" {
        return Err(invalid());
    }
    file.seek(SeekFrom::Start(
        u32::from_le_bytes(header[4..].try_into().unwrap()).into(),
    ))?;
    let mut count = [0; 2];
    file.read_exact(&mut count)?;
    let count = u16::from_le_bytes(count);
    if !(1..=64).contains(&count) {
        return Err(invalid());
    }
    let mut entries = vec![[0; 12]; count as usize];
    for entry in &mut entries {
        file.read_exact(entry)?;
    }
    let mut next = [0; 4];
    file.read_exact(&mut next)?;
    if next != [0; 4] {
        return Err(invalid());
    }
    let version = entries
        .iter_mut()
        .find(|e| u16::from_le_bytes([e[0], e[1]]) == 50706)
        .ok_or_else(invalid)?;
    if version[2..8] != [1, 0, 4, 0, 0, 0] {
        return Err(invalid());
    }
    // DNG 1.6 adds optional features, not mandatory tags for this linear image.
    // Keep DNGBackwardVersion=1.4 because float samples require a 1.4 reader.
    version[8..12].copy_from_slice(&[1, 6, 0, 0]);
    file.seek(SeekFrom::End(0))?;
    if let Some(path) = original {
        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .filter(|n| n.is_ascii() && !n.contains('\0'))
            .ok_or_else(|| std::io::Error::other("original filename must be ASCII"))?;
        if !path.metadata()?.is_file() {
            return Err(std::io::Error::other("original raw must be a regular file"));
        }
        let mut options = std::fs::OpenOptions::new();
        options.read(true);
        // A path could be replaced with a FIFO after the metadata check. Open
        // nonblocking and validate the actual descriptor as well.
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.custom_flags(libc::O_NONBLOCK);
        }
        let mut input = options.open(path)?;
        let metadata = input.metadata()?;
        // Classic TIFF and the original-raw envelope use 32-bit offsets. Bound
        // input before allocating the block table; never buffer the whole raw.
        if !metadata.is_file() || metadata.len() == 0 || metadata.len() > 1024 * 1024 * 1024 {
            return Err(std::io::Error::other(
                "original raw must be a nonempty regular file at most 1 GiB",
            ));
        }
        let mut filename = name.as_bytes().to_vec();
        filename.push(0);
        entries.push(payload(file, 50827, 2, &filename)?);
        align(file)?;
        let start = position(file)?;
        let len = metadata.len() as u32;
        let blocks = len.div_ceil(65536);
        let table_size = 4 * (blocks + 2);
        file.write_all(&vec![0; table_size as usize])?;
        let mut offsets = vec![table_size];
        let mut buffer = vec![0; 65536];
        let mut remaining = len as usize;
        while remaining != 0 {
            cancel.check().map_err(std::io::Error::other)?;
            let size = remaining.min(buffer.len());
            input.read_exact(&mut buffer[..size])?;
            let mut encoder =
                flate2::write::ZlibEncoder::new(&mut *file, flate2::Compression::default());
            encoder.write_all(&buffer[..size])?;
            encoder.finish()?;
            offsets.push(position(file)? - start);
            remaining -= size;
        }
        if input.read(&mut [0])? != 0 {
            return Err(std::io::Error::other("original changed during embedding"));
        }
        // Empty resource fork, type, creator, THM data/resource/type/creator.
        // Empty forks are encoded as a single zero length, not an offset table.
        file.write_all(&[0; 28])?;
        let end = position(file)?;
        file.seek(SeekFrom::Start(start.into()))?;
        file.write_all(&len.to_be_bytes())?;
        for offset in offsets {
            file.write_all(&offset.to_be_bytes())?;
        }
        file.seek(SeekFrom::Start(end.into()))?;
        entries.push(entry(50828, 7, end - start, start.to_le_bytes()));
    }
    align(file)?;
    let directory = position(file)?;
    entries.sort_by_key(|e| u16::from_le_bytes([e[0], e[1]]));
    if entries.windows(2).any(|w| w[0][..2] == w[1][..2]) {
        return Err(invalid());
    }
    file.write_all(&(entries.len() as u16).to_le_bytes())?;
    for entry in entries {
        file.write_all(&entry)?;
    }
    file.write_all(&[0; 4])?;
    position(file)?; // reject classic-TIFF overflow before changing IFD0
    file.seek(SeekFrom::Start(4))?;
    file.write_all(&directory.to_le_bytes())?;
    Ok(())
}

fn position(file: &mut File) -> std::io::Result<u32> {
    u32::try_from(file.stream_position()?)
        .map_err(|_| std::io::Error::other("DNG exceeds classic TIFF limit"))
}
fn align(file: &mut File) -> std::io::Result<()> {
    if file.stream_position()? % 2 != 0 {
        file.write_all(&[0])?;
    }
    Ok(())
}
fn entry(tag: u16, kind: u16, count: u32, value: [u8; 4]) -> [u8; 12] {
    let mut entry = [0; 12];
    entry[..2].copy_from_slice(&tag.to_le_bytes());
    entry[2..4].copy_from_slice(&kind.to_le_bytes());
    entry[4..8].copy_from_slice(&count.to_le_bytes());
    entry[8..].copy_from_slice(&value);
    entry
}
fn payload(file: &mut File, tag: u16, kind: u16, bytes: &[u8]) -> std::io::Result<[u8; 12]> {
    let count = u32::try_from(bytes.len()).map_err(std::io::Error::other)?;
    let value = if bytes.len() <= 4 {
        let mut value = [0; 4];
        value[..bytes.len()].copy_from_slice(bytes);
        value
    } else {
        align(file)?;
        let at = position(file)?;
        file.write_all(bytes)?;
        at.to_le_bytes()
    };
    Ok(entry(tag, kind, count, value))
}
