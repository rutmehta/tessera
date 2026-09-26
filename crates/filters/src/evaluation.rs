//! Non-destructive stacks and explicit global operator barriers.
use crate::{Buffer, Effect, Filter, FilterParams, Halo, checkpoint, cpu, validate};
use compositor::{geom::Rect, raster::Raster};
use engine_api::{EngineError, EngineResult};
use std::sync::atomic::AtomicBool;

impl Effect {
    /// Independently evaluate 256-pixel interiors with gathered, real-neighbour
    /// halos. Supports larger-than-engine halos without changing engine-api.
    /// WholeImage operators deliberately use the global path, never crop seams.
    pub fn apply_tiled(
        &self,
        input: &Raster,
        p: &FilterParams,
        cancel: &AtomicBool,
    ) -> EngineResult<Raster> {
        checkpoint(cancel)?;
        validate(p)?;
        if p.amount == 0.0 {
            return Ok(input.clone());
        }
        let Halo::Radius(halo) = self.halo(p) else {
            return self.apply(input, p, cancel);
        };
        let e = input.extent();
        if e.width == 0 || e.height == 0 || !matches!(input.channels(), 1 | 3 | 4) {
            return Err(EngineError::invalid("filters", "invalid raster"));
        }
        let mut out = input.clone();
        let rev = input
            .max_rev()
            .checked_add(1)
            .ok_or_else(|| EngineError::invalid("filters", "revision overflow"))?;
        let (nx, ny) = input.grid();
        for ty in 0..ny {
            for tx in 0..nx {
                checkpoint(cancel)?;
                let x0 = (tx * 256).saturating_sub(halo);
                let y0 = (ty * 256).saturating_sub(halo);
                let x1 = ((tx + 1) * 256).saturating_add(halo).min(e.width);
                let y1 = ((ty + 1) * 256).saturating_add(halo).min(e.height);
                let w = (x1 - x0) as usize;
                let h = (y1 - y0) as usize;
                let mut pixels = Vec::with_capacity(w * h);
                for y in y0..y1 {
                    checkpoint(cancel)?;
                    for x in x0..x1 {
                        let mut v = input.pixel(x, y);
                        if input.channels() == 1 {
                            v[1] = v[0];
                            v[2] = v[0];
                        }
                        if input.channels() != 4 {
                            v[3] = 1.0;
                        }
                        pixels.push(v);
                    }
                }
                if pixels.iter().flatten().any(|v| !v.is_finite()) {
                    return Err(EngineError::invalid("filters", "finite pixels required"));
                }
                let src = Buffer { w, h, pixels };
                let mut dst = cpu::run(*self, &src, p, cancel)?;
                for (a, b) in src.pixels.iter().zip(&mut dst.pixels) {
                    for c in 0..4 {
                        b[c] = a[c] + p.amount * (b[c] - a[c]);
                    }
                }
                if dst.pixels.iter().flatten().any(|v| !v.is_finite()) {
                    return Err(EngineError::invalid("filters", "nonfinite result"));
                }
                out.edit_region(
                    Rect::new(
                        i64::from(tx) * 256,
                        i64::from(ty) * 256,
                        i64::from(tx + 1) * 256,
                        i64::from(ty + 1) * 256,
                    ),
                    rev,
                    |x, y, v| *v = dst.pixels[(y - y0) as usize * w + (x - x0) as usize],
                )?;
            }
        }
        checkpoint(cancel)?;
        Ok(out)
    }
}

/// An editable, non-destructive stack node. The original raster is never owned
/// mutably; callers can re-evaluate after editing parameters or filter masks.
#[derive(Clone, Debug)]
pub struct SmartFilter {
    pub enabled: bool,
    pub effect: Effect,
    pub params: FilterParams,
    pub mask: Option<Raster>,
}
#[derive(Clone, Debug, Default)]
pub struct SmartFilters {
    pub filters: Vec<SmartFilter>,
}
impl SmartFilters {
    pub fn apply(&self, input: &Raster, cancel: &AtomicBool) -> EngineResult<Raster> {
        let mut current = input.clone();
        for node in &self.filters {
            checkpoint(cancel)?;
            if !node.enabled {
                continue;
            }
            if let Some(mask) = &node.mask
                && (mask.extent() != input.extent() || mask.channels() != 1)
            {
                return Err(EngineError::invalid(
                    "smart filter",
                    "same-size one-channel mask required",
                ));
            }
            let next = node.effect.apply_tiled(&current, &node.params, cancel)?;
            current = if let Some(mask) = &node.mask {
                let mut dst = Buffer::read(&next, cancel)?;
                let src = Buffer::read(&current, cancel)?;
                for y in 0..dst.h {
                    checkpoint(cancel)?;
                    for x in 0..dst.w {
                        let m = mask.pixel(x as u32, y as u32)[0];
                        if !m.is_finite() || !(0.0..=1.0).contains(&m) {
                            return Err(EngineError::invalid(
                                "smart filter",
                                "mask values must be finite [0,1]",
                            ));
                        }
                        let i = y * dst.w + x;
                        for c in 0..4 {
                            dst.pixels[i][c] =
                                src.pixels[i][c] + m * (dst.pixels[i][c] - src.pixels[i][c]);
                        }
                    }
                }
                dst.write(&current, cancel)?
            } else {
                next
            };
        }
        checkpoint(cancel)?;
        Ok(current)
    }
}

/// Camera Raw is a full pipeline, not an ordinary local convolution. The host
/// supplies its configured raw/develop recipe processor; no synthetic stand-in.
pub trait CameraRawProcessor: Send + Sync {
    fn process(&self, input: &Raster, cancel: &AtomicBool) -> EngineResult<Raster>;
}
pub struct CameraRawFilter<'a> {
    pub processor: &'a dyn CameraRawProcessor,
}
impl Filter for CameraRawFilter<'_> {
    fn halo(&self, _: &FilterParams) -> Halo {
        Halo::WholeImage
    }
    fn apply(&self, input: &Raster, p: &FilterParams, cancel: &AtomicBool) -> EngineResult<Raster> {
        checkpoint(cancel)?;
        validate(p)?;
        if p.amount == 0.0 {
            return Ok(input.clone());
        }
        let result = self.processor.process(input, cancel)?;
        if result.extent() != input.extent()
            || result.channels() != input.channels()
            || result.depth() != input.depth()
        {
            return Err(EngineError::invalid(
                "camera raw",
                "processor changed raster layout",
            ));
        }
        let src = Buffer::read(input, cancel)?;
        let mut dst = Buffer::read(&result, cancel)?;
        for (a, b) in src.pixels.iter().zip(&mut dst.pixels) {
            for c in 0..4 {
                b[c] = a[c] + p.amount * (b[c] - a[c]);
            }
        }
        dst.write(input, cancel)
    }
}
