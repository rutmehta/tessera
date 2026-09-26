//! Lossless sparse raster wire form for clone/heal preset sources.
use compositor::{Depth, Raster};
use engine_api::tile::{Extent, Tile, TileCoord};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

#[derive(Serialize, Deserialize)]
struct WireRaster {
    extent: Extent,
    channels: u8,
    depth: Depth,
    default_bits: u32,
    slots: Vec<WireSlot>,
}
#[derive(Serialize, Deserialize)]
struct WireSlot {
    x: u32,
    y: u32,
    revision: u64,
    tile: Option<(TileCoord, Samples)>,
}
#[derive(Serialize, Deserialize)]
enum Samples {
    U8(Vec<u8>),
    U16(Vec<u16>),
    F32Bits(Vec<u32>),
}

pub(crate) fn serialize<S: Serializer>(source: &Option<Raster>, s: S) -> Result<S::Ok, S::Error> {
    let wire = source
        .as_ref()
        .map(|r| {
            let slots = r
                .slots()
                .map(|((x, y), slot)| {
                    let tile = slot
                        .tile
                        .as_ref()
                        .map(|t| {
                            let samples = match r.depth() {
                                Depth::U8 => Samples::U8(t.samples::<u8>()?.to_vec()),
                                Depth::U16 => Samples::U16(t.samples::<u16>()?.to_vec()),
                                Depth::F32 => Samples::F32Bits(
                                    t.samples::<f32>()?.iter().map(|f| f.to_bits()).collect(),
                                ),
                            };
                            Ok::<_, engine_api::EngineError>((t.coord(), samples))
                        })
                        .transpose()?;
                    Ok::<_, engine_api::EngineError>(WireSlot {
                        x,
                        y,
                        revision: slot.rev,
                        tile,
                    })
                })
                .collect::<Result<Vec<_>, _>>()?;
            Ok::<_, engine_api::EngineError>(WireRaster {
                extent: r.extent(),
                channels: r.channels(),
                depth: r.depth(),
                default_bits: r.default_value().to_bits(),
                slots,
            })
        })
        .transpose()
        .map_err(serde::ser::Error::custom)?;
    wire.serialize(s)
}

pub(crate) fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Option<Raster>, D::Error> {
    Option::<WireRaster>::deserialize(d)?
        .map(|w| {
            if !matches!(w.channels, 1 | 3 | 4) {
                return Err(serde::de::Error::custom(
                    "source raster needs 1, 3 or 4 channels",
                ));
            }
            let mut r = Raster::new(
                w.extent,
                w.channels,
                w.depth,
                f32::from_bits(w.default_bits),
            );
            let mut seen = std::collections::BTreeSet::new();
            let (cols, rows) = r.grid();
            for slot in w.slots {
                if slot.x >= cols || slot.y >= rows || !seen.insert((slot.x, slot.y)) {
                    return Err(serde::de::Error::custom(
                        "invalid or duplicate source tile position",
                    ));
                }
                let layout = r.layout(slot.x, slot.y);
                let tile = slot
                    .tile
                    .map(|(coord, samples)| match samples {
                        Samples::U8(v) => Tile::from_samples(coord, layout, v),
                        Samples::U16(v) => Tile::from_samples(coord, layout, v),
                        Samples::F32Bits(v) => Tile::from_samples(
                            coord,
                            layout,
                            v.into_iter().map(f32::from_bits).collect(),
                        ),
                    })
                    .transpose()
                    .map_err(serde::de::Error::custom)?;
                r.set_slot(slot.x, slot.y, tile, slot.revision)
                    .map_err(serde::de::Error::custom)?;
            }
            Ok(r)
        })
        .transpose()
}
