//! Lightroom lossy LinearRaw DNG. JPEG components are camera channels, not display RGB.
//! Bounded classic TIFF reader; no LibRaw pixel unpack or native JPEG dependency.
use crate::{CfaLayout, RawMetadata};
use engine_api::color::ColorMatrix3;
use std::{
    collections::{BTreeMap, BTreeSet},
    io::{self, Read, Seek, SeekFrom},
};
use zune_jpeg::{
    JpegDecoder,
    zune_core::{bytestream::ZCursor, colorspace::ColorSpace, options::DecoderOptions},
};

/// Cap the actual camera RGB output (three f32 channels) at 1.5 GiB, or
/// 134,217,728 pixels, admitting 100 MP originals with headroom. This is an
/// output allocation cap, not a total-process cap: the final crop can coexist
/// with the full output (up to another 1.5 GiB); codec scratch retains its
/// independent limits. Tiles decode sequentially, so summing padded tile
/// working sets would unnecessarily reject large originals.
///
/// The same figure bounds the aggregate compressed payload, together with the
/// file size: lossy tiles cannot sensibly exceed the f32 output they decode
/// to, so a large original is never refused on compressed size alone. The
/// pixel reader keeps the compressed tiles resident while decoding, so they
/// can add up to this many bytes again (at most the file size).
const MAX_DECODED_BYTES: usize = 1536 * 1024 * 1024;

type Tags = BTreeMap<u16, Tag>;
#[derive(Clone)]
struct Tag {
    kind: u16,
    bytes: Vec<u8>,
}
struct Tiff<'a, R> {
    input: &'a mut R,
    size: u64,
    le: bool,
    budget: usize,
}
fn invalid(s: impl ToString) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, s.to_string())
}
impl<R: Read + Seek> Tiff<'_, R> {
    fn u16(&self, b: &[u8]) -> u16 {
        if self.le {
            u16::from_le_bytes(b[..2].try_into().unwrap())
        } else {
            u16::from_be_bytes(b[..2].try_into().unwrap())
        }
    }
    fn u32(&self, b: &[u8]) -> u32 {
        if self.le {
            u32::from_le_bytes(b[..4].try_into().unwrap())
        } else {
            u32::from_be_bytes(b[..4].try_into().unwrap())
        }
    }
    fn at(&mut self, offset: u64, n: usize) -> io::Result<Vec<u8>> {
        if offset.checked_add(n as u64).is_none_or(|v| v > self.size) {
            return Err(invalid("TIFF range outside file"));
        }
        self.input.seek(SeekFrom::Start(offset))?;
        let mut bytes = vec![0; n];
        self.input.read_exact(&mut bytes)?;
        Ok(bytes)
    }
    fn ifd(&mut self, offset: u32, identified: &mut bool) -> io::Result<(Tags, u32)> {
        let count = self.at(offset as u64, 2)?;
        let n = self.u16(&count) as usize;
        if n > 4096 {
            return Err(invalid("IFD entry budget exceeded"));
        }
        let entries = self.at(offset as u64 + 2, n * 12 + 4)?;
        // Identify the selected image using only inline, well-typed fields. Do this
        // before parsing other fields: their order must not affect fail-closed admission.
        let entries_only = entries[..n * 12].as_chunks::<12>().0;
        let full_resolution = entries_only
            .iter()
            .filter(|e| self.u16(*e) == 254)
            .all(|e| self.u16(&e[2..]) == 4 && self.u32(&e[4..]) == 1 && self.u32(&e[8..]) == 0);
        let inline_short = |id, values: &[u16]| {
            entries_only.iter().any(|e| {
                self.u16(e) == id
                    && self.u16(&e[2..]) == 3
                    && self.u32(&e[4..]) == 1
                    && values.contains(&self.u16(&e[8..]))
            })
        };
        if full_resolution
            && inline_short(259, &[34892, 52546])
            && inline_short(277, &[3])
            && entries_only.iter().any(|e| {
                self.u16(e) == 262
                    && self.u16(&e[2..]) == 3
                    && self.u32(&e[4..]) == 1
                    && self.u16(&e[8..]) == 34892
            })
        {
            *identified = true;
        }
        let mut tags = Tags::new();
        for e in entries[..n * 12].as_chunks::<12>().0 {
            let id = self.u16(e);
            let kind = self.u16(&e[2..]);
            let unit = match kind {
                1 | 2 | 6 | 7 => 1,
                3 | 8 => 2,
                4 | 9 | 11 | 13 => 4,
                5 | 10 | 12 => 8,
                _ => return Err(invalid("unsupported TIFF field type")),
            };
            let bytes = (self.u32(&e[4..]) as usize)
                .checked_mul(unit)
                .ok_or_else(|| invalid("tag overflow"))?;
            // Check even unretained fields structurally without allocating their data.
            if bytes > 4
                && (self.u32(&e[8..]) as u64)
                    .checked_add(bytes as u64)
                    .is_none_or(|end| end > self.size)
            {
                return Err(invalid("TIFF field range outside file"));
            }
            // Only pixel-layout and calibration tags are retained. In particular,
            // embedded originals, maker notes, XMP and profile tables are never allocated.
            if !matches!(id,254|256..=259|262|271..=274|277..=279|284|322..=325|330|339|50712..=50717|50719..=50730|50778|50779|50829|50964|50965|51008|51009|51022)
            {
                continue;
            }
            self.budget = self
                .budget
                .checked_add(bytes)
                .ok_or_else(|| invalid("tag overflow"))?;
            if self.budget > 8 * 1024 * 1024 {
                return Err(invalid("metadata budget exceeded"));
            }
            let bytes = if bytes <= 4 {
                e[8..8 + bytes].to_vec()
            } else {
                self.at(self.u32(&e[8..]) as u64, bytes)?
            };
            if tags.insert(id, Tag { kind, bytes }).is_some() {
                return Err(invalid("duplicate TIFF tag"));
            }
        }
        Ok((tags, self.u32(&entries[n * 12..])))
    }
    fn numbers(&self, tags: &Tags, id: u16) -> io::Result<Vec<f64>> {
        let Some(t) = tags.get(&id) else {
            return Ok(Vec::new());
        };
        let unit = match t.kind {
            1 => 1,
            3 => 2,
            4 | 9 | 11 | 13 => 4,
            12 => 8,
            5 | 10 => 8,
            _ => return Err(invalid("non-numeric TIFF field")),
        };
        t.bytes
            .chunks_exact(unit)
            .map(|b| {
                let value = match t.kind {
                    1 => b[0] as f64,
                    3 => self.u16(b) as f64,
                    4 | 13 => self.u32(b) as f64,
                    9 => (self.u32(b) as i32) as f64,
                    11 => f32::from_bits(self.u32(b)) as f64,
                    12 => f64::from_bits(if self.le {
                        u64::from_le_bytes(b.try_into().unwrap())
                    } else {
                        u64::from_be_bytes(b.try_into().unwrap())
                    }),
                    5 | 10 => {
                        let (a, b) = if t.kind == 10 {
                            (
                                (self.u32(b) as i32) as f64,
                                (self.u32(&b[4..]) as i32) as f64,
                            )
                        } else {
                            (self.u32(b) as f64, self.u32(&b[4..]) as f64)
                        };
                        if b == 0. {
                            return Err(invalid("zero rational denominator"));
                        }
                        a / b
                    }
                    _ => unreachable!(),
                };
                if !value.is_finite() {
                    return Err(invalid("nonfinite TIFF number"));
                }
                Ok(value)
            })
            .collect()
    }
    fn scalar(&self, t: &Tags, id: u16, default: f64) -> io::Result<f64> {
        let v = self.numbers(t, id)?;
        if v.len() > 1 {
            return Err(invalid("scalar tag has multiple values"));
        }
        Ok(v.first().copied().unwrap_or(default))
    }
    fn ints(&self, t: &Tags, id: u16) -> io::Result<Vec<usize>> {
        self.numbers(t, id)?
            .into_iter()
            .map(|v| {
                if v < 0. || v > u32::MAX as f64 || v.fract() != 0. {
                    Err(invalid("integer tag required"))
                } else {
                    Ok(v as usize)
                }
            })
            .collect()
    }
}

