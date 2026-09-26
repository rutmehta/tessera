//! Non-destructive liquify. Displacements are **inverse** offsets: output (x,y)
//! samples source (x + dx, y + dy). Nodes lie at multiples of cell_size;
//! the final node may lie beyond the last pixel. Freeze protects edits, not render.
use crate::{Buffer, checkpoint};
use compositor::raster::Raster;
use engine_api::{EngineError, EngineResult};
use serde::{Deserialize, Serialize};
use std::sync::atomic::AtomicBool;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Interpolation {
    #[default]
    Bilinear,
    Bicubic,
}

impl Mesh {
    pub fn render(
        &self,
        input: &Raster,
        interpolation: Interpolation,
        cancel: &AtomicBool,
    ) -> EngineResult<Raster> {
        checkpoint(cancel)?;
        self.validate()?;
        let extent = input.extent();
        if extent.width != self.width || extent.height != self.height {
            return Err(invalid("mesh and raster dimensions differ"));
        }
        let source = read_source(input, cancel)?;
        checkpoint(cancel)?;
        if self.displacement.iter().all(|d| *d == [0., 0.]) {
            return Ok(input.clone());
        }
        // Render directly into final planar tiles: no full-image output clone,
        // old-tile unpack, or second interleaved-to-planar conversion.
        let rev = input
            .max_rev()
            .checked_add(1)
            .ok_or_else(|| invalid("revision overflow"))?;
        let (nx, ny) = input.grid();
        let count = (nx * ny) as usize;
        let workers = std::thread::available_parallelism()
            .map_or(1, usize::from)
            .min(count)
            .min(10);
        let next_tile = std::sync::atomic::AtomicUsize::new(0);
        let tiles = std::thread::scope(|scope| {
            let mut jobs = Vec::new();
            for _ in 0..workers {
                let source = &source;
                let next_tile = &next_tile;
                jobs.push(scope.spawn(move || -> EngineResult<Vec<_>> {
                    let mut tiles = Vec::new();
                    loop {
                        let index = next_tile.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                        if index >= count {
                            break;
                        }
                        let (tx, ty) = (index as u32 % nx, index as u32 / nx);
                        let layout = input.layout(tx, ty);
                        let plane = layout.plane_len();
                        let mut data = vec![0.; layout.len()];
                        for y in 0..layout.extent.height as usize {
                            checkpoint(cancel)?;
                            let yy = ty as usize * 256 + y;
                            for x in 0..layout.extent.width as usize {
                                let xx = tx as usize * 256 + x;
                                let d = self.displacement_at(xx as f32, yy as f32);
                                let sx = (xx as f32 + d[0]).clamp(0., (source.w - 1) as f32);
                                let sy = (yy as f32 + d[1]).clamp(0., (source.h - 1) as f32);
                                let p = sample(source, sx, sy, interpolation);
                                if p.iter().any(|v| !v.is_finite()) {
                                    return Err(invalid("nonfinite result"));
                                }
                                for c in 0..input.channels() as usize {
                                    data[c * plane + y * layout.stride() + x] = p[c];
                                }
                            }
                        }
                        use compositor::raster::Depth;
                        use engine_api::tile::{Tile, TileCoord};
                        let coord = TileCoord::new(0, tx, ty);
                        let tile = match input.depth() {
                            Depth::F32 => Tile::from_samples(coord, layout, data)?,
                            Depth::U8 => Tile::from_samples(
                                coord,
                                layout,
                                data.into_iter()
                                    .map(|v| (v.clamp(0., 1.) * 255. + 0.5) as u8)
                                    .collect(),
                            )?,
                            Depth::U16 => Tile::from_samples(
                                coord,
                                layout,
                                data.into_iter()
                                    .map(|v| (v.clamp(0., 1.) * 65535. + 0.5) as u16)
                                    .collect(),
                            )?,
                        };
                        tiles.push((tx, ty, tile));
                    }
                    Ok(tiles)
                }));
            }
            jobs.into_iter()
                .map(|job| job.join().map_err(|_| invalid("render worker panicked"))?)
                .collect::<EngineResult<Vec<_>>>()
        })?;
        checkpoint(cancel)?;
        let mut out = input.clone();
        for (tx, ty, tile) in tiles.into_iter().flatten() {
            out.set_slot(tx, ty, Some(tile), rev)?;
        }
        checkpoint(cancel)?;
        Ok(out)
    }
}
#[cfg(test)]
mod render_tests {
    use super::*;
    use compositor::{geom::Rect, raster::Depth};
    use engine_api::tile::Extent;

