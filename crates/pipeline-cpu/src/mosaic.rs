use engine_api::{
    EngineError, EngineResult,
    recipe::settings::HighlightReconstruction,
    tile::{TILE_SIZE, Tile, TileLayout},
};
use raw_decode::CfaLayout;

/// Auto resolves to MHC for Bayer without changing the recipe contract.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DemosaicAlgorithm {
    Bilinear,
    MalvarHeCutler,
}

/// Algebraic inverse of the unclamped decode normalization. Clipped sensor
/// values cannot be recovered by this helper.
pub fn inverse_linearize(value: f32, black: f32, white: f32) -> EngineResult<f32> {
    if !value.is_finite() || !black.is_finite() || !white.is_finite() || white <= black {
        return Err(EngineError::invalid(
            "linearization",
            "finite values and white > black required",
        ));
    }
    Ok(value * (white - black) + black)
}

fn rgb_channel(c: usize) -> usize {
    if c == 3 { 1 } else { c }
}

pub(crate) fn validate_cfa(cfa: CfaLayout) -> EngineResult<()> {
    let valid = match cfa {
        CfaLayout::Bayer(p) => {
            let p = p.map(|r| r.map(|c| rgb_channel(c as usize)));
            let count = |c| p.iter().flatten().filter(|&&v| v == c).count();
            count(0) == 1
                && count(1) == 2
                && count(2) == 1
                && p[0][0] + p[1][1] == 2
                && p[0][1] + p[1][0] == 2
        }
        CfaLayout::XTrans(p) => {
            p.iter().flatten().all(|&c| c < 3)
                && (0..3).all(|c| p.iter().flatten().any(|&v| v == c))
        }
        CfaLayout::Unsupported => false,
    };
    if valid {
        Ok(())
    } else {
        Err(EngineError::invalid(
            "CFA",
            "unsupported or malformed pattern",
        ))
    }
}

struct Mosaic<'a> {
    data: &'a [f32],
    layout: TileLayout,
    origin: (u32, u32),
    cfa: CfaLayout,
}
impl<'a> Mosaic<'a> {
    fn new(tile: &'a Tile, cfa: CfaLayout, halo: u16) -> EngineResult<Self> {
        validate_cfa(cfa)?;
        if tile.layout().channels != 1 || tile.halo() < halo {
            return Err(EngineError::invalid(
                "CFA tile",
                format!("one plane and halo >= {halo} required"),
            ));
        }
        Ok(Self {
            data: tile.samples::<f32>()?,
            layout: tile.layout(),
            origin: tile.coord().pixel_origin(TILE_SIZE),
            cfa,
        })
    }
    fn get(&self, x: i32, y: i32) -> f32 {
        self.data[self.layout.index(0, x, y).expect("validated halo")]
    }
    fn channel(&self, x: i32, y: i32) -> usize {
        let period = if matches!(self.cfa, CfaLayout::XTrans(_)) {
            6
        } else {
            2
        };
        rgb_channel(self.cfa.channel_at(
            (i64::from(self.origin.0) + i64::from(x)).rem_euclid(period) as u32,
            (i64::from(self.origin.1) + i64::from(y)).rem_euclid(period) as u32,
        ))
    }
    fn mean(&self, x: i32, y: i32, c: usize, r: i32) -> Option<f32> {
        let (mut sum, mut count) = (0.0, 0);
        for dy in -r..=r {
            for dx in -r..=r {
                if self.channel(x + dx, y + dy) == c {
                    sum += self.get(x + dx, y + dy);
                    count += 1;
                }
            }
        }
        (count > 0).then(|| sum / count as f32)
    }
}

