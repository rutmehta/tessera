//! The internal `.tessera-doc` container: a JSON manifest describing the
//! whole document model plus zstd-compressed tile chunks.
//!
//! Layout (all integers little-endian):
//!
//! ```text
//! "TSRDOC\0\x01"                      8-byte magic
//! chunk 0 … chunk N-1                  zstd frames of raw tile samples
//! manifest                             zstd frame of UTF-8 JSON (Manifest)
//! u64 manifest offset, u64 manifest length, "TSRDEND\0"   24-byte trailer
//! ```
//!
//! Chunks are deduplicated by BLAKE3 of their raw bytes, so COW-shared
//! tiles (duplicated layers) are stored once. Revisions are persisted and
//! the revision clock is advanced past them on load. History is not
//! persisted (like PSD); a loaded document starts a fresh history.

use std::collections::HashMap;
use std::io::Write;
use std::path::Path;
use std::sync::Arc;

use engine_api::color::IccProfileHandle;
use engine_api::tile::{Extent, Tile, TileCoord, TileFormat};
use engine_api::{EngineError, EngineResult};
use serde::{Deserialize, Serialize};

use crate::adjust::Adjustment;
use crate::document::{
    ColorProfile, DocState, Fill, GroupMode, Layer, LayerId, LayerKind, LayerProps, Mask,
    SmartFilter, SmartObject, TextLayer, VectorMask,
};
use crate::edit::Document;
use crate::geom::{Affine, next_doc_key, observe_rev};
use crate::raster::{Depth, Raster};

const MAGIC: &[u8; 8] = b"TSRDOC\0\x01";
const END: &[u8; 8] = b"TSRDEND\0";
/// Manifest format version.
pub const FORMAT_VERSION: u32 = 1;

#[derive(Serialize, Deserialize)]
struct Manifest {
    format: String,
    version: u32,
    chunks: Vec<MChunk>,
    document: MDoc,
}

#[derive(Serialize, Deserialize, Clone, Copy)]
struct MChunk {
    offset: u64,
    len: u64,
    raw_len: u64,
}

#[derive(Serialize, Deserialize)]
struct MDoc {
    #[serde(default)]
    global_light: crate::render::styles::GlobalLight,
    canvas: Extent,
    depth: Depth,
    ppi: f32,
    profile: Option<MProfile>,
    root_rev: u64,
    rev: u64,
    next_id: u64,
    selection: Option<MRaster>,
    layers: Vec<MLayer>,
}

#[derive(Serialize, Deserialize)]
struct MProfile {
    name: String,
    handle: IccProfileHandle,
    icc_chunk: Option<u32>,
}

#[derive(Serialize, Deserialize)]
struct MLayer {
    id: LayerId,
    props: LayerProps,
    props_rev: u64,
    content_rev: u64,
    mask: Option<MMask>,
    vector_mask: Option<VectorMask>,
    kind: MKind,
}

#[derive(Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum MKind {
    Pixel {
        raster: MRaster,
    },
    Adjustment {
        adjustment: Adjustment,
    },
    Fill {
        fill: Fill,
    },
    Group {
        mode: GroupMode,
        children: Vec<MLayer>,
    },
    SmartObject {
        document: Box<MDoc>,
        transform: Affine,
        filters: Vec<SmartFilter>,
        #[serde(default)]
        filter_mask: Option<MMask>,
    },
    Text {
        text: String,
        font: String,
        size: f32,
        color: [f32; 3],
        proxy: MRaster,
    },
}

#[derive(Serialize, Deserialize)]
struct MMask {
    raster: MRaster,
    density: f32,
    feather: f32,
    enabled: bool,
}

#[derive(Serialize, Deserialize)]
struct MRaster {
    extent: Extent,
    channels: u8,
    depth: Depth,
    default: f32,
    tiles: Vec<MTile>,
}

#[derive(Serialize, Deserialize)]
struct MTile {
    x: u32,
    y: u32,
    rev: u64,
    /// `None` for a tombstone.
    chunk: Option<u32>,
}

struct Writer<W: Write> {
    out: W,
    pos: u64,
    chunks: Vec<MChunk>,
    seen: HashMap<[u8; 32], u32>,
    level: i32,
}

fn io(e: std::io::Error) -> EngineError {
    EngineError::Io {
        path: None,
        message: e.to_string(),
    }
}

fn enc(e: impl std::fmt::Display) -> EngineError {
    EngineError::Encode {
        format: "tessera-doc".into(),
        message: e.to_string(),
    }
}

fn dec(e: impl std::fmt::Display) -> EngineError {
    EngineError::Decode {
        format: "tessera-doc".into(),
        message: e.to_string(),
    }
}