/// Cropped, black-subtracted and white-normalized camera RGB; orientation is
/// retained in metadata for the source boundary to consume exactly once.
pub struct LossyDng {
    pub width: usize,
    pub height: usize,
    pub pixels: Vec<[f32; 3]>,
    pub metadata: RawMetadata,
    pub color_matrices: [Option<[[f64; 3]; 3]>; 2],
    pub forward_matrices: [Option<[[f64; 3]; 3]>; 2],
    pub calibration_illuminants: [u16; 2],
    pub baseline_exposure: f32,
}

thread_local! {
    static PIXEL_DECODES: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
}

/// Pixel decode boundary entries on this thread, for performance diagnostics.
pub fn pixel_decode_count() -> u64 {
    PIXEL_DECODES.get()
}

/// Returns None (including on parse errors) until the selected full-resolution
/// IFD identifies three-channel LinearRaw with classic lossy JPEG or JXL.
/// All other DNGs, including other LinearRaw layouts, stay on the LibRaw path.
/// Malformed identified LinearRaw containers fail closed.
pub fn read<R: Read + Seek>(input: &mut R) -> io::Result<Option<LossyDng>> {
    PIXEL_DECODES.set(PIXEL_DECODES.get() + 1);
    read_impl(input, true)
}

/// Bounded header projection for indexing. This never reads compressed tiles
/// and is not a promise that their pixel payload can be decoded.
pub fn read_metadata<R: Read + Seek>(input: &mut R) -> io::Result<Option<RawMetadata>> {
    Ok(read_impl(input, false)?.map(|dng| dng.metadata))
}

fn read_impl<R: Read + Seek>(input: &mut R, decode_pixels: bool) -> io::Result<Option<LossyDng>> {
    let mut identified = false;
    match read_identified(input, decode_pixels, &mut identified) {
        Err(_) if !identified => Ok(None),
        result => result,
    }
}