    #[test]
    fn parallel_packing_and_tiles_match_serial_reference() {
        let cancel = AtomicBool::new(false);
        for depth in [Depth::U8, Depth::U16, Depth::F32] {
            for channels in [1, 3, 4] {
                let mut input = Raster::new(Extent::new(263, 259), channels, depth, 0.25);
                // Leave the last tile row/column absent to exercise default samples.
                input
                    .edit_region(Rect::new(0, 0, 256, 256), 7, |x, y, p| {
                        *p = [x as f32 / 103., y as f32 / 97., -0.2, 0.7];
                    })
                    .unwrap();
                let before = input.clone();
                let source = Buffer::read(&input, &cancel).unwrap();
                assert_eq!(read_source(&input, &cancel).unwrap().pixels, source.pixels);
                let mut mesh = Mesh::new(263, 259, 7).unwrap();
                for (i, d) in mesh.displacement.iter_mut().enumerate() {
                    *d = [(i % 5) as f32 * 1.7 - 3.2, (i % 7) as f32 * 0.9 - 2.7];
                }
                for mode in [Interpolation::Bilinear, Interpolation::Bicubic] {
                    let mut expected = source.clone();
                    for y in 0..source.h {
                        for x in 0..source.w {
                            let d = mesh.displacement_at(x as f32, y as f32);
                            let sx = (x as f32 + d[0]).clamp(0., (source.w - 1) as f32);
                            let sy = (y as f32 + d[1]).clamp(0., (source.h - 1) as f32);
                            let (fx, fy) = (sx - sx.floor(), sy - sy.floor());
                            let range = if mode == Interpolation::Bilinear {
                                0..=1
                            } else {
                                -1..=2
                            };
                            let mut p = [0.; 4];
                            for dy in range.clone() {
                                for dx in range.clone() {
                                    let weight = match mode {
                                        Interpolation::Bilinear => {
                                            (if dx == 0 { 1. - fx } else { fx })
                                                * (if dy == 0 { 1. - fy } else { fy })
                                        }
                                        Interpolation::Bicubic => {
                                            cubic(dx as f32 - fx) * cubic(dy as f32 - fy)
                                        }
                                    };
                                    let q =
                                        source.at(sx.floor() as i32 + dx, sy.floor() as i32 + dy);
                                    for c in 0..4 {
                                        p[c] += q[c] * weight;
                                    }
                                }
                            }
                            expected.pixels[y * source.w + x] = p;
                        }
                    }
                    let expected = expected.write(&input, &cancel).unwrap();
                    let actual = mesh.render(&input, mode, &cancel).unwrap();
                    assert_eq!(actual.max_rev(), 8);
                    let (nx, ny) = input.grid();
                    let (mut a, mut b) = (Vec::new(), Vec::new());
                    for ty in 0..ny {
                        for tx in 0..nx {
                            actual.read_tile(tx, ty, &mut a).unwrap();
                            expected.read_tile(tx, ty, &mut b).unwrap();
                            assert_eq!(a, b, "{depth:?}/{channels}/{mode:?} tile {tx},{ty}");
                        }
                    }
                    assert!(input.shares_all_tiles_with(&before));
                    assert_eq!(input.max_rev(), 7);
                }
            }
        }
    }

    #[test]
    fn parallel_path_rejects_nonfinite_and_cancelled_inputs() {
        let input = Raster::new(Extent::new(263, 259), 4, Depth::F32, f32::NAN);
        let mesh = Mesh::new(263, 259, 16).unwrap();
        assert!(
            mesh.render(&input, Interpolation::Bilinear, &AtomicBool::new(false))
                .is_err()
        );
        assert!(matches!(
            mesh.render(&input, Interpolation::Bicubic, &AtomicBool::new(true)),
            Err(EngineError::Cancelled)
        ));
    }
}

