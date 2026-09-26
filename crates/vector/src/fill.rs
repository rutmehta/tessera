use crate::*;
pub type Color = [f32; 4];
fn valid_color(c: Color) -> bool {
    c.iter().all(|v| v.is_finite() && (0.0..=1.0).contains(v))
}
#[derive(Clone, Copy, Debug)]
pub enum GradientKind {
    Linear,
    Radial,
    Angle,
    Reflected,
    Diamond,
}
#[derive(Clone, Copy, Debug)]
pub struct Stop {
    pub position: f64,
    pub color: Color,
}
#[derive(Clone, Debug)]
pub struct Gradient {
    kind: GradientKind,
    start: Point,
    end: Point,
    stops: Vec<Stop>,
    dither: bool,
}
impl Gradient {
    pub fn new(
        kind: GradientKind,
        start: Point,
        end: Point,
        stops: Vec<Stop>,
        dither: bool,
    ) -> Result<Self> {
        if ![start.x, start.y, end.x, end.y]
            .iter()
            .all(|x| x.is_finite())
            || start.distance(end) <= f64::EPSILON
            || stops.len() < 2
            || stops.iter().any(|s| {
                !s.position.is_finite()
                    || !(0.0..=1.0).contains(&s.position)
                    || !valid_color(s.color)
            })
            || stops.windows(2).any(|w| w[0].position >= w[1].position)
        {
            return Err(Error::Invalid("gradient"));
        }
        Ok(Self {
            kind,
            start,
            end,
            stops,
            dither,
        })
    }
    pub fn sample(&self, p: Point) -> Color {
        let axis = self.end - self.start;
        let d = p - self.start;
        let x = d.dot(axis) / axis.hypot2();
        let y = axis.cross(d) / axis.hypot2();
        let t = match self.kind {
            GradientKind::Linear => x,
            GradientKind::Radial => x.hypot(y),
            GradientKind::Angle => {
                y.atan2(x).rem_euclid(std::f64::consts::TAU) / std::f64::consts::TAU
            }
            GradientKind::Reflected => x.abs(),
            GradientKind::Diamond => x.abs() + y.abs(),
        };
        if t <= self.stops[0].position {
            return self.stops[0].color;
        }
        if t >= self.stops.last().unwrap().position {
            return self.stops.last().unwrap().color;
        }
        let index = self.stops.partition_point(|s| s.position < t);
        let a = self.stops[index - 1];
        let b = self.stops[index];
        let u = ((t - a.position) / (b.position - a.position)) as f32;
        let mut color = std::array::from_fn(|i| a.color[i] + (b.color[i] - a.color[i]) * u);
        if self.dither && u > 0. && u < 1. {
            // Stable document-space hash; do not perturb endpoints or alpha.
            let hash =
                p.x.to_bits().wrapping_mul(0x9e3779b97f4a7c15) ^ p.y.to_bits().rotate_left(29);
            let hash = hash.wrapping_mul(0xbf58476d1ce4e5b9);
            let noise = ((hash >> 40) as f32 / 16777215. - 0.5) / 255.;
            for c in &mut color[..3] {
                *c = (*c + noise).clamp(0., 1.);
            }
        }
        color
    }
}
#[derive(Clone, Debug)]
pub struct Pattern {
    width: u32,
    height: u32,
    pixels: Vec<Color>,
    document_to_tile: Affine,
}
impl Pattern {
    pub fn new(
        width: u32,
        height: u32,
        pixels: Vec<Color>,
        tile_to_document: Affine,
    ) -> Result<Self> {
        if width == 0
            || height == 0
            || u64::from(width) * u64::from(height) != pixels.len() as u64
            || pixels.iter().any(|p| !valid_color(*p))
            || !tile_to_document.as_coeffs().iter().all(|v| v.is_finite())
            || tile_to_document.determinant().abs() < 1e-12
        {
            return Err(Error::Invalid("pattern"));
        }
        Ok(Self {
            width,
            height,
            pixels,
            document_to_tile: tile_to_document.inverse(),
        })
    }
    pub fn sample(&self, p: Point) -> Color {
        let p = self.document_to_tile * p;
        let x = p.x.floor().rem_euclid(f64::from(self.width)) as usize;
        let y = p.y.floor().rem_euclid(f64::from(self.height)) as usize;
        self.pixels[y * self.width as usize + x]
    }
}
#[derive(Clone, Debug)]
pub enum Fill {
    Solid(Color),
    Gradient(Gradient),
    Pattern(Pattern),
}
impl Fill {
    pub fn sample(&self, p: Point) -> Color {
        match self {
            Self::Solid(c) => *c,
            Self::Gradient(g) => g.sample(p),
            Self::Pattern(t) => t.sample(p),
        }
    }
    pub(crate) fn validate(&self) -> Result<()> {
        if matches!(self,Self::Solid(c) if !valid_color(*c)) {
            Err(Error::Invalid("color"))
        } else {
            Ok(())
        }
    }
}