fn read_identified<R: Read + Seek>(
    input: &mut R,
    decode_pixels: bool,
    identified: &mut bool,
) -> io::Result<Option<LossyDng>> {
    let size = input.seek(SeekFrom::End(0))?;
    if size < 8 {
        return Err(invalid("truncated TIFF header"));
    }
    let mut t = Tiff {
        input,
        size,
        le: true,
        budget: 0,
    };
    let header = t.at(0, 8)?;
    t.le = match &header[..2] {
        b"II" => true,
        b"MM" => false,
        _ => return Ok(None),
    };
    if t.u16(&header[2..]) != 42 {
        return Ok(None);
    }
    let mut pending = vec![t.u32(&header[4..])];
    let mut visited = BTreeSet::new();
    let mut root = None;
    let mut selected = None;
    while let Some(offset) = pending.pop() {
        if offset == 0 {
            continue;
        }
        if !visited.insert(offset) || visited.len() > 64 {
            return Err(invalid("cyclic or excessive TIFF IFD graph"));
        }
        let (tags, next) = t.ifd(offset, identified)?;
        pending.push(next);
        for child in t.ints(&tags, 330)? {
            pending.push(child as u32);
        }
        if t.scalar(&tags, 254, 0.)? == 0.
            && t.scalar(&tags, 262, 0.)? == 34892.
            && matches!(t.scalar(&tags, 259, 0.)?, 34892. | 52546.)
            && t.scalar(&tags, 277, 0.)? == 3.
        {
            if selected.is_some() {
                return Err(invalid("ambiguous full-resolution LinearRaw IFD"));
            }
            selected = Some(tags.clone());
        }
        if root.is_none() {
            root = Some(tags);
        }
    }
    let Some(mut tags) = selected else {
        return Ok(None);
    };
    // DNG calibration and orientation commonly live in IFD0, image geometry in SubIFD.
    for (id, tag) in root.unwrap_or_default() {
        if matches!(
            id,
            271 | 272
                | 274
                | 50721
                | 50722
                | 50723
                | 50724
                | 50727
                | 50728
                | 50729
                | 50730
                | 50778
                | 50779
                | 50964
                | 50965
                | 51008
                | 51009
                | 51022
        ) {
            tags.entry(id).or_insert(tag);
        }
    }
    // Validate all retained numeric fields, including optional calibration fields,
    // before any pixel allocation or compressed payload read.
    for (&id, tag) in &tags {
        match id {
            271 | 272 if tag.kind == 2 => {}
            51008 | 51009 | 51022 if matches!(tag.kind, 1 | 7) => {}
            271 | 272 | 51008 | 51009 | 51022 => return Err(invalid("invalid TIFF field type")),
            _ => {
                t.numbers(&tags, id)?;
            }
        }
    }
    let dimension = |id| -> io::Result<usize> {
        let v = t.ints(&tags, id)?;
        if v.len() != 1 || v[0] == 0 || v[0] > 65535 {
            return Err(invalid("invalid image dimension"));
        }
        Ok(v[0])
    };
    let width = dimension(256)?;
    let height = dimension(257)?;
    if width
        .checked_mul(height)
        .and_then(|n| n.checked_mul(size_of::<[f32; 3]>()))
        .is_none_or(|bytes| bytes > MAX_DECODED_BYTES)
    {
        return Err(invalid("total decoded byte budget exceeded"));
    }
    if t.scalar(&tags, 284, 1.)? != 1. {
        return Err(invalid("planar JPEG unsupported"));
    }
    let jxl = t.scalar(&tags, 259, 0.)? == 52546.;
    let bits = t.ints(&tags, 258)?;
    if !matches!(bits.len(), 1 | 3) || bits.iter().any(|&v| v != bits[0]) {
        return Err(invalid("uniform BitsPerSample required"));
    }
    let bits = bits[0];
    let formats = t.ints(&tags, 339)?;
    let format = formats.first().copied().unwrap_or(1);
    if !matches!(formats.len(), 0 | 1 | 3)
        || formats.iter().any(|&v| v != format)
        || !(format == 1 && (1..=16).contains(&bits) || format == 3 && matches!(bits, 16 | 32))
        || (!jxl && (bits != 8 || format != 1))
    {
        return Err(invalid("unsupported BitsPerSample/SampleFormat"));
    }
    let max_code = if format == 3 {
        1.
    } else {
        ((1u64 << bits) - 1) as f64
    };
    if tags.contains_key(&50715) || tags.contains_key(&50716) {
        return Err(invalid("BlackLevelDelta unsupported"));
    }
    let repeat = t.ints(&tags, 50713)?;
    if !repeat.is_empty() && repeat != [1, 1] {
        return Err(invalid("spatial BlackLevelRepeatDim unsupported"));
    }
    let black = t.numbers(&tags, 50714)?;
    let white = t.numbers(&tags, 50717)?;
    if !matches!(black.len(), 0 | 1 | 3) || !matches!(white.len(), 0 | 1 | 3) {
        return Err(invalid("spatial black/white levels unsupported"));
    }
    let levels = |v: &[f64], default: f64| -> [f64; 3] {
        std::array::from_fn(|c| v.get(c).or(v.first()).copied().unwrap_or(default))
    };
    let black = levels(&black, 0.);
    let white = levels(&white, max_code);
    if (0..3).any(|c| white[c] <= black[c]) {
        return Err(invalid("invalid black/white interval"));
    }
    let lut = t.numbers(&tags, 50712)?;
    if tags.contains_key(&50712) && (format != 1 || lut.len() != 1usize << bits) {
        return Err(invalid("complete integer linearization table required"));
    }
    let (tw, th, offsets, counts) = if tags.contains_key(&324) {
        (
            dimension(322)?,
            dimension(323)?,
            t.ints(&tags, 324)?,
            t.ints(&tags, 325)?,
        )
    } else {
        (
            width,
            dimension(278)?,
            t.ints(&tags, 273)?,
            t.ints(&tags, 279)?,
        )
    };
    // TIFF tiles may pad the right/bottom edge to the next 16-pixel boundary.
    // Strips have no horizontal padding and at most one image's worth of rows.
    let (max_tw, max_th) = if tags.contains_key(&324) {
        (width.next_multiple_of(16), height.next_multiple_of(16))
    } else {
        (width, height)
    };
    if tw > max_tw || th > max_th {
        return Err(invalid("tile dimensions exceed image padding bounds"));
    }
    let across = width.div_ceil(tw);
    let down = height.div_ceil(th);
    let tile_count = across
        .checked_mul(down)
        .ok_or_else(|| invalid("tile count overflow"))?;
    if tile_count > 65536 {
        return Err(invalid("tile count budget exceeded"));
    }
    if offsets.len() != tile_count || offsets.len() != counts.len() {
        return Err(invalid("invalid tile/strip count"));
    }
    let compressed_total = counts
        .iter()
        .try_fold(0u64, |sum, &n| sum.checked_add(n as u64))
        .ok_or_else(|| invalid("compressed byte budget overflow"))?;
    // The file-size term bounds alias amplification: tiles sharing or
    // overlapping byte ranges cannot declare more bytes than the file holds.
    if compressed_total > size.min(MAX_DECODED_BYTES as u64) {
        return Err(invalid("total compressed byte budget exceeded"));
    }
    for (&offset, &count) in offsets.iter().zip(&counts) {
        if count == 0 || count > 32 * 1024 * 1024 {
            return Err(invalid("compressed tile budget exceeded"));
        }
        if (offset as u64)
            .checked_add(count as u64)
            .is_none_or(|end| end > size)
        {
            return Err(invalid("TIFF tile range outside file"));
        }
    }
    let mut polynomials = Vec::new();
    for id in [51008, 51009, 51022] {
        let (ops, retained) = polynomial_list(tags.get(&id).map(|t| t.bytes.as_slice()))?;
        polynomials.push(ops);
        if !retained {
            tags.remove(&id);
        }
    }
    let origin = t.ints(&tags, 50719)?;
    let crop = t.ints(&tags, 50720)?;
    let active = t.ints(&tags, 50829)?;
    if !origin.is_empty() && origin.len() != 2
        || !crop.is_empty() && crop.len() != 2
        || !active.is_empty() && active.len() != 4
    {
        return Err(invalid("invalid crop tags"));
    }
    let area = if active.is_empty() {
        [0, 0, height, width]
    } else {
        [active[0], active[1], active[2], active[3]]
    };
    if area[0] >= area[2] || area[1] >= area[3] || area[2] > height || area[3] > width {
        return Err(invalid("invalid ActiveArea"));
    }
    let left = origin.first().copied().unwrap_or(0) + area[1];
    let top = origin.get(1).copied().unwrap_or(0) + area[0];
    let cw = crop.first().copied().unwrap_or(area[3] - area[1]);
    let ch = crop.get(1).copied().unwrap_or(area[2] - area[0]);
    if cw == 0 || ch == 0 || left + cw > area[3] || top + ch > area[2] {
        return Err(invalid("crop outside image"));
    }
    let matrix = |id| -> io::Result<Option<[[f64; 3]; 3]>> {
        let v = t.numbers(&tags, id)?;
        if v.is_empty() {
            return Ok(None);
        }
        if v.len() != 9 {
            return Err(invalid("3x3 matrix required"));
        }
        Ok(Some(std::array::from_fn(|r| {
            std::array::from_fn(|c| v[r * 3 + c])
        })))
    };
    let color_matrices = [matrix(50721)?, matrix(50722)?];
    let forward_matrices = [matrix(50964)?, matrix(50965)?];
    let short_scalar = |id, default| -> io::Result<u16> {
        let values = t.ints(&tags, id)?;
        if values.len() > 1 || values.first().is_some_and(|&v| v > u16::MAX as usize) {
            return Err(invalid("invalid SHORT scalar"));
        }
        Ok(values.first().copied().unwrap_or(default) as u16)
    };
    let calibration_illuminants = [short_scalar(50778, 0)?, short_scalar(50779, 0)?];
    let cm = if calibration_illuminants[1] == 21 {
        color_matrices[1].or(color_matrices[0])
    } else {
        color_matrices[0].or(color_matrices[1])
    }
    .ok_or_else(|| invalid("missing ColorMatrix"))?;
    let inverse = ColorMatrix3(cm).inverse().map_err(invalid)?;
    let mut neutral = t.numbers(&tags, 50728)?;
    let xy = t.numbers(&tags, 50729)?;
    if !neutral.is_empty() && !xy.is_empty() {
        return Err(invalid(
            "AsShotNeutral and AsShotWhiteXY are mutually exclusive",
        ));
    }
    if neutral.is_empty() && !xy.is_empty() {
        if xy.len() != 2
            || xy.iter().any(|v| !v.is_finite())
            || xy[0] <= 0.
            || xy[1] <= 0.
            || xy[0] + xy[1] >= 1.
        {
            return Err(invalid("invalid AsShotWhiteXY"));
        }
        // DNG chapter 6: CameraNeutral = AB * CC * CM * XYZ (Y=1).
        // This decoder's profile is the selected ColorMatrix, so only identity
        // AB/CC is admitted here until those extra calibration stages exist.
        for (id, identity) in [
            (50727, vec![1.; 3]),
            (50723, vec![1., 0., 0., 0., 1., 0., 0., 0., 1.]),
            (50724, vec![1., 0., 0., 0., 1., 0., 0., 0., 1.]),
        ] {
            let values = t.numbers(&tags, id)?;
            if !values.is_empty() && values != identity {
                return Err(invalid(
                    "AsShotWhiteXY with nonidentity camera calibration is unsupported",
                ));
            }
        }
        let xyz = [xy[0] / xy[1], 1., (1. - xy[0] - xy[1]) / xy[1]];
        neutral = cm
            .iter()
            .map(|row| row.iter().zip(xyz).map(|(m, v)| m * v).sum())
            .collect();
    }
    if neutral.len() != 3 || neutral.iter().any(|v| !v.is_finite() || *v <= 0.) {
        return Err(invalid(
            "positive AsShotNeutral or valid AsShotWhiteXY required",
        ));
    }
    let wb = [
        (neutral[1] / neutral[0]) as f32,
        1.,
        (neutral[1] / neutral[2]) as f32,
        1.,
    ];
    let orientation = short_scalar(274, 1)?;
    if !(1..=8).contains(&orientation) {
        return Err(invalid("invalid orientation"));
    }
    let text = |id| {
        tags.get(&id)
            .map(|v| {
                String::from_utf8_lossy(&v.bytes)
                    .trim_end_matches('\0')
                    .to_string()
            })
            .unwrap_or_default()
    };
    let opcode_lists = [51008, 51009, 51022].map(|id| tags.get(&id).map(|t| t.bytes.clone()));
    let metadata = RawMetadata {
        make: text(271),
        model: text(272),
        lens: None,
        iso: 0.,
        shutter_s: 0.,
        aperture: 0.,
        focal_mm: 0.,
        capture_time: 0,
        catalog_orientation: None,
        baseline_exposure: t.scalar(&tags, 50730, 0.)? as f32,
        orientation,
        width: cw as u32,
        height: ch as u32,
        cfa_layout: CfaLayout::Unsupported,
        black_levels: [0.; 4],
        white_level: 1,
        as_shot_wb: wb,
        camera_to_xyz: inverse,
        cam_xyz: std::array::from_fn(|r| {
            if r < 3 {
                cm[r].map(|v| v as f32)
            } else {
                [0.; 3]
            }
        }),
        rgb_cam: [[0.; 4]; 3],
        default_crop: [0, 0, cw as u32, ch as u32],
        has_gain_map: false,
        has_opcode_list: opcode_lists.iter().any(Option::is_some),
        opcode_lists,
    };
    let mut tiles = Vec::new();
    if decode_pixels {
        // Validate every codec header against the bounded IFD geometry before
        // allocating the full image. Metadata-only indexing never reads tiles.
        for (i, (&offset, &count)) in offsets.iter().zip(&counts).enumerate() {
            let bytes = t.at(offset as u64, count)?;
            let (dw, dh, _) = if jxl {
                decode_jxl(&bytes, tw, th, bits, format, false)?
            } else {
                decode_jpeg(&bytes, tw, th, false)?
            };
            let (ox, oy) = ((i % across) * tw, (i / across) * th);
            if dw < tw.min(width - ox) || dh < th.min(height - oy) || dw > tw || dh > th {
                return Err(invalid("codec dimensions disagree with TIFF"));
            }
            tiles.push(bytes);
        }
    }
    let mut pixels = if decode_pixels {
        vec![[0.; 3]; width * height]
    } else {
        Vec::new()
    };
    if decode_pixels {
        for (i, bytes) in tiles.into_iter().enumerate() {
            let (dw, dh, decoded) = if jxl {
                decode_jxl(&bytes, tw, th, bits, format, true)?
            } else {
                decode_jpeg(&bytes, tw, th, true)?
            };
            let (ox, oy) = ((i % across) * tw, (i / across) * th);
            let (cw, ch) = (tw.min(width - ox), th.min(height - oy));
            if dw < cw || dh < ch || dw > tw || dh > th || decoded.len() != dw * dh * 3 {
                return Err(invalid("JPEG dimensions disagree with TIFF"));
            }
            for y in 0..ch {
                for x in 0..cw {
                    for c in 0..3 {
                        let mut code = decoded.code((y * dw + x) * 3 + c, max_code);
                        for op in &polynomials[0] {
                            code = op.map(code, ox + x, oy + y, c, max_code);
                        }
                        if !code.is_finite() {
                            return Err(invalid("nonfinite camera sample"));
                        }
                        let linear = if lut.is_empty() {
                            code
                        } else {
                            lut[code.round().clamp(0., (lut.len() - 1) as f64) as usize]
                        };
                        pixels[(oy + y) * width + ox + x][c] =
                            ((linear - black[c]) / (white[c] - black[c])) as f32;
                    }
                }
            }
        }
        for ops in &polynomials[1..] {
            for (i, pixel) in pixels.iter_mut().enumerate() {
                for (c, v) in pixel.iter_mut().enumerate() {
                    for op in ops {
                        *v = op.map(f64::from(*v), i % width, i / width, c, 1.) as f32;
                    }
                }
            }
        }
    }
    let pixels = if decode_pixels {
        (top..top + ch)
            .flat_map(|y| {
                pixels[y * width + left..y * width + left + cw]
                    .iter()
                    .copied()
            })
            .collect()
    } else {
        Vec::new()
    };
    Ok(Some(LossyDng {
        width: cw,
        height: ch,
        pixels,
        metadata,
        color_matrices,
        forward_matrices,
        calibration_illuminants,
        baseline_exposure: t.scalar(&tags, 50730, 0.)? as f32,
    }))
}