/// Pack independent tile-row bands concurrently and validate before rendering.
fn read_source(input: &Raster, cancel: &AtomicBool) -> EngineResult<Buffer> {
    let e = input.extent();
    if e.area() < 256 * 256 || !matches!(input.channels(), 1 | 3 | 4) {
        return Buffer::read(input, cancel);
    }
    let w = e.width as usize;
    let mut pixels = vec![[0.; 4]; e.area() as usize];
    let (nx, ny) = input.grid();
    let workers = std::thread::available_parallelism()
        .map_or(1, usize::from)
        .min(10);
    let band_rows = (ny as usize).div_ceil(workers) * 256;
    std::thread::scope(|scope| -> EngineResult<()> {
        let mut jobs = Vec::new();
        for (band, dst) in pixels.chunks_mut(band_rows * w).enumerate() {
            jobs.push(scope.spawn(move || -> EngineResult<()> {
                let first_ty = band * band_rows / 256;
                let mut scratch = Vec::new();
                for local_ty in 0..dst.len().div_ceil(w * 256) {
                    let ty = (first_ty + local_ty) as u32;
                    for tx in 0..nx {
                        checkpoint(cancel)?;
                        let l = input.layout(tx, ty);
                        // Float tiles can be borrowed without a staging copy.
                        let samples = if let Some(tile) = input.tile(tx, ty)
                            && input.depth() == compositor::raster::Depth::F32
                        {
                            tile.samples::<f32>()?
                        } else {
                            input.read_tile(tx, ty, &mut scratch)?;
                            &scratch
                        };
                        let plane = l.plane_len();
                        for y in 0..l.extent.height as usize {
                            let start = (local_ty * 256 + y) * w + tx as usize * 256;
                            let row = &mut dst[start..start + l.extent.width as usize];
                            for (x, p) in row.iter_mut().enumerate() {
                                let i = y * l.stride() + x;
                                *p = match input.channels() {
                                    4 => [
                                        samples[i],
                                        samples[plane + i],
                                        samples[2 * plane + i],
                                        samples[3 * plane + i],
                                    ],
                                    3 => {
                                        [samples[i], samples[plane + i], samples[2 * plane + i], 1.]
                                    }
                                    _ => [samples[i], samples[i], samples[i], 1.],
                                };
                            }
                        }
                    }
                }
                if dst.iter().flatten().any(|v| !v.is_finite()) {
                    return Err(invalid("finite pixels required"));
                }
                checkpoint(cancel)
            }));
        }
        for job in jobs {
            job.join().map_err(|_| invalid("read worker panicked"))??;
        }
        Ok(())
    })?;
    Ok(Buffer {
        w,
        h: e.height as usize,
        pixels,
    })
}

fn sample(source: &Buffer, x: f32, y: f32, interpolation: Interpolation) -> [f32; 4] {
    let (ix, iy) = (x.floor() as i32, y.floor() as i32);
    let (fx, fy) = (x - x.floor(), y - y.floor());
    let mut result = [0.; 4];
    match interpolation {
        Interpolation::Bilinear => {
            for (dy, wy) in [(0, 1. - fy), (1, fy)] {
                for (dx, wx) in [(0, 1. - fx), (1, fx)] {
                    let p = source.at(ix + dx, iy + dy);
                    let weight = wx * wy;
                    for c in 0..4 {
                        result[c] += p[c] * weight;
                    }
                }
            }
        }
        Interpolation::Bicubic => {
            let xs = [-1, 0, 1, 2].map(|dx| (ix + dx).clamp(0, source.w as i32 - 1) as usize);
            let wx = [-1., 0., 1., 2.].map(|dx| cubic(dx - fx));
            let wy = [-1., 0., 1., 2.].map(|dy| cubic(dy - fy));
            for (dy, wy) in wy.into_iter().enumerate() {
                let row = (iy + dy as i32 - 1).clamp(0, source.h as i32 - 1) as usize * source.w;
                for dx in 0..4 {
                    let p = source.pixels[row + xs[dx]];
                    let weight = wx[dx] * wy;
                    for c in 0..4 {
                        result[c] += p[c] * weight;
                    }
                }
            }
        }
    }
    result
}
/// Catmull-Rom reconstruction; retain HDR/negative RGB, like the raster API.
fn cubic(x: f32) -> f32 {
    let t = x.abs();
    if t <= 1. {
        1.5 * t * t * t - 2.5 * t * t + 1.
    } else if t < 2. {
        -0.5 * t * t * t + 2.5 * t * t - 4. * t + 2.
    } else {
        0.
    }
}

