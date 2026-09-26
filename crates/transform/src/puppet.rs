//! Deterministic grid-triangulated alpha-shape puppet deformation.
use crate::{Error, Point, Result};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum PuppetDensity {
    Sparse,
    Normal,
    Dense,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum PuppetMode {
    Normal,
    Rigid,
}
/// A hard vertex position constraint and optional absolute rest-relative rotation in radians.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PuppetPin {
    pub vertex: usize,
    pub target: Point,
    pub rotation: Option<f64>,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PuppetWarp {
    pub rest_vertices: Vec<Point>,
    pub triangles: Vec<[usize; 3]>,
    pub pins: Vec<PuppetPin>,
    pub density: PuppetDensity,
    pub expansion: u32,
    pub mode: PuppetMode,
    pub iterations: usize,
}
#[derive(Clone, Debug, PartialEq)]
pub struct PreparedPuppetWarp {
    pub vertices: Vec<Point>,
    pub rest_vertices: Vec<Point>,
    pub triangles: Vec<[usize; 3]>,
}
fn invalid(message: &str) -> Error {
    Error::Invalid(message.into())
}
fn sub(a: Point, b: Point) -> Point {
    [a[0] - b[0], a[1] - b[1]]
}
fn cross(a: Point, b: Point) -> f64 {
    a[0] * b[1] - a[1] * b[0]
}
impl PuppetWarp {
    /// Alpha is row-major u8; any nonzero value belongs to the shape.
    /// Density uses 8/4/2-pixel cells, conservatively covering occupied cells.
    /// Expansion is square (Chebyshev) dilation, clipped to image bounds.
    pub fn from_alpha(
        alpha: &[u8],
        width: usize,
        height: usize,
        density: PuppetDensity,
        expansion: u32,
    ) -> Result<Self> {
        if width == 0
            || height == 0
            || width.checked_mul(height) != Some(alpha.len())
            || alpha.len() > 16_777_216
            || expansion > 64
        {
            return Err(invalid(
                "invalid alpha dimensions or expansion (maximum 64)",
            ));
        }
        let step = match density {
            PuppetDensity::Sparse => 8,
            PuppetDensity::Normal => 4,
            PuppetDensity::Dense => 2,
        };
        // Summed-area occupancy makes dilation independent of expansion radius.
        let stride = width + 1;
        let mut sum = vec![0u32; stride * (height + 1)];
        for y in 0..height {
            let mut row = 0;
            for x in 0..width {
                row += u32::from(alpha[y * width + x] != 0);
                sum[(y + 1) * stride + x + 1] = sum[y * stride + x + 1] + row;
            }
        }
        let mut rest_vertices = Vec::new();
        let mut triangles = Vec::new();
        let mut ids = BTreeMap::new();
        let e = expansion as usize;
        for y in (0..height).step_by(step) {
            for x in (0..width).step_by(step) {
                let right = (x + step).min(width);
                let bottom = (y + step).min(height);
                let x0 = x.saturating_sub(e);
                let y0 = y.saturating_sub(e);
                let x1 = (right + e).min(width);
                let y1 = (bottom + e).min(height);
                let occupied = sum[y1 * stride + x1] + sum[y0 * stride + x0]
                    - sum[y0 * stride + x1]
                    - sum[y1 * stride + x0];
                if occupied == 0 {
                    continue;
                }
                let mut quad = [0; 4];
                for (i, p) in [(x, y), (right, y), (right, bottom), (x, bottom)]
                    .into_iter()
                    .enumerate()
                {
                    quad[i] = *ids.entry(p).or_insert_with(|| {
                        let id = rest_vertices.len();
                        rest_vertices.push([p.0 as f64, p.1 as f64]);
                        id
                    });
                }
                if rest_vertices.len() > 16384 {
                    return Err(invalid(
                        "puppet mesh exceeds 16384 vertices; reduce density",
                    ));
                }
                triangles.push([quad[0], quad[1], quad[2]]);
                triangles.push([quad[0], quad[2], quad[3]]);
            }
        }
        let warp = Self {
            rest_vertices,
            triangles,
            pins: Vec::new(),
            density,
            expansion,
            mode: PuppetMode::Normal,
            iterations: 20,
        };
        warp.validate()?;
        Ok(warp)
    }
    pub fn validate(&self) -> Result<()> {
        if self.rest_vertices.is_empty()
            || self.rest_vertices.len() > 16384
            || self.triangles.is_empty()
            || self.triangles.len() > 32768
            || self.iterations == 0
            || self.iterations > 100
            || self.expansion > 64
        {
            return Err(invalid("invalid puppet mesh size or parameters"));
        }
        if self
            .rest_vertices
            .iter()
            .flatten()
            .any(|v| !v.is_finite() || v.abs() > 1e9)
        {
            return Err(invalid("invalid puppet coordinate"));
        }
        for t in &self.triangles {
            if t.iter().any(|&v| v >= self.rest_vertices.len()) {
                return Err(invalid("invalid triangle index"));
            }
            if cross(
                sub(self.rest_vertices[t[1]], self.rest_vertices[t[0]]),
                sub(self.rest_vertices[t[2]], self.rest_vertices[t[0]]),
            )
            .abs()
                < 1e-12
            {
                return Err(invalid("degenerate rest triangle"));
            }
        }
        let mut seen = BTreeSet::new();
        for p in &self.pins {
            if p.vertex >= self.rest_vertices.len()
                || !seen.insert(p.vertex)
                || p.target.iter().any(|v| !v.is_finite() || v.abs() > 1e9)
                || p.rotation.is_some_and(|r| !r.is_finite())
            {
                return Err(invalid("invalid or duplicate puppet pin"));
            }
        }
        Ok(())
    }
    /// Uniform positive-weight ARAP: local polar rotations, global constrained
    /// Laplacian solved by conjugate gradients. Unpinned components stay at rest.
    /// Rigid mode couples local rotations within each connected component; hard
    /// position pins still take precedence, so incompatible pins can stretch it.
    pub fn solve(&self) -> Result<PreparedPuppetWarp> {
        self.validate()?;
        let n = self.rest_vertices.len();
        let mut vertices = self.rest_vertices.clone();
        if !self.pins.is_empty() {
            let mut edges = BTreeSet::new();
            for &[a, b, c] in &self.triangles {
                for (i, j) in [(a, b), (b, c), (c, a)] {
                    edges.insert((i.min(j), i.max(j)));
                }
            }
            let mut neighbors = vec![Vec::new(); n];
            for &(i, j) in &edges {
                neighbors[i].push(j);
                neighbors[j].push(i);
            }
            let mut fixed = vec![false; n];
            let mut prescribed = vec![None; n];
            for pin in &self.pins {
                fixed[pin.vertex] = true;
                prescribed[pin.vertex] = pin.rotation;
            }
            let mut visited = vec![false; n];
            let mut components = Vec::new();
            for root in 0..n {
                if visited[root] {
                    continue;
                }
                visited[root] = true;
                let mut component = vec![root];
                let mut k = 0;
                while k < component.len() {
                    let i = component[k];
                    for &j in &neighbors[i] {
                        if !visited[j] {
                            visited[j] = true;
                            component.push(j);
                        }
                    }
                    k += 1;
                }
                component.sort_unstable();
                if let Some(pin) = self
                    .pins
                    .iter()
                    .filter(|p| component.binary_search(&p.vertex).is_ok())
                    .min_by_key(|p| p.vertex)
                {
                    let angle = pin.rotation.unwrap_or(0.0);
                    let (s, c) = angle.sin_cos();
                    for &i in &component {
                        let p = sub(self.rest_vertices[i], self.rest_vertices[pin.vertex]);
                        vertices[i] = [
                            pin.target[0] + c * p[0] - s * p[1],
                            pin.target[1] + s * p[0] + c * p[1],
                        ];
                    }
                } else {
                    for &i in &component {
                        fixed[i] = true;
                    }
                }
                components.push(component);
            }
            for pin in &self.pins {
                vertices[pin.vertex] = pin.target;
            }
            let mut rotations = vec![[1.0, 0.0]; n];
            for _ in 0..self.iterations {
                for i in 0..n {
                    let mut dot = 0.0;
                    let mut wedge = 0.0;
                    for &j in &neighbors[i] {
                        let p = sub(self.rest_vertices[i], self.rest_vertices[j]);
                        let q = sub(vertices[i], vertices[j]);
                        dot += p[0] * q[0] + p[1] * q[1];
                        wedge += cross(p, q);
                    }
                    let angle = prescribed[i].unwrap_or_else(|| wedge.atan2(dot));
                    let (s, c) = angle.sin_cos();
                    rotations[i] = [c, s];
                }
                if self.mode == PuppetMode::Rigid {
                    for component in &components {
                        let explicit: Vec<_> =
                            component.iter().filter_map(|&i| prescribed[i]).collect();
                        let mut r = [0.0, 0.0];
                        if explicit.is_empty() {
                            for &i in component {
                                r[0] += rotations[i][0];
                                r[1] += rotations[i][1];
                            }
                        } else {
                            for angle in explicit {
                                let (s, c) = angle.sin_cos();
                                r[0] += c;
                                r[1] += s;
                            }
                        }
                        let norm = r[0].hypot(r[1]);
                        if norm > 1e-12 {
                            r[0] /= norm;
                            r[1] /= norm;
                        } else {
                            r = [1.0, 0.0];
                        }
                        for &i in component {
                            if prescribed[i].is_none() {
                                rotations[i] = r;
                            }
                        }
                    }
                }
                let mut rhs = vec![[0.0; 2]; n];
                for &(i, j) in &edges {
                    let p = sub(self.rest_vertices[i], self.rest_vertices[j]);
                    let c = (rotations[i][0] + rotations[j][0]) * 0.5;
                    let s = (rotations[i][1] + rotations[j][1]) * 0.5;
                    let r = [c * p[0] - s * p[1], s * p[0] + c * p[1]];
                    for axis in 0..2 {
                        rhs[i][axis] += r[axis];
                        rhs[j][axis] -= r[axis];
                    }
                }
                for axis in 0..2 {
                    let mut b = vec![0.0; n];
                    let mut x = vec![0.0; n];
                    for i in 0..n {
                        if !fixed[i] {
                            b[i] = rhs[i][axis];
                            x[i] = vertices[i][axis];
                            for &j in &neighbors[i] {
                                if fixed[j] {
                                    b[i] += vertices[j][axis];
                                }
                            }
                        }
                    }
                    solve_laplacian(&neighbors, &fixed, &b, &mut x)?;
                    for i in 0..n {
                        if !fixed[i] {
                            vertices[i][axis] = x[i];
                        }
                    }
                }
            }
        }
        if vertices.iter().flatten().any(|x| !x.is_finite()) {
            return Err(invalid("nonfinite ARAP solution"));
        }
        Ok(PreparedPuppetWarp {
            vertices,
            rest_vertices: self.rest_vertices.clone(),
            triangles: self.triangles.clone(),
        })
    }
}
// Matrix-free CG on the free-vertex SPD principal Laplacian. A component
// either has a hard pin or is entirely fixed, removing translation nullspaces.
fn solve_laplacian(
    neighbors: &[Vec<usize>],
    fixed: &[bool],
    b: &[f64],
    x: &mut [f64],
) -> Result<()> {
    let n = x.len();
    let apply = |v: &[f64], out: &mut [f64]| {
        for i in 0..n {
            out[i] = if fixed[i] {
                0.0
            } else {
                neighbors[i].len() as f64 * v[i]
                    - neighbors[i]
                        .iter()
                        .filter(|&&j| !fixed[j])
                        .map(|&j| v[j])
                        .sum::<f64>()
            };
        }
    };
    let mut ax = vec![0.0; n];
    apply(x, &mut ax);
    let mut r: Vec<_> = b.iter().zip(ax).map(|(b, a)| b - a).collect();
    let mut p = r.clone();
    let mut rr = r.iter().map(|v| v * v).sum::<f64>();
    let tolerance = 1e-20 * b.iter().map(|v| v * v).sum::<f64>().max(1.0);
    let mut ap = vec![0.0; n];
    for _ in 0..n.min(1024) {
        if rr <= tolerance {
            return Ok(());
        }
        apply(&p, &mut ap);
        let denominator = p.iter().zip(&ap).map(|(p, a)| p * a).sum::<f64>();
        if !denominator.is_finite() || denominator <= 0.0 {
            return Err(invalid("ARAP linear solve failed"));
        }
        let alpha = rr / denominator;
        for i in 0..n {
            x[i] += alpha * p[i];
            r[i] -= alpha * ap[i];
        }
        let next = r.iter().map(|v| v * v).sum::<f64>();
        let beta = next / rr;
        for i in 0..n {
            p[i] = r[i] + beta * p[i];
        }
        rr = next;
    }
    if rr <= tolerance {
        Ok(())
    } else {
        Err(invalid(
            "ARAP linear solve did not converge within 1024 steps",
        ))
    }
}
impl PreparedPuppetWarp {
    /// Destination-to-source barycentric lookup. Outside the deformed mesh is transparent.
    /// In an overlap, the first triangle in stable mesh order wins.
    pub fn inverse_map(&self, point: Point) -> Option<Point> {
        if point.iter().any(|x| !x.is_finite()) {
            return None;
        }
        for t in &self.triangles {
            if t.iter()
                .any(|&i| i >= self.vertices.len() || i >= self.rest_vertices.len())
            {
                continue;
            }
            let a = self.vertices[t[0]];
            let b = sub(self.vertices[t[1]], a);
            let c = sub(self.vertices[t[2]], a);
            let det = cross(b, c);
            if det.abs() < 1e-12 {
                continue;
            }
            let d = sub(point, a);
            let v = cross(d, c) / det;
            let w = cross(b, d) / det;
            let u = 1.0 - v - w;
            if u >= -1e-9 && v >= -1e-9 && w >= -1e-9 {
                let a = self.rest_vertices[t[0]];
                let b = self.rest_vertices[t[1]];
                let c = self.rest_vertices[t[2]];
                return Some([
                    u * a[0] + v * b[0] + w * c[0],
                    u * a[1] + v * b[1] + w * c[1],
                ]);
            }
        }
        None
    }
}