// Adobe APP14 transform=0 is labelled CMYK by zune even for three components.
// DNG's LinearRaw tag is authoritative: omit APP14 from this private JPEG copy,
// retain SOF/SOS component IDs, then ask zune for its *input* colorspace unchanged.
fn without_adobe(bytes: &[u8]) -> io::Result<Vec<u8>> {
    if !bytes.starts_with(&[255, 216]) {
        return Err(invalid("JPEG SOI required"));
    }
    let mut out = bytes[..2].to_vec();
    let mut p = 2;
    let mut adobe_transform = None;
    while p < bytes.len() {
        let start = p;
        if bytes[p] != 255 {
            return Err(invalid("invalid JPEG marker"));
        }
        while p < bytes.len() && bytes[p] == 255 {
            p += 1;
        }
        let marker = *bytes
            .get(p)
            .ok_or_else(|| invalid("truncated JPEG marker"))?;
        p += 1;
        if marker == 218 || marker == 217 {
            out.extend_from_slice(&bytes[start..]);
            return Ok(out);
        }
        let len = bytes
            .get(p..p + 2)
            .ok_or_else(|| invalid("truncated JPEG segment"))?;
        let len = u16::from_be_bytes(len.try_into().unwrap()) as usize;
        if len < 2 || p + len > bytes.len() {
            return Err(invalid("invalid JPEG segment length"));
        }
        let adobe = marker == 238 && bytes[p + 2..p + len].starts_with(b"Adobe");
        if adobe {
            if len != 14 {
                return Err(invalid("invalid Adobe marker"));
            }
            let transform = bytes[p + 13];
            if adobe_transform.is_some_and(|prior| prior != transform) {
                return Err(invalid("contradictory Adobe markers"));
            }
            if transform > 1 {
                return Err(invalid("unsupported Adobe transform (YCCK or unknown)"));
            }
            adobe_transform = Some(transform);
        }
        // Only transform 0 authorizes camera-component interleaving.
        let camera_marker = adobe && adobe_transform == Some(0);
        if !camera_marker {
            out.extend_from_slice(&bytes[start..p + len]);
        }
        p += len;
    }
    Err(invalid("JPEG scan missing"))
}