/// Brush diameter in image pixels; normalized density (hard core radius),
/// pressure and rate in [0,1]. One call is one dab; callers resample strokes.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Brush {
    pub size: f32,
    pub density: f32,
    pub pressure: f32,
    pub rate: f32,
}
impl Default for Brush {
    fn default() -> Self {
        Self {
            size: 64.,
            density: 0.5,
            pressure: 0.5,
            rate: 0.5,
        }
    }
}
impl Brush {
    pub fn validate(&self) -> EngineResult<()> {
        if !self.size.is_finite()
            || self.size <= 0.
            || [self.density, self.pressure, self.rate]
                .iter()
                .any(|v| !v.is_finite() || !(0. ..=1.).contains(v))
        {
            Err(invalid("invalid brush controls"))
        } else {
            Ok(())
        }
    }
    fn weight(&self, distance: f32) -> f32 {
        let r = distance / (self.size * 0.5);
        let falloff = if r >= 1. {
            0.
        } else if r <= self.density {
            1.
        } else {
            let t = (r - self.density) / (1. - self.density);
            1. - t * t * (3. - 2. * t)
        };
        falloff * self.pressure * self.rate
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BrushTool {
    ForwardWarp,
    Reconstruct,
    Smooth,
    /// Clockwise in image coordinates.
    Twirl,
    TwirlCounterClockwise,
    Pucker,
    Bloat,
    PushLeft,
    Freeze,
    Thaw,
}
impl Mesh {
    /// Edits atomically. Positive twirl is visually clockwise in image space;
    /// push-left is the left normal of a drag (positive y points down).
    pub fn apply_brush(
        &mut self,
        tool: BrushTool,
        center: [f32; 2],
        delta: [f32; 2],
        brush: &Brush,
    ) -> EngineResult<()> {
        self.validate()?;
        brush.validate()?;
        if center.iter().chain(delta.iter()).any(|x| !x.is_finite()) {
            return Err(invalid("nonfinite brush position"));
        }
        let (w, h) = self.grid();
        let mut next = self.clone();
        for y in 0..h {
            for x in 0..w {
                let i = y * w + x;
                let p = [
                    x as f32 * self.cell_size as f32,
                    y as f32 * self.cell_size as f32,
                ];
                let v = [p[0] - center[0], p[1] - center[1]];
                let weight = brush.weight(v[0].hypot(v[1]));
                if weight == 0. {
                    continue;
                }
                if tool == BrushTool::Freeze {
                    next.freeze[i] = (self.freeze[i] + weight).min(1.);
                    continue;
                }
                if tool == BrushTool::Thaw {
                    next.freeze[i] = (self.freeze[i] - weight).max(0.);
                    continue;
                }
                let a = weight * (1. - self.freeze[i]);
                if a == 0. {
                    continue;
                }
                if tool == BrushTool::Reconstruct {
                    next.displacement[i] = self.displacement[i].map(|d| d * (1. - a));
                    continue;
                }
                if tool == BrushTool::Smooth {
                    let mut sum = [0.; 2];
                    let mut n = 0.;
                    for yy in y.saturating_sub(1)..=(y + 1).min(h - 1) {
                        for xx in x.saturating_sub(1)..=(x + 1).min(w - 1) {
                            for (c, s) in sum.iter_mut().enumerate() {
                                *s += self.displacement[yy * w + xx][c];
                            }
                            n += 1.;
                        }
                    }
                    for (c, s) in sum.iter().enumerate() {
                        next.displacement[i][c] = self.displacement[i][c] * (1. - a) + s / n * a;
                    }
                    continue;
                }
                let offset = match tool {
                    BrushTool::ForwardWarp => [-delta[0] * a, -delta[1] * a],
                    BrushTool::PushLeft => [-delta[1] * a, delta[0] * a],
                    BrushTool::Twirl | BrushTool::TwirlCounterClockwise => {
                        let direction = if tool == BrushTool::Twirl { -1. } else { 1. };
                        let (s, c) = (direction * a * 0.25).sin_cos();
                        [c * v[0] - s * v[1] - v[0], s * v[0] + c * v[1] - v[1]]
                    }
                    BrushTool::Pucker | BrushTool::Bloat => {
                        let scale = (if tool == BrushTool::Pucker {
                            a * 0.25
                        } else {
                            -a * 0.25
                        })
                        .exp()
                            - 1.;
                        [v[0] * scale, v[1] * scale]
                    }
                    _ => unreachable!(),
                };
                // Compose inverse maps rather than merely adding vectors, so a new
                // drag transports the already-deformed content under the brush.
                let old = self.displacement_at(p[0] + offset[0], p[1] + offset[1]);
                next.displacement[i] = [offset[0] + old[0], offset[1] + old[1]];
            }
        }
        next.validate()?;
        *self = next;
        Ok(())
    }
}

/// Normalized [-1,1] controls; zero is identity. Eye controls affect both eyes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct FaceParams {
    pub eye_size: f32,
    pub eye_height: f32,
    pub eye_width: f32,
    pub eye_tilt: f32,
    pub nose_width: f32,
    pub nose_height: f32,
    pub mouth_smile: f32,
    pub mouth_width: f32,
    pub mouth_height: f32,
    pub face_width: f32,
    pub jaw: f32,
    pub chin: f32,
    pub forehead: f32,
}
impl FaceParams {
    pub fn validate(&self) -> EngineResult<()> {
        if [
            self.eye_size,
            self.eye_height,
            self.eye_width,
            self.eye_tilt,
            self.nose_width,
            self.nose_height,
            self.mouth_smile,
            self.mouth_width,
            self.mouth_height,
            self.face_width,
            self.jaw,
            self.chin,
            self.forehead,
        ]
        .iter()
        .any(|v| !v.is_finite() || !(-1. ..=1.).contains(v))
        {
            Err(invalid("face controls must be finite in [-1,1]"))
        } else {
            Ok(())
        }
    }
}
/// YuNet's two eye centers, nose tip, and two mouth corners, in image pixels.
/// The first eye/corner pair may be on either side; roll is inferred from eyes.
/// Five points do not locate the jaw/chin/forehead: these regions are explicitly
/// heuristic ellipses scaled by interocular distance, not dense face landmarks.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FaceAware {
    pub landmarks5: [[f32; 2]; 5],
    pub params: FaceParams,
}
impl FaceAware {
    pub fn validate(&self) -> EngineResult<()> {
        self.params.validate()?;
        if self.landmarks5.iter().flatten().any(|v| !v.is_finite()) {
            return Err(invalid("nonfinite face landmarks"));
        }
        let a = self.landmarks5[0];
        let b = self.landmarks5[1];
        let d = (b[0] - a[0]).hypot(b[1] - a[1]);
        let mouth = [
            (self.landmarks5[3][0] + self.landmarks5[4][0]) * 0.5,
            (self.landmarks5[3][1] + self.landmarks5[4][1]) * 0.5,
        ];
        let mid = [(a[0] + b[0]) * 0.5, (a[1] + b[1]) * 0.5];
        let cross =
            ((b[0] - a[0]) * (mouth[1] - mid[1]) - (b[1] - a[1]) * (mouth[0] - mid[0])).abs();
        let mw = (self.landmarks5[3][0] - self.landmarks5[4][0])
            .hypot(self.landmarks5[3][1] - self.landmarks5[4][1]);
        if !d.is_finite()
            || d < 1e-3
            || !cross.is_finite()
            || cross < d * d * 0.05
            || !mw.is_finite()
            || mw < 1e-3
        {
            return Err(invalid("degenerate face landmarks"));
        }
        Ok(())
    }
    pub fn apply(&self, mesh: &mut Mesh) -> EngineResult<()> {
        self.validate()?;
        mesh.validate()?;
        if self.params == FaceParams::default() {
            return Ok(());
        }
        let a = self.landmarks5[0];
        let b = self.landmarks5[1];
        let scale = (b[0] - a[0]).hypot(b[1] - a[1]);
        let u = [(b[0] - a[0]) / scale, (b[1] - a[1]) / scale];
        let mut v = [-u[1], u[0]];
        let origin = [(a[0] + b[0]) * 0.5, (a[1] + b[1]) * 0.5];
        let mouth_world = [
            (self.landmarks5[3][0] + self.landmarks5[4][0]) * 0.5,
            (self.landmarks5[3][1] + self.landmarks5[4][1]) * 0.5,
        ];
        if (mouth_world[0] - origin[0]) * v[0] + (mouth_world[1] - origin[1]) * v[1] < 0. {
            v = [-v[0], -v[1]];
        }
        let local = |p: [f32; 2]| {
            let d = [p[0] - origin[0], p[1] - origin[1]];
            [
                (d[0] * u[0] + d[1] * u[1]) / scale,
                (d[0] * v[0] + d[1] * v[1]) / scale,
            ]
        };
        let nose = local(self.landmarks5[2]);
        let mouth = local(mouth_world);
        let corners = [local(self.landmarks5[3]), local(self.landmarks5[4])];
        let p = self.params;
        let (w, h) = mesh.grid();
        let mut next = mesh.clone();
        for y in 0..h {
            for x in 0..w {
                let i = y * w + x;
                if mesh.freeze[i] == 1. {
                    continue;
                }
                let world = [
                    x as f32 * mesh.cell_size as f32,
                    y as f32 * mesh.cell_size as f32,
                ];
                let q = local(world);
                let mut d = [0.; 2];
                // Compact-support inverse affine patches. Eye size is isotropic;
                // height/width independently change its vertical/horizontal axes.
                for eye in [[-0.5, 0.], [0.5, 0.]] {
                    add_patch(
                        &mut d,
                        q,
                        eye,
                        [0.42, 0.32],
                        [
                            p.eye_size * 0.3 + p.eye_width * 0.25,
                            p.eye_size * 0.3 + p.eye_height * 0.25,
                        ],
                        p.eye_tilt * 0.35 * if eye[0] < 0. { -1. } else { 1. },
                        [0., 0.],
                    );
                }
                add_patch(
                    &mut d,
                    q,
                    nose,
                    [0.48, 0.65],
                    [p.nose_width * 0.35, p.nose_height * 0.3],
                    0.,
                    [0., 0.],
                );
                add_patch(
                    &mut d,
                    q,
                    mouth,
                    [0.7, 0.4],
                    [p.mouth_width * 0.35, p.mouth_height * 0.35],
                    0.,
                    [0., 0.],
                );
                for corner in corners {
                    add_patch(
                        &mut d,
                        q,
                        corner,
                        [0.35, 0.35],
                        [0., 0.],
                        0.,
                        [0., p.mouth_smile * 0.15],
                    );
                }
                add_patch(
                    &mut d,
                    q,
                    [0., 0.6],
                    [1.3, 1.9],
                    [p.face_width * 0.25, 0.],
                    0.,
                    [0., 0.],
                );
                add_patch(
                    &mut d,
                    q,
                    [mouth[0], mouth[1] + 0.35],
                    [1., 0.65],
                    [p.jaw * 0.3, 0.],
                    0.,
                    [0., 0.],
                );
                add_patch(
                    &mut d,
                    q,
                    [mouth[0], mouth[1] + 0.65],
                    [0.65, 0.6],
                    [0., 0.],
                    0.,
                    [0., -p.chin * 0.25],
                );
                add_patch(
                    &mut d,
                    q,
                    [0., -0.65],
                    [1., 0.8],
                    [0., 0.],
                    0.,
                    [0., p.forehead * 0.25],
                );
                let strength = (1. - mesh.freeze[i]) * scale;
                let offset = [
                    (u[0] * d[0] + v[0] * d[1]) * strength,
                    (u[1] * d[0] + v[1] * d[1]) * strength,
                ];
                if offset == [0., 0.] {
                    continue;
                }
                let old = mesh.displacement_at(world[0] + offset[0], world[1] + offset[1]);
                next.displacement[i] = [offset[0] + old[0], offset[1] + old[1]];
            }
        }
        next.validate()?;
        *mesh = next;
        Ok(())
    }
}
fn add_patch(
    out: &mut [f32; 2],
    point: [f32; 2],
    center: [f32; 2],
    radius: [f32; 2],
    stretch: [f32; 2],
    angle: f32,
    translation: [f32; 2],
) {
    let q = [point[0] - center[0], point[1] - center[1]];
    let r2 = (q[0] / radius[0]).powi(2) + (q[1] / radius[1]).powi(2);
    if r2 >= 1. {
        return;
    }
    let weight = (1. - r2).powi(2);
    let (s, c) = (-angle).sin_cos();
    let rotated = [
        (c * q[0] - s * q[1]) / (1. + stretch[0]),
        (s * q[0] + c * q[1]) / (1. + stretch[1]),
    ];
    for k in 0..2 {
        out[k] += (rotated[k] - q[k] + translation[k]) * weight;
    }
}

