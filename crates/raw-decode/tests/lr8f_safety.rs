//! Deterministic, complete synthetic inputs only.
#[allow(dead_code)]
mod support;
use std::io::{self, Cursor, Read, Seek, SeekFrom};
fn entry(b: &[u8], tag: u16) -> usize {
    (0..u16::from_le_bytes(b[38..40].try_into().unwrap()) as usize)
        .map(|i| 40 + 12 * i)
        .find(|&p| u16::from_le_bytes(b[p..p + 2].try_into().unwrap()) == tag)
        .unwrap()
}
fn set(b: &mut [u8], tag: u16, value: u32) {
    let p = entry(b, tag);
    b[p + 8..p + 12].copy_from_slice(&value.to_le_bytes());
}
fn array(b: &mut Vec<u8>, tag: u16, values: &[u32]) {
    let p = entry(b, tag);
    b[p + 4..p + 8].copy_from_slice(&(values.len() as u32).to_le_bytes());
    let offset = b.len() as u32;
    set(b, tag, offset);
    b.extend(values.iter().flat_map(|v| v.to_le_bytes()));
}
#[test]
fn other_linear_compressions_and_single_channel_fall_through() {
    for compression in [1, 7, 8, 34892, 52546] {
        for channels in [1, 3] {
            if channels == 3 && matches!(compression, 34892 | 52546) {
                continue;
            }
            let mut b = support::lossy_dng(false, false);
            set(&mut b, 259, compression);
            set(&mut b, 277, channels);
            for malformed in [false, true] {
                if malformed {
                    let p = entry(&b, 256);
                    b[p + 2..p + 4].copy_from_slice(&99u16.to_le_bytes());
                }
                assert!(
                    raw_decode::lossy_dng::read(&mut Cursor::new(&b))
                        .unwrap()
                        .is_none(),
                    "compression={compression}, channels={channels}"
                );
                assert!(
                    raw_decode::lossy_dng::read_metadata(&mut Cursor::new(&b))
                        .unwrap()
                        .is_none()
                );
            }
        }
    }
}
#[test]
fn aliased_tiles_cannot_amplify_compressed_reads() {
    let mut b = support::lossy_dng(false, false);
    let p = entry(&b, 324);
    let offset = u32::from_le_bytes(b[p + 8..p + 12].try_into().unwrap());
    let count = (b.len() as u32) - offset;
    set(&mut b, 256, 128);
    array(&mut b, 324, &[offset; 8]);
    array(&mut b, 325, &[count; 8]);
    let error = raw_decode::lossy_dng::read_metadata(&mut Cursor::new(&b)).unwrap_err();
    assert!(
        error.to_string().contains("compressed") || error.to_string().contains("overlap"),
        "{error}"
    );
}
#[test]
fn excessive_tiny_tile_count_is_rejected() {
    let mut b = support::lossy_dng(false, false);
    for (tag, v) in [(256, 257), (257, 256), (322, 1), (323, 1)] {
        set(&mut b, tag, v);
    }
    array(&mut b, 324, &vec![0; 257 * 256]);
    array(&mut b, 325, &vec![1; 257 * 256]);
    let error = raw_decode::lossy_dng::read_metadata(&mut Cursor::new(&b)).unwrap_err();
    assert!(error.to_string().contains("tile count"), "{error}");
}
struct CountReads {
    inner: Cursor<Vec<u8>>,
    start: u64,
    payload_read: usize,
}
impl Read for CountReads {
    fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
        let p = self.inner.position();
        let n = self.inner.read(out)?;
        self.payload_read += (p + n as u64).saturating_sub(p.max(self.start)) as usize;
        Ok(n)
    }
}
impl Seek for CountReads {
    fn seek(&mut self, p: SeekFrom) -> io::Result<u64> {
        self.inner.seek(p)
    }
}
#[test]
fn each_compressed_tile_is_read_once() {
    let b = support::lossy_dng(false, false);
    let p = entry(&b, 324);
    let start = u32::from_le_bytes(b[p + 8..p + 12].try_into().unwrap()) as u64;
    let count = b.len() - start as usize;
    let mut input = CountReads {
        inner: Cursor::new(b),
        start,
        payload_read: 0,
    };
    assert!(raw_decode::lossy_dng::read(&mut input).unwrap().is_some());
    assert_eq!(input.payload_read, count);
}
fn marker_jpeg(transform: Option<u8>) -> Vec<u8> {
    let original = include_bytes!("fixtures/linear-gradient.jpg");
    // Remove every APP14; keep the JFIF marker for the no-Adobe case.
    let mut jpeg = original[..2].to_vec();
    let mut p = 2;
    while original[p + 1] != 0xda {
        let len = u16::from_be_bytes([original[p + 2], original[p + 3]]) as usize + 2;
        if original[p + 1] != 0xee {
            jpeg.extend_from_slice(&original[p..p + len]);
        }
        p += len;
    }
    jpeg.extend_from_slice(&original[p..]);
    if let Some(t) = transform {
        jpeg.splice(
            2..2,
            [
                255, 238, 0, 14, b'A', b'd', b'o', b'b', b'e', 0, 100, 0, 0, 0, 0, t,
            ],
        );
    }
    jpeg
}
#[test]
fn adobe_zero_one_and_jfif_decode_actual_pixels() {
    let mut outputs = Vec::new();
    for transform in [Some(0), Some(1), None] {
        let jpeg = marker_jpeg(transform);
        let b = support::lossy_dng_with_jpeg(false, false, &jpeg);
        let d = raw_decode::lossy_dng::read(&mut Cursor::new(b))
            .unwrap()
            .unwrap();
        // First cropped pixel: codes Y/R=48, Cb/G=69, Cr/B=84.
        // BT.601 YCbCr conversion -> RGB approximately (0,100,0).
        let expected = if transform == Some(0) {
            [48., 69., 84.]
        } else {
            [0., 100., 0.]
        };
        for (v, code) in d.pixels[0].iter().zip(expected) {
            assert!(
                (v - (code - 1.) / 254.).abs() < 3. / 254.,
                "transform={transform:?}, pixel={:?}",
                d.pixels[0]
            );
        }
        outputs.push(d.pixels);
    }
    assert_eq!(outputs[1], outputs[2]);
    assert_ne!(outputs[0], outputs[1]);
}
fn full_seed(jxl: bool) -> Vec<u8> {
    let tile: &[u8] = if jxl {
        include_bytes!("fixtures/linear-gradient-16.jxl")
    } else {
        include_bytes!("fixtures/linear-gradient.jpg")
    };
    let mut list = Vec::new();
    for v in [1u32, 8, 0x01030000, 0, 52, 0, 0, 16, 16, 0, 3, 1, 1, 1] {
        list.extend(v.to_be_bytes());
    }
    for v in [0f64, 1.] {
        list.extend(v.to_be_bytes());
    }
    let mut b = support::lossy_dng_with_opcodes(false, false, tile, &list);
    // Add ActiveArea and both remaining opcode lists without moving old payloads.
    let old_n = u16::from_le_bytes(b[38..40].try_into().unwrap()) as usize;
    let mut tags = b[40..40 + old_n * 12].to_vec();
    let active = b.len() as u32;
    b.extend([0u32, 0, 16, 16].into_iter().flat_map(u32::to_le_bytes));
    for (tag, kind, count, value) in [
        (50829u16, 4u16, 4u32, active),
        (51008, 7, 4, 0),
        (51022, 7, 4, 0),
    ] {
        tags.extend(tag.to_le_bytes());
        tags.extend(kind.to_le_bytes());
        tags.extend(count.to_le_bytes());
        tags.extend(value.to_le_bytes());
    }
    if jxl {
        set(&mut b, 259, 52546);
        let p = entry(&b, 258);
        let offset = u32::from_le_bytes(b[p + 8..p + 12].try_into().unwrap()) as usize;
        for c in 0..3 {
            b[offset + c * 2..offset + c * 2 + 2].copy_from_slice(&16u16.to_le_bytes());
        }
        let lut = b.len() as u32;
        b.extend((0..=65535u16).flat_map(u16::to_le_bytes));
        let p = entry(&b, 50712);
        b[p + 4..p + 8].copy_from_slice(&65536u32.to_le_bytes());
        set(&mut b, 50712, lut);
        tags[..old_n * 12].copy_from_slice(&b[40..40 + old_n * 12]);
    }
    // Repack the complete IFD before all payloads, so truncation exercises
    // post-identification validation rather than just cutting off an EOF IFD.
    let mut out = b[..38].to_vec();
    out[30..34].copy_from_slice(&38u32.to_le_bytes());
    out.extend(((old_n + 3) as u16).to_le_bytes());
    let payload_start = 40 + tags.len() + 4;
    let mut payload = Vec::new();
    let mut tile_slot = 0;
    for e in tags.as_chunks::<12>().0 {
        let tag = u16::from_le_bytes(e[..2].try_into().unwrap());
        let kind = u16::from_le_bytes(e[2..4].try_into().unwrap());
        let count = u32::from_le_bytes(e[4..8].try_into().unwrap()) as usize;
        let unit = match kind {
            3 => 2,
            4 => 4,
            5 | 10 => 8,
            _ => 1,
        };
        out.extend_from_slice(&e[..8]);
        if tag == 324 {
            tile_slot = out.len();
        }
        if count * unit <= 4 {
            out.extend_from_slice(&e[8..]);
        } else {
            let old = u32::from_le_bytes(e[8..].try_into().unwrap()) as usize;
            out.extend(((payload_start + payload.len()) as u32).to_le_bytes());
            payload.extend_from_slice(&b[old..old + count * unit]);
        }
    }
    out.extend(0u32.to_le_bytes());
    out.extend(payload);
    let offset = out.len() as u32;
    out[tile_slot..tile_slot + 4].copy_from_slice(&offset.to_le_bytes());
    out.extend(tile);
    out
}
#[test]
fn header_only_jxl_returns_error_without_render_panic() {
    let mut bits = Vec::new();
    for (v, n) in [
        (0xaffu32, 16),
        (0, 1),
        (0, 2),
        (15, 9),
        (1, 3),
        (1, 1),
        (1, 1),
    ] {
        bits.extend((0..n).map(|i| ((v >> i) & 1) as u8));
    }
    let payload: Vec<u8> = bits
        .chunks(8)
        .map(|c| c.iter().enumerate().fold(0, |v, (i, b)| v | (b << i)))
        .collect();
    let header = jxl_oxide::JxlImage::builder()
        .read(Cursor::new(&payload))
        .unwrap();
    assert_eq!((header.width(), header.height()), (16, 16));
    assert_eq!(header.num_loaded_keyframes(), 0);
    let mut b = support::lossy_dng_with_jpeg(false, false, &payload);
    set(&mut b, 259, 52546);
    let result = std::panic::catch_unwind(|| raw_decode::lossy_dng::read(&mut Cursor::new(b)));
    assert!(result.is_ok(), "header-only JXL panicked");
    assert_eq!(
        result.unwrap().err().unwrap().to_string(),
        "JXL frame missing"
    );
}
// Per-thread allocation accounting: cumulative bytes bounds peak as well. No
// samples or allocator metadata are retained after each case.
struct Meter;
thread_local! { static ALLOC: std::cell::Cell<Option<usize>> = const { std::cell::Cell::new(None) }; }
unsafe impl std::alloc::GlobalAlloc for Meter {
    unsafe fn alloc(&self, l: std::alloc::Layout) -> *mut u8 {
        let _ = ALLOC.try_with(|n| {
            if let Some(v) = n.get() {
                n.set(Some(v.saturating_add(l.size())))
            }
        });
        unsafe { std::alloc::System.alloc(l) }
    }
    unsafe fn dealloc(&self, p: *mut u8, l: std::alloc::Layout) {
        unsafe { std::alloc::System.dealloc(p, l) }
    }
    unsafe fn realloc(&self, p: *mut u8, l: std::alloc::Layout, n: usize) -> *mut u8 {
        let _ = ALLOC.try_with(|v| {
            if let Some(x) = v.get() {
                v.set(Some(x.saturating_add(n)))
            }
        });
        unsafe { std::alloc::System.realloc(p, l, n) }
    }
}
#[global_allocator]
static METER: Meter = Meter;
#[test]
fn seeded_full_jpeg_and_16bit_jxl_mutations_are_bounded() {
    let mut rng = 0x8f5eed1234567890u64;
    let mut max_bytes = 0;
    let mut max_time = std::time::Duration::ZERO;
    let mut slowest_case = (false, 0, false);
    for jxl in [false, true] {
        let seed = full_seed(jxl);
        assert!(
            raw_decode::lossy_dng::read(&mut Cursor::new(&seed))
                .unwrap()
                .is_some()
        );
        for case in 0..128 {
            rng ^= rng << 13;
            rng ^= rng >> 7;
            rng ^= rng << 17;
            let mut b = seed.clone();
            if case % 2 == 0 {
                let p = match case % 6 {
                    0 => rng as usize % 400.min(b.len()),
                    2 => b.len() - 1 - rng as usize % 128.min(b.len()),
                    _ => rng as usize % b.len(),
                };
                b[p] ^= 1 << ((rng >> 32) % 8);
            } else {
                b.truncate(rng as usize % b.len());
            }
            for metadata in [false, true] {
                ALLOC.with(|v| v.set(Some(0)));
                let start = std::time::Instant::now();
                let result = std::panic::catch_unwind(|| {
                    if metadata {
                        let _ = raw_decode::lossy_dng::read_metadata(&mut Cursor::new(&b));
                    } else {
                        let _ = raw_decode::lossy_dng::read(&mut Cursor::new(&b));
                    }
                });
                let elapsed = start.elapsed();
                let allocated = ALLOC.with(|v| v.replace(None).unwrap());
                max_bytes = max_bytes.max(allocated);
                if elapsed > max_time {
                    max_time = elapsed;
                    slowest_case = (jxl, case, metadata);
                }
                assert!(result.is_ok(), "jxl={jxl} case={case} metadata={metadata}");
                assert!(
                    allocated < 768 * 1024 * 1024,
                    "allocation={allocated} case={case}"
                );
                assert!(
                    elapsed < std::time::Duration::from_secs(2),
                    "elapsed={elapsed:?} case={case}"
                );
            }
        }
    }
    eprintln!(
        "mutation cases=512 max_allocated_bytes={max_bytes} max_time_us={} slowest_jxl_case_metadata={slowest_case:?}",
        max_time.as_micros()
    );
}