// Preserve native codec storage and promote only the sample being normalized.
// In particular, multiplying JXL in f64 here preserves pre-hotfix pixel rounding.
enum TileSamples {
    Jpeg(Vec<u8>),
    Jxl(jxl_oxide::FrameBuffer),
}
impl TileSamples {
    fn len(&self) -> usize {
        match self {
            Self::Jpeg(v) => v.len(),
            Self::Jxl(v) => v.buf().len(),
        }
    }
    fn code(&self, index: usize, max_code: f64) -> f64 {
        match self {
            Self::Jpeg(v) => f64::from(v[index]),
            Self::Jxl(v) => f64::from(v.buf()[index]) * max_code,
        }
    }
}

/// Same-encoding f32 output: integer codes are divided by (2^bits - 1),
/// floating samples are returned as floats. Restore integer code units before
/// DNG linearization/black/white normalization. Never request display sRGB or
/// linear-sRGB: DNG, not the codestream colour label, defines these channels.
fn decode_jxl(
    bytes: &[u8],
    tw: usize,
    th: usize,
    bits: usize,
    format: usize,
    decode_pixels: bool,
) -> io::Result<(usize, usize, TileSamples)> {
    use jxl_oxide::{
        JxlImage, NullCms,
        image::{BitDepth, color::ColourEncoding},
    };
    // jxl-oxide has no max-dimension builder option. Supply its allocation
    // limit first, then enforce dimensions before pixel rendering or feeding
    // the remaining payload. Initialization may buffer a bounded frame prefix.
    let allocation_limit = tw
        .checked_mul(th)
        .and_then(|n| n.checked_mul(128))
        .and_then(|n| n.checked_add(16 * 1024 * 1024))
        .ok_or_else(|| invalid("JXL allocation budget overflow"))?
        .min(512 * 1024 * 1024);
    let mut uninit = JxlImage::builder()
        .alloc_tracker(jxl_oxide::AllocTracker::with_limit(allocation_limit))
        .build_uninit();
    let mut consumed = 0;
    let mut end = bytes.len().min(64);
    let mut image = loop {
        if consumed >= bytes.len() || consumed >= 64 * 1024 {
            return Err(invalid("truncated or excessive JXL header"));
        }
        // Geometric chunks bound repeated parsing to logarithmically many
        // attempts. Keep unconsumed container framing in the next slice.
        let n = uninit.feed_bytes(&bytes[consumed..end]).map_err(invalid)?;
        consumed += n;
        match uninit.try_init().map_err(invalid)? {
            jxl_oxide::InitializeResult::NeedMoreData(next) => {
                uninit = next;
                if end == bytes.len() || end == 64 * 1024 {
                    return Err(invalid("truncated or excessive JXL header"));
                }
                end = (end * 2).min(bytes.len()).min(64 * 1024);
            }
            jxl_oxide::InitializeResult::Initialized(image) => break image,
        }
    };
    let meta = &image.image_header().metadata;
    if image.width() as usize > tw || image.height() as usize > th {
        return Err(invalid("JXL tile size exceeds TIFF tile"));
    }
    if meta.animation.is_some() {
        return Err(invalid("multi-frame JXL tiles unsupported"));
    }
    if meta.orientation != 1 {
        return Err(invalid("JXL tile orientation must be identity"));
    }
    if meta.bit_depth.bits_per_sample() as usize != bits {
        return Err(invalid("JXL and TIFF bit depths differ"));
    }
    if matches!(meta.bit_depth, BitDepth::FloatSample { .. }) != (format == 3) {
        return Err(invalid("JXL and TIFF sample formats differ"));
    }
    if meta.grayscale() || !meta.ec_info.is_empty() {
        return Err(invalid("three JXL camera channels required"));
    }
    if !decode_pixels {
        return Ok((
            image.width() as usize,
            image.height() as usize,
            TileSamples::Jxl(jxl_oxide::FrameBuffer::new(0, 0, 3)),
        ));
    }
    let encoding = meta.colour_encoding.clone();
    image.set_cms(NullCms);
    match encoding {
        ColourEncoding::Enum(encoding) => image.request_color_encoding(encoding),
        ColourEncoding::IccProfile(_) => {
            let icc = image
                .original_icc()
                .ok_or_else(|| invalid("missing JXL ICC"))?
                .to_vec();
            image.request_icc(&icc).map_err(invalid)?;
        }
    }
    image.feed_bytes(&bytes[consumed..]).map_err(invalid)?;
    image.finalize().map_err(invalid)?;
    if image.num_loaded_keyframes() == 0 {
        return Err(invalid("JXL frame missing"));
    }
    if image.num_loaded_frames() != 1 {
        return Err(invalid("multi-frame JXL tiles unsupported"));
    }
    let rendered = image.render_frame(0).map_err(invalid)?;
    let frame = rendered.image_all_channels();
    if frame.buf().len() != frame.width() * frame.height() * 3 {
        return Err(invalid("three JXL camera channels required"));
    }
    Ok((frame.width(), frame.height(), TileSamples::Jxl(frame)))
}