fn tile_bytes(t: &Tile) -> EngineResult<Vec<u8>> {
    Ok(match t.format() {
        TileFormat::U8 => t.samples::<u8>()?.to_vec(),
        TileFormat::U16 => t
            .samples::<u16>()?
            .iter()
            .flat_map(|v| v.to_le_bytes())
            .collect(),
        TileFormat::F32Planar => t
            .samples::<f32>()?
            .iter()
            .flat_map(|v| v.to_le_bytes())
            .collect(),
        TileFormat::F16Planar => t
            .samples::<half::f16>()?
            .iter()
            .flat_map(|v| v.to_le_bytes())
            .collect(),
    })
}

impl<W: Write> Writer<W> {
    fn chunk(&mut self, raw: &[u8]) -> EngineResult<u32> {
        let h = *blake3::hash(raw).as_bytes();
        if let Some(&i) = self.seen.get(&h) {
            return Ok(i);
        }
        let z = zstd::bulk::compress(raw, self.level).map_err(enc)?;
        self.out.write_all(&z).map_err(io)?;
        let i = self.chunks.len() as u32;
        self.chunks.push(MChunk {
            offset: self.pos,
            len: z.len() as u64,
            raw_len: raw.len() as u64,
        });
        self.pos += z.len() as u64;
        self.seen.insert(h, i);
        Ok(i)
    }

    fn raster(&mut self, r: &Raster) -> EngineResult<MRaster> {
        let mut tiles = Vec::new();
        for ((x, y), s) in r.slots() {
            let chunk = match &s.tile {
                Some(t) => Some(self.chunk(&tile_bytes(t)?)?),
                None => None,
            };
            tiles.push(MTile {
                x,
                y,
                rev: s.rev,
                chunk,
            });
        }
        Ok(MRaster {
            extent: r.extent(),
            channels: r.channels(),
            depth: r.depth(),
            default: r.default_value(),
            tiles,
        })
    }

    fn layer(&mut self, l: &Layer) -> EngineResult<MLayer> {
        let kind = match &l.kind {
            LayerKind::Pixel(r) => MKind::Pixel {
                raster: self.raster(r)?,
            },
            LayerKind::Adjustment(a) => MKind::Adjustment {
                adjustment: a.clone(),
            },
            LayerKind::Fill(f) => MKind::Fill { fill: f.clone() },
            LayerKind::Group { mode, children } => MKind::Group {
                mode: *mode,
                children: children
                    .iter()
                    .map(|c| self.layer(c))
                    .collect::<EngineResult<_>>()?,
            },
            LayerKind::SmartObject(so) => MKind::SmartObject {
                document: Box::new(self.doc(&so.state)?),
                transform: so.transform,
                filters: so.filters.clone(),
                filter_mask: so
                    .filter_mask
                    .as_ref()
                    .map(|m| -> EngineResult<MMask> {
                        Ok(MMask {
                            raster: self.raster(&m.raster)?,
                            density: m.density,
                            feather: m.feather,
                            enabled: m.enabled,
                        })
                    })
                    .transpose()?,
            },
            LayerKind::Text(t) => MKind::Text {
                text: t.text.clone(),
                font: t.font.clone(),
                size: t.size,
                color: t.color,
                proxy: self.raster(&t.proxy)?,
            },
        };
        let mask = match &l.mask {
            Some(m) => Some(MMask {
                raster: self.raster(&m.raster)?,
                density: m.density,
                feather: m.feather,
                enabled: m.enabled,
            }),
            None => None,
        };
        Ok(MLayer {
            id: l.id,
            props: l.props.clone(),
            props_rev: l.props_rev,
            content_rev: l.content_rev,
            mask,
            vector_mask: l.vector_mask.clone(),
            kind,
        })
    }

    fn doc(&mut self, s: &DocState) -> EngineResult<MDoc> {
        let profile = match &s.profile {
            Some(p) => Some(MProfile {
                name: p.name.clone(),
                handle: p.handle,
                icc_chunk: p.icc.as_ref().map(|b| self.chunk(b)).transpose()?,
            }),
            None => None,
        };
        Ok(MDoc {
            global_light: s.global_light,
            canvas: s.canvas,
            depth: s.depth,
            ppi: s.ppi,
            profile,
            root_rev: s.root_rev,
            rev: s.rev,
            next_id: s.next_id,
            selection: s.selection.as_ref().map(|r| self.raster(r)).transpose()?,
            layers: s
                .root
                .iter()
                .map(|l| self.layer(l))
                .collect::<EngineResult<_>>()?,
        })
    }
}