pub const MESH_VERSION: u32 = 1;
const MAX_NODES: usize = 16_777_216;
fn invalid(message: &str) -> EngineError {
    EngineError::invalid("liquify", message)
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "MeshData")]
pub struct Mesh {
    pub width: u32,
    pub height: u32,
    pub cell_size: u32,
    pub displacement: Vec<[f32; 2]>,
    pub freeze: Vec<f32>,
    pub version: u32,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct MeshData {
    width: u32,
    height: u32,
    cell_size: u32,
    displacement: Vec<[f32; 2]>,
    freeze: Vec<f32>,
    version: u32,
}
impl TryFrom<MeshData> for Mesh {
    type Error = EngineError;
    fn try_from(v: MeshData) -> EngineResult<Self> {
        let mesh = Self {
            width: v.width,
            height: v.height,
            cell_size: v.cell_size,
            displacement: v.displacement,
            freeze: v.freeze,
            version: v.version,
        };
        mesh.validate()?;
        Ok(mesh)
    }
}
impl Mesh {
    pub fn new(width: u32, height: u32, cell_size: u32) -> EngineResult<Self> {
        let mut mesh = Self {
            width,
            height,
            cell_size,
            displacement: Vec::new(),
            freeze: Vec::new(),
            version: MESH_VERSION,
        };
        let n = mesh.node_count()?;
        mesh.displacement
            .try_reserve_exact(n)
            .map_err(|_| invalid("mesh allocation failed"))?;
        mesh.freeze
            .try_reserve_exact(n)
            .map_err(|_| invalid("mask allocation failed"))?;
        mesh.displacement.resize(n, [0.; 2]);
        mesh.freeze.resize(n, 0.);
        Ok(mesh)
    }
    /// Row-major grid, including the outer interpolation nodes. Invalid zero
    /// dimensions return an empty grid rather than panicking on public fields.
    pub fn grid(&self) -> (usize, usize) {
        if self.width == 0 || self.height == 0 || self.cell_size == 0 {
            return (0, 0);
        }
        (
            ((self.width - 1).div_ceil(self.cell_size) as usize) + 1,
            ((self.height - 1).div_ceil(self.cell_size) as usize) + 1,
        )
    }
    fn node_count(&self) -> EngineResult<usize> {
        let (w, h) = self.grid();
        w.checked_mul(h)
            .filter(|n| *n > 0 && *n <= MAX_NODES)
            .ok_or_else(|| invalid("invalid mesh dimensions or node limit exceeded"))
    }
    pub fn validate(&self) -> EngineResult<()> {
        let n = self.node_count()?;
        if self.version != MESH_VERSION
            || self.displacement.len() != n
            || self.freeze.len() != n
            || self.displacement.iter().flatten().any(|x| !x.is_finite())
            || self
                .freeze
                .iter()
                .any(|x| !x.is_finite() || !(0. ..=1.).contains(x))
        {
            return Err(invalid(
                "unsupported version, malformed displacement or freeze mask",
            ));
        }
        Ok(())
    }
    /// Bilinear field interpolation with clamped edges. Call validate() after
    /// directly modifying public fields; malformed buffers safely sample zero.
    pub fn displacement_at(&self, x: f32, y: f32) -> [f32; 2] {
        let (w, h) = self.grid();
        if w == 0 || h == 0 || !x.is_finite() || !y.is_finite() {
            return [0.; 2];
        }
        let gx = (x / self.cell_size as f32).clamp(0., (w - 1) as f32);
        let gy = (y / self.cell_size as f32).clamp(0., (h - 1) as f32);
        let (ix, iy) = (gx.floor() as usize, gy.floor() as usize);
        let (fx, fy) = (gx - ix as f32, gy - iy as f32);
        let mut result = [0.; 2];
        for (xx, wx) in [(ix, 1. - fx), ((ix + 1).min(w - 1), fx)] {
            for (yy, wy) in [(iy, 1. - fy), ((iy + 1).min(h - 1), fy)] {
                if let Some(d) = self.displacement.get(yy * w + xx) {
                    for c in 0..2 {
                        result[c] += d[c] * wx * wy;
                    }
                }
            }
        }
        result
    }
}