/// Bayer bilinear or independent MHC (Malvar/He/Cutler 2004, Fig. 2).
/// Requires a two-pixel halo for Bayer, three for X-Trans. Outputs RGB
/// interior only. X-Trans uses 3x3 means, falling back to 7x7 if absent.
pub fn demosaic(tile: &Tile, cfa: CfaLayout, algorithm: DemosaicAlgorithm) -> EngineResult<Tile> {
    let xtrans = matches!(cfa, CfaLayout::XTrans(_));
    let m = Mosaic::new(tile, cfa, if xtrans { 3 } else { 2 })?;
    let e = tile.layout().extent;
    let n = e.area() as usize;
    let mut out = vec![0.0; n * 3];
    for y in 0..e.height as i32 {
        for x in 0..e.width as i32 {
            let known = m.channel(x, y);
            let v = m.get(x, y);
            for c in 0..3 {
                let value = if c == known {
                    v
                } else if xtrans || algorithm == DemosaicAlgorithm::Bilinear {
                    m.mean(x, y, c, 1)
                        .or_else(|| xtrans.then(|| m.mean(x, y, c, 3)).flatten())
                        .unwrap_or(v)
                } else {
                    let h1 = m.get(x - 1, y) + m.get(x + 1, y);
                    let v1 = m.get(x, y - 1) + m.get(x, y + 1);
                    let h2 = m.get(x - 2, y) + m.get(x + 2, y);
                    let v2 = m.get(x, y - 2) + m.get(x, y + 2);
                    let diag = m.get(x - 1, y - 1)
                        + m.get(x + 1, y - 1)
                        + m.get(x - 1, y + 1)
                        + m.get(x + 1, y + 1);
                    if c == 1 {
                        (4.0 * v + 2.0 * (h1 + v1) - h2 - v2) / 8.0
                    } else if known != 1 {
                        (6.0 * v + 2.0 * diag - 1.5 * (h2 + v2)) / 8.0
                    } else if m.channel(x + 1, y) == c {
                        (5.0 * v + 4.0 * h1 - h2 - diag + 0.5 * v2) / 8.0
                    } else {
                        (5.0 * v + 4.0 * v1 - v2 - diag + 0.5 * h2) / 8.0
                    }
                };
                out[c * n + y as usize * e.width as usize + x as usize] = value;
            }
        }
    }
    Tile::from_samples(
        tile.coord(),
        TileLayout {
            extent: e,
            halo: 0,
            channels: 3,
        },
        out,
    )
}

/// Pre-demosaic channel-ratio propagation. Read a frozen halo, never pixels
/// written earlier in the scan. Fully clipped/no-donor regions fall back to 1.
pub fn reconstruct_highlights(
    tile: &Tile,
    cfa: CfaLayout,
    mode: HighlightReconstruction,
) -> EngineResult<Tile> {
    if !matches!(
        mode,
        HighlightReconstruction::Clip | HighlightReconstruction::ReconstructColor
    ) {
        return Err(EngineError::invalid(
            "highlight reconstruction",
            "mode not implemented in M1",
        ));
    }
    let m = Mosaic::new(
        tile,
        cfa,
        if mode == HighlightReconstruction::Clip {
            0
        } else {
            4
        },
    )?;
    let e = tile.layout().extent;
    let mut out = Vec::with_capacity(e.area() as usize);
    for y in 0..e.height as i32 {
        for x in 0..e.width as i32 {
            let value = m.get(x, y);
            if value < 1.0 || mode == HighlightReconstruction::Clip {
                out.push(value.min(1.0));
                continue;
            }
            let channel = m.channel(x, y);
            let proxy = |px: i32, py: i32| {
                let (mut sum, mut count) = (0.0, 0);
                for dy in -1..=1 {
                    for dx in -1..=1 {
                        let v = m.get(px + dx, py + dy);
                        if m.channel(px + dx, py + dy) != channel && v > 0.0 && v < 1.0 {
                            sum += v;
                            count += 1;
                        }
                    }
                }
                if count > 0 { sum / count as f32 } else { 0.0 }
            };
            let target = proxy(x, y);
            let (mut ratios, mut count) = (0.0, 0);
            for dy in -3..=3 {
                for dx in -3..=3 {
                    let v = m.get(x + dx, y + dy);
                    if m.channel(x + dx, y + dy) == channel && v > 0.0 && v < 1.0 {
                        let p = proxy(x + dx, y + dy);
                        if p > 1e-6 {
                            ratios += v / p;
                            count += 1;
                        }
                    }
                }
            }
            out.push(if count > 0 && target > 0.0 {
                (target * ratios / count as f32).clamp(1.0, 4.0)
            } else {
                1.0
            });
        }
    }
    Tile::from_samples(
        tile.coord(),
        TileLayout {
            extent: e,
            halo: 0,
            channels: 1,
        },
        out,
    )
}