fn large_original(width: u32, height: u32) -> Vec<u8> {
    let tile = include_bytes!("fixtures/solid-512.jpg");
    let mut b = support::lossy_dng_with_jpeg(false, false, tile);
    for (tag, value) in [(256, width), (257, height), (322, 512), (323, 512)] {
        set(&mut b, tag, value);
    }
    array(&mut b, 50719, &[0, 0]);
    array(&mut b, 50720, &[width, height]);
    let count = width.div_ceil(512) * height.div_ceil(512);
    let mut offsets = Vec::new();
    for _ in 0..count {
        offsets.push(b.len() as u32);
        b.extend_from_slice(tile);
    }
    array(&mut b, 324, &offsets);
    array(&mut b, 325, &vec![tile.len() as u32; count as usize]);
    b
}

#[test]
fn lr8g_full_resolution_24mp_original_decodes() {
    let b = large_original(6144, 4096);
    let d = raw_decode::lossy_dng::read(&mut Cursor::new(b))
        .unwrap()
        .unwrap();
    assert_eq!(
        (d.width, d.height, d.pixels.len()),
        (6144, 4096, 25_165_824)
    );
    for pixel in &d.pixels {
        for (&v, code) in pixel.iter().zip([48., 69., 84.]) {
            assert!((v - (code - 1.) / 254.).abs() < 2. / 254.);
        }
    }
}