struct Polynomial {
    area: [usize; 4],
    plane: usize,
    planes: usize,
    pitch: [usize; 2],
    coefficients: Vec<f64>,
}
impl Polynomial {
    fn map(&self, value: f64, x: usize, y: usize, c: usize, max: f64) -> f64 {
        let [top, left, bottom, right] = self.area;
        if y < top
            || y >= bottom
            || x < left
            || x >= right
            || c < self.plane
            || c >= self.plane + self.planes
            || !(y - top).is_multiple_of(self.pitch[0])
            || !(x - left).is_multiple_of(self.pitch[1])
        {
            return value;
        }
        // Stage 1 coefficients operate on native code units; stages 2/3 on 0..1.
        self.coefficients
            .iter()
            .rev()
            .fold(0., |sum, k| sum * value + k)
            .clamp(0., max)
    }
}
/// A bounded MapPolynomial list. Mixed operations fail closed to preserve order.
fn polynomial_list(bytes: Option<&[u8]>) -> io::Result<(Vec<Polynomial>, bool)> {
    let Some(bytes) = bytes else {
        return Ok((Vec::new(), false));
    };
    let word = |p: usize| -> io::Result<u32> {
        Ok(u32::from_be_bytes(
            bytes
                .get(p..p + 4)
                .ok_or_else(|| invalid("truncated opcode"))?
                .try_into()
                .unwrap(),
        ))
    };
    let count = word(0)? as usize;
    if count > 4096 {
        return Err(invalid("opcode budget exceeded"));
    }
    let mut cursor = 4usize;
    let mut ops = Vec::new();
    let mut other = false;
    for _ in 0..count {
        let id = word(cursor)?;
        let version = word(cursor + 4)?;
        let flags = word(cursor + 8)?;
        let length = word(cursor + 12)? as usize;
        cursor += 16;
        let end = cursor
            .checked_add(length)
            .filter(|&p| p <= bytes.len())
            .ok_or_else(|| invalid("opcode length"))?;
        if id == 8 {
            if version > 0x01030000 || flags & !3 != 0 {
                return Err(invalid("polynomial opcode version or flags"));
            }
            let degree = word(cursor + 32)? as usize;
            if degree > 8 || length != 36 + (degree + 1) * 8 {
                return Err(invalid("polynomial degree/length"));
            }
            let area = [
                word(cursor)? as usize,
                word(cursor + 4)? as usize,
                word(cursor + 8)? as usize,
                word(cursor + 12)? as usize,
            ];
            let plane = word(cursor + 16)? as usize;
            let planes = word(cursor + 20)? as usize;
            let pitch = [word(cursor + 24)? as usize, word(cursor + 28)? as usize];
            if plane >= 3
                || planes == 0
                || plane + planes > 3
                || pitch.contains(&0)
                || area[0] > area[2]
                || area[1] > area[3]
            {
                return Err(invalid("polynomial area"));
            }
            let coefficients: Vec<f64> = bytes[cursor + 36..end]
                .as_chunks::<8>()
                .0
                .iter()
                .map(|b| f64::from_be_bytes(*b))
                .collect();
            if coefficients.iter().any(|v| !v.is_finite()) {
                return Err(invalid("nonfinite polynomial"));
            }
            ops.push(Polynomial {
                area,
                plane,
                planes,
                pitch,
                coefficients,
            });
        } else {
            other = true;
        }
        cursor = end;
    }
    if cursor != bytes.len() || other && !ops.is_empty() {
        return Err(invalid("mixed or trailing polynomial opcodes unsupported"));
    }
    Ok((ops, other))
}