/// Serializes a document state.
pub fn to_bytes(state: &DocState) -> EngineResult<Vec<u8>> {
    let mut w = Writer {
        out: Vec::new(),
        pos: MAGIC.len() as u64,
        chunks: Vec::new(),
        seen: HashMap::new(),
        level: 3,
    };
    w.out.extend_from_slice(MAGIC);
    let document = w.doc(state)?;
    let manifest = Manifest {
        format: "tessera-doc".into(),
        version: FORMAT_VERSION,
        chunks: w.chunks.clone(),
        document,
    };
    let json = serde_json::to_vec(&manifest).map_err(enc)?;
    let z = zstd::bulk::compress(&json, w.level).map_err(enc)?;
    let moff = w.pos;
    w.out.extend_from_slice(&z);
    w.out.extend_from_slice(&moff.to_le_bytes());
    w.out.extend_from_slice(&(z.len() as u64).to_le_bytes());
    w.out.extend_from_slice(END);
    Ok(w.out)
}

/// Writes a document's current state to `path`.
pub fn save(doc: &Document, path: impl AsRef<Path>) -> EngineResult<()> {
    let bytes = to_bytes(doc.state())?;
    let path = path.as_ref();
    let tmp = path.with_extension("tessera-doc.tmp");
    std::fs::write(&tmp, &bytes).map_err(|e| EngineError::io_at(&tmp, &e))?;
    std::fs::rename(&tmp, path).map_err(|e| EngineError::io_at(path, &e))
}

/// Loads a document (fresh history rooted at the saved state).
pub fn load(path: impl AsRef<Path>) -> EngineResult<Document> {
    let path = path.as_ref();
    let bytes = std::fs::read(path).map_err(|e| EngineError::io_at(path, &e))?;
    Ok(Document::new(from_bytes(&bytes)?))
}

struct Reader<'a> {
    bytes: &'a [u8],
    chunks: Vec<MChunk>,
    decoded: HashMap<u32, Arc<Vec<u8>>>,
    max_rev: u64,
}