#[test]
fn lr8g_output_budget_admits_100mp_and_exact_cap_rejects_next_column() {
    for (width, height) in [(10000, 10000), (32768, 4096)] {
        let b = large_original(width, height);
        let m = raw_decode::lossy_dng::read_metadata(&mut Cursor::new(b))
            .unwrap()
            .unwrap();
        assert_eq!((m.width, m.height), (width, height));
    }
    let b = large_original(32769, 4096);
    for metadata in [false, true] {
        let error = if metadata {
            raw_decode::lossy_dng::read_metadata(&mut Cursor::new(&b))
                .err()
                .unwrap()
        } else {
            raw_decode::lossy_dng::read(&mut Cursor::new(&b))
                .err()
                .unwrap()
        };
        assert_eq!(error.to_string(), "total decoded byte budget exceeded");
    }
}

#[test]
fn lr8g_ycck_and_contradictory_adobe_markers_fail_closed() {
    for transforms in [&[2][..], &[0, 1], &[1, 0], &[0, 2], &[2, 0]] {
        let mut jpeg = marker_jpeg(None);
        for &t in transforms {
            jpeg.splice(
                2..2,
                [
                    255, 238, 0, 14, b'A', b'd', b'o', b'b', b'e', 0, 100, 0, 0, 0, 0, t,
                ],
            );
        }
        let b = support::lossy_dng_with_jpeg(false, false, &jpeg);
        assert!(
            raw_decode::lossy_dng::read(&mut Cursor::new(b)).is_err(),
            "transforms={transforms:?}"
        );
    }
}