fn decode_jpeg(
    bytes: &[u8],
    tw: usize,
    th: usize,
    decode_pixels: bool,
) -> io::Result<(usize, usize, TileSamples)> {
    // Called only for the selected PhotometricInterpretation=LinearRaw IFD.
    // A YCbCr IFD never reaches this marker rewrite.
    let jpeg = without_adobe(bytes)?;
    let mut decoder = JpegDecoder::new_with_options(
        ZCursor::new(&jpeg),
        DecoderOptions::default()
            .set_max_width(tw)
            .set_max_height(th),
    );
    decoder.decode_headers().map_err(invalid)?;
    let info = decoder
        .info()
        .ok_or_else(|| invalid("JPEG header missing"))?;
    let space = decoder
        .input_colorspace()
        .ok_or_else(|| invalid("JPEG colorspace missing"))?;
    if info.components != 3 || space.num_components() != 3 {
        return Err(invalid("three JPEG components required"));
    }
    // Only an explicit Adobe transform=0 authorizes raw component interleave.
    // Otherwise honor the codec's YCbCr/JFIF label and reconstruct RGB.
    let output = if jpeg.len() != bytes.len() {
        space
    } else {
        ColorSpace::RGB
    };
    decoder.set_options((*decoder.options()).jpeg_set_out_colorspace(output));
    if !decode_pixels {
        return Ok((
            info.width as usize,
            info.height as usize,
            TileSamples::Jpeg(Vec::new()),
        ));
    }
    let decoded = decoder.decode().map_err(invalid)?;
    Ok((
        info.width as usize,
        info.height as usize,
        TileSamples::Jpeg(decoded),
    ))
}

#[cfg(test)]
mod tests {
    #[test]
    fn classic_jpeg_ycbcr_adobe_marker_is_preserved() {
        // Synthetic classic JPEG marker stream: APP14 followed by SOS.
        let mut bytes = vec![
            255, 216, 255, 238, 0, 14, b'A', b'd', b'o', b'b', b'e', 0, 100, 0, 0, 0, 0, 1, 255,
            218,
        ];
        assert_eq!(super::without_adobe(&bytes).unwrap(), bytes);
        bytes[17] = 0;
        assert_eq!(super::without_adobe(&bytes).unwrap(), [255, 216, 255, 218]);
    }
}