impl Reader<'_> {
    fn chunk(&mut self, i: u32) -> EngineResult<Arc<Vec<u8>>> {
        if let Some(c) = self.decoded.get(&i) {
            return Ok(c.clone());
        }
        let c = *self
            .chunks
            .get(i as usize)
            .ok_or_else(|| dec("chunk index"))?;
        let end = c
            .offset
            .checked_add(c.len)
            .ok_or_else(|| dec("chunk range"))?;
        let z = self
            .bytes
            .get(c.offset as usize..end as usize)
            .ok_or_else(|| dec("chunk outside file"))?;
        let raw = zstd::bulk::decompress(z, c.raw_len as usize).map_err(dec)?;
        if raw.len() as u64 != c.raw_len {
            return Err(dec("chunk length"));
        }
        let raw = Arc::new(raw);
        self.decoded.insert(i, raw.clone());
        Ok(raw)
    }

    fn raster(&mut self, m: &MRaster) -> EngineResult<Raster> {
        if m.channels == 0 || m.channels > 4 {
            return Err(dec("raster channels"));
        }
        let mut r = Raster::new(m.extent, m.channels, m.depth, m.default);
        let (cols, rows) = r.grid();
        for t in &m.tiles {
            self.max_rev = self.max_rev.max(t.rev);
            if t.x >= cols || t.y >= rows {
                return Err(dec("tile outside raster"));
            }
            let tile = match t.chunk {
                None => None,
                Some(c) => {
                    let raw = self.chunk(c)?;
                    let layout = r.layout(t.x, t.y);
                    let coord = TileCoord::new(0, t.x, t.y);
                    let bps = m.depth.bytes();
                    if raw.len() != layout.len() * bps {
                        return Err(dec("tile size"));
                    }
                    Some(match m.depth {
                        Depth::U8 => Tile::from_samples(coord, layout, raw.to_vec())?,
                        Depth::U16 => Tile::from_samples(
                            coord,
                            layout,
                            raw.as_chunks::<2>()
                                .0
                                .iter()
                                .map(|b| u16::from_le_bytes([b[0], b[1]]))
                                .collect(),
                        )?,
                        Depth::F32 => Tile::from_samples(
                            coord,
                            layout,
                            raw.as_chunks::<4>()
                                .0
                                .iter()
                                .map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]]))
                                .collect(),
                        )?,
                    })
                }
            };
            r.set_slot(t.x, t.y, tile, t.rev)?;
        }
        Ok(r)
    }

    fn layer(&mut self, m: &MLayer) -> EngineResult<Layer> {
        self.max_rev = self.max_rev.max(m.props_rev).max(m.content_rev);
        let kind = match &m.kind {
            MKind::Pixel { raster } => LayerKind::Pixel(self.raster(raster)?),
            MKind::Adjustment { adjustment } => LayerKind::Adjustment(adjustment.clone()),
            MKind::Fill { fill } => LayerKind::Fill(fill.clone()),
            MKind::Group { mode, children } => LayerKind::Group {
                mode: *mode,
                children: children
                    .iter()
                    .map(|c| self.layer(c).map(Arc::new))
                    .collect::<EngineResult<_>>()?,
            },
            MKind::SmartObject {
                document,
                transform,
                filters,
                filter_mask,
            } => LayerKind::SmartObject(SmartObject {
                state: Arc::new(self.doc(document)?),
                transform: *transform,
                filters: filters.clone(),
                filter_mask: filter_mask
                    .as_ref()
                    .map(|m| -> EngineResult<Mask> {
                        Ok(Mask {
                            raster: self.raster(&m.raster)?,
                            density: m.density,
                            feather: m.feather,
                            enabled: m.enabled,
                        })
                    })
                    .transpose()?,
                key: next_doc_key(),
            }),
            MKind::Text {
                text,
                font,
                size,
                color,
                proxy,
            } => LayerKind::Text(TextLayer {
                text: text.clone(),
                font: font.clone(),
                size: *size,
                color: *color,
                proxy: self.raster(proxy)?,
            }),
        };
        let mask = match &m.mask {
            Some(mm) => Some(Mask {
                raster: self.raster(&mm.raster)?,
                density: mm.density,
                feather: mm.feather,
                enabled: mm.enabled,
            }),
            None => None,
        };
        Ok(Layer {
            id: m.id,
            props: m.props.clone(),
            props_rev: m.props_rev,
            content_rev: m.content_rev,
            mask,
            vector_mask: m.vector_mask.clone(),
            kind,
        })
    }

    fn doc(&mut self, m: &MDoc) -> EngineResult<DocState> {
        self.max_rev = self.max_rev.max(m.rev).max(m.root_rev);
        let profile = match &m.profile {
            Some(p) => Some(ColorProfile {
                name: p.name.clone(),
                handle: p.handle,
                icc: p
                    .icc_chunk
                    .map(|c| self.chunk(c).map(|b| Arc::new(b.to_vec())))
                    .transpose()?,
            }),
            None => None,
        };
        let state = DocState {
            global_light: m.global_light,
            canvas: m.canvas,
            depth: m.depth,
            ppi: m.ppi,
            profile,
            root: m
                .layers
                .iter()
                .map(|l| self.layer(l).map(Arc::new))
                .collect::<EngineResult<_>>()?,
            root_rev: m.root_rev,
            rev: m.rev,
            next_id: m.next_id,
            selection: m
                .selection
                .as_ref()
                .map(|r| self.raster(r).map(Arc::new))
                .transpose()?,
        };
        let ids = state.layer_ids();
        let mut uniq = ids.clone();
        uniq.sort();
        uniq.dedup();
        if uniq.len() != ids.len() {
            return Err(dec("duplicate layer ids"));
        }
        Ok(state)
    }
}

/// Parses a `.tessera-doc`.
pub fn from_bytes(bytes: &[u8]) -> EngineResult<DocState> {
    if bytes.len() < 32 || &bytes[..8] != MAGIC || &bytes[bytes.len() - 8..] != END {
        return Err(dec("not a tessera-doc"));
    }
    let t = bytes.len() - 24;
    let moff = u64::from_le_bytes(bytes[t..t + 8].try_into().map_err(dec)?) as usize;
    let mlen = u64::from_le_bytes(bytes[t + 8..t + 16].try_into().map_err(dec)?) as usize;
    let z = bytes
        .get(
            moff..moff
                .checked_add(mlen)
                .ok_or_else(|| dec("manifest range"))?,
        )
        .ok_or_else(|| dec("manifest outside file"))?;
    let json = zstd::stream::decode_all(z).map_err(dec)?;
    let m: Manifest = serde_json::from_slice(&json).map_err(dec)?;
    if m.format != "tessera-doc" {
        return Err(dec("format tag"));
    }
    if m.version > FORMAT_VERSION {
        return Err(EngineError::SchemaVersion {
            document: "tessera-doc".into(),
            found: m.version,
            supported: FORMAT_VERSION,
        });
    }
    let mut r = Reader {
        bytes,
        chunks: m.chunks,
        decoded: HashMap::new(),
        max_rev: 0,
    };
    let state = r.doc(&m.document)?;
    observe_rev(r.max_rev);
    Ok(state)
}