#[test]
fn lr8g_multi_frame_jxl_is_rejected() {
    let tile = include_bytes!("fixtures/two-frame.jxl");
    let image = jxl_oxide::JxlImage::builder()
        .read(Cursor::new(tile))
        .unwrap();
    assert_eq!(image.num_loaded_keyframes(), 2);
    let mut b = support::lossy_dng_with_jpeg(false, false, tile);
    set(&mut b, 259, 52546);
    let error = raw_decode::lossy_dng::read(&mut Cursor::new(b))
        .err()
        .expect("multi-frame tile rejected");
    assert!(error.to_string().contains("multi-frame JXL"), "{error}");
}

/// LR-8h: a file whose bytes are never materialised. `head` is real; every
/// `stride` bytes from `first` a copy of `tile` begins; all else reads as zero.
struct Sparse {
    head: Vec<u8>,
    tile: &'static [u8],
    first: u64,
    stride: u64,
    size: u64,
    position: u64,
}
impl Read for Sparse {
    fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
        let n = (out.len() as u64).min(self.size.saturating_sub(self.position)) as usize;
        let (start, end) = (self.position, self.position + n as u64);
        out[..n].fill(0);
        let mut copy = |at: u64, bytes: &[u8]| {
            let (a, b) = (at.max(start), (at + bytes.len() as u64).min(end));
            if a < b {
                out[(a - start) as usize..(b - start) as usize]
                    .copy_from_slice(&bytes[(a - at) as usize..(b - at) as usize]);
            }
        };
        copy(0, &self.head);
        if !self.tile.is_empty() && end > self.first {
            let mut at = self.first + start.saturating_sub(self.first) / self.stride * self.stride;
            while at < end {
                copy(at, self.tile);
                at += self.stride;
            }
        }
        self.position = end;
        Ok(n)
    }
}
impl Seek for Sparse {
    fn seek(&mut self, p: SeekFrom) -> io::Result<u64> {
        self.position = match p {
            SeekFrom::Start(v) => v,
            SeekFrom::End(v) => self.size.checked_add_signed(v).unwrap(),
            SeekFrom::Current(v) => self.position.checked_add_signed(v).unwrap(),
        };
        Ok(self.position)
    }
}
const MIB: u32 = 1024 * 1024;
/// Identified LinearRaw of 512-pixel tiles, `count` compressed bytes each,
/// placed `stride` bytes apart, in a sparse file of `payload` bytes after the
/// TIFF structures. Nothing beyond the IFD and tile tables is allocated.
fn sparse_original(side: u32, count: u32, stride: u32, payload: u64) -> Sparse {
    let tile = include_bytes!("fixtures/solid-512.jpg");
    let mut head = support::lossy_dng_with_jpeg(false, false, &[]);
    for (tag, value) in [(256, side), (257, side), (322, 512), (323, 512)] {
        set(&mut head, tag, value);
    }
    array(&mut head, 50719, &[0, 0]);
    array(&mut head, 50720, &[side, side]);
    let tiles = side.div_ceil(512).pow(2);
    let first = head.len() as u32 + 8 * tiles;
    let offsets: Vec<u32> = (0..tiles).map(|i| first + i * stride).collect();
    array(&mut head, 324, &offsets);
    array(&mut head, 325, &vec![count; tiles as usize]);
    assert_eq!(head.len() as u32, first);
    Sparse {
        head,
        tile,
        first: first as u64,
        stride: stride.max(1) as u64,
        size: first as u64 + payload,
        position: 0,
    }
}
fn compressed_budget_error(mut file: Sparse) {
    for metadata in [true, false] {
        let error = if metadata {
            raw_decode::lossy_dng::read_metadata(&mut file).err()
        } else {
            raw_decode::lossy_dng::read(&mut file).err()
        };
        assert_eq!(
            error.expect("rejected").to_string(),
            "total compressed byte budget exceeded"
        );
    }
}

#[test]
fn lr8h_compressed_total_above_128mib_is_not_refused_alone() {
    // Admission: a 100 MP original with 400 tiles of 512 KiB = 200 MiB.
    let count = MIB / 2;
    let mut file = sparse_original(10000, count, count, 400 * count as u64);
    let m = raw_decode::lossy_dng::read_metadata(&mut file)
        .unwrap()
        .unwrap();
    assert_eq!((m.width, m.height), (10000, 10000));
    // The exact cap, 64 tiles of 24 MiB = 1.5 GiB of compressed bytes.
    let count = 24 * MIB;
    let mut file = sparse_original(4096, count, count, 64 * count as u64);
    assert!(
        raw_decode::lossy_dng::read_metadata(&mut file)
            .unwrap()
            .is_some()
    );
    // Complete decode: 16 tiles of 9 MiB = 144 MiB, each a real JPEG followed
    // by zero padding inside its declared byte count.
    let count = 9 * MIB;
    let mut file = sparse_original(2048, count, count, 16 * count as u64);
    let d = raw_decode::lossy_dng::read(&mut file).unwrap().unwrap();
    assert_eq!((d.width, d.height, d.pixels.len()), (2048, 2048, 4_194_304));
    for pixel in &d.pixels {
        for (&v, code) in pixel.iter().zip([48., 69., 84.]) {
            assert!((v - (code - 1.) / 254.).abs() < 2. / 254.);
        }
    }
}

#[test]
fn lr8h_compressed_total_above_decoded_budget_is_rejected() {
    // 64 tiles of 24 MiB + 1 byte: just above 1.5 GiB, inside a large enough file.
    let count = 24 * MIB + 1;
    compressed_budget_error(sparse_original(4096, count, count, 64 * count as u64));
}

#[test]
fn lr8h_compressed_total_above_file_size_and_aliased_ranges_are_rejected() {
    let count = MIB / 2;
    // 200 MiB declared in a file one byte too short to hold it.
    compressed_budget_error(sparse_original(10000, count, count, 400 * count as u64 - 1));
    // Every tile aliases the same 512 KiB range; each range is inside the file.
    compressed_budget_error(sparse_original(10000, count, 0, 100 * count as u64));
    // Overlapping ranges half a tile apart; each range is inside the file.
    compressed_budget_error(sparse_original(10000, count, count / 2, 201 * count as u64));
}
