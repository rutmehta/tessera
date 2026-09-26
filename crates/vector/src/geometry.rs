use crate::{Error, Result};
use i_overlay::{
    core::{fill_rule::FillRule as Rule, overlay_rule::OverlayRule},
    float::single::SingleFloatOverlay,
};
pub use kurbo::{Affine, Point, Rect, Vec2};
use kurbo::{BezPath, PathEl, Shape as KurboShape};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum FillRule {
    EvenOdd,
    #[default]
    NonZero,
}
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Anchor {
    pub point: Point,
    pub incoming: Point,
    pub outgoing: Point,
}
impl Anchor {
    pub fn corner(point: Point) -> Self {
        Self {
            point,
            incoming: point,
            outgoing: point,
        }
    }
}
#[derive(Clone, Debug, PartialEq)]
pub struct Subpath {
    pub anchors: Vec<Anchor>,
    pub closed: bool,
}
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Path {
    pub subpaths: Vec<Subpath>,
    pub fill_rule: FillRule,
}
#[derive(Clone, Copy, Debug)]
pub enum Operation {
    Combine,
    Subtract,
    Intersect,
    Exclude,
}
impl Path {
    pub fn polyline(points: &[Point], closed: bool) -> Self {
        Self {
            subpaths: vec![Subpath {
                anchors: points.iter().copied().map(Anchor::corner).collect(),
                closed,
            }],
            ..Self::default()
        }
    }
    pub fn to_bez(&self) -> BezPath {
        let mut b = BezPath::new();
        for s in &self.subpaths {
            if let Some(first) = s.anchors.first() {
                b.move_to(first.point);
                for pair in s.anchors.windows(2) {
                    b.curve_to(pair[0].outgoing, pair[1].incoming, pair[1].point);
                }
                if s.closed {
                    let last = s.anchors.last().unwrap();
                    b.curve_to(last.outgoing, first.incoming, first.point);
                    b.close_path();
                }
            }
        }
        b
    }
    pub fn from_bez(b: &BezPath) -> Self {
        let mut path = Self::default();
        let mut s = Subpath {
            anchors: vec![],
            closed: false,
        };
        for el in b.elements() {
            match *el {
                PathEl::MoveTo(p) => {
                    if !s.anchors.is_empty() {
                        path.subpaths.push(s);
                    }
                    s = Subpath {
                        anchors: vec![Anchor::corner(p)],
                        closed: false,
                    };
                }
                PathEl::LineTo(p) => s.anchors.push(Anchor::corner(p)),
                PathEl::CurveTo(a, b, p) => {
                    if let Some(last) = s.anchors.last_mut() {
                        last.outgoing = a;
                    }
                    s.anchors.push(Anchor {
                        point: p,
                        incoming: b,
                        outgoing: p,
                    });
                }
                PathEl::QuadTo(q, p) => {
                    if let Some(last) = s.anchors.last_mut() {
                        last.outgoing = last.point + (q - last.point) * (2. / 3.);
                    }
                    s.anchors.push(Anchor {
                        point: p,
                        incoming: p + (q - p) * (2. / 3.),
                        outgoing: p,
                    });
                }
                PathEl::ClosePath => {
                    s.closed = true;
                    if s.anchors.len() > 1 && s.anchors.last().unwrap().point == s.anchors[0].point
                    {
                        let last = s.anchors.pop().unwrap();
                        s.anchors[0].incoming = last.incoming;
                    }
                }
            }
        }
        if !s.anchors.is_empty() {
            path.subpaths.push(s);
        }
        path
    }
    pub fn validate(&self) -> Result<()> {
        if self.subpaths.iter().flat_map(|s| &s.anchors).any(|a| {
            [a.point, a.incoming, a.outgoing]
                .iter()
                .any(|p| !p.x.is_finite() || !p.y.is_finite())
        }) {
            return Err(Error::Invalid("nonfinite path"));
        }
        Ok(())
    }
    pub fn flattened(&self, tolerance: f64) -> Result<Vec<(Vec<Point>, bool)>> {
        self.validate()?;
        if !tolerance.is_finite() || tolerance <= 0. {
            return Err(Error::Invalid("tolerance"));
        }
        let mut out = vec![];
        let mut points = vec![];
        kurbo::flatten(
            self.to_bez().elements().iter().copied(),
            tolerance,
            |el| match el {
                PathEl::MoveTo(p) => {
                    if !points.is_empty() {
                        out.push((std::mem::take(&mut points), false));
                    }
                    points.push(p);
                }
                PathEl::LineTo(p) => points.push(p),
                PathEl::ClosePath => {
                    if points.len() > 1 && points.first() == points.last() {
                        points.pop();
                    }
                    out.push((std::mem::take(&mut points), true));
                }
                _ => unreachable!(),
            },
        );
        if !points.is_empty() {
            out.push((points, false));
        }
        Ok(out)
    }
    fn contours(&self, tolerance: f64) -> Result<Vec<Vec<[f64; 2]>>> {
        Ok(self
            .flattened(tolerance)?
            .into_iter()
            .filter(|(p, _)| p.len() >= 3)
            .map(|(p, _)| p.iter().map(|p| [p.x, p.y]).collect())
            .collect())
    }
    fn normalized(&self, tolerance: f64) -> Result<Vec<Vec<[f64; 2]>>> {
        let c = self.contours(tolerance)?;
        let empty: Vec<Vec<[f64; 2]>> = vec![];
        Ok(c.overlay(
            &empty,
            OverlayRule::Union,
            match self.fill_rule {
                FillRule::EvenOdd => Rule::EvenOdd,
                FillRule::NonZero => Rule::NonZero,
            },
        )
        .into_iter()
        .flatten()
        .collect())
    }
    pub fn boolean(&self, other: &Self, op: Operation, tolerance: f64) -> Result<Self> {
        let a = self.normalized(tolerance)?;
        let b = other.normalized(tolerance)?;
        let shapes = a.overlay(
            &b,
            match op {
                Operation::Combine => OverlayRule::Union,
                Operation::Subtract => OverlayRule::Difference,
                Operation::Intersect => OverlayRule::Intersect,
                Operation::Exclude => OverlayRule::Xor,
            },
            Rule::NonZero,
        );
        Ok(Self {
            subpaths: shapes
                .into_iter()
                .flatten()
                .map(|c| Subpath {
                    anchors: c
                        .into_iter()
                        .map(|p| Anchor::corner(Point::new(p[0], p[1])))
                        .collect(),
                    closed: true,
                })
                .collect(),
            fill_rule: FillRule::NonZero,
        })
    }
    pub fn area(&self, tolerance: f64) -> Result<f64> {
        Ok(self
            .normalized(tolerance)?
            .iter()
            .map(|p| {
                p.iter()
                    .zip(p.iter().cycle().skip(1))
                    .take(p.len())
                    .map(|(a, b)| a[0] * b[1] - a[1] * b[0])
                    .sum::<f64>()
                    / 2.
            })
            .sum::<f64>()
            .abs())
    }
    pub fn bounds(&self) -> Rect {
        self.to_bez().bounding_box()
    }
    pub fn contains(&self, p: Point) -> bool {
        let w = self.to_bez().winding(p);
        match self.fill_rule {
            FillRule::EvenOdd => w % 2 != 0,
            FillRule::NonZero => w != 0,
        }
    }
    pub fn affine(&self, a: Affine) -> Self {
        Self::from_bez(&(a * self.to_bez())).with_rule(self.fill_rule)
    }
    pub fn with_rule(mut self, rule: FillRule) -> Self {
        self.fill_rule = rule;
        self
    }
}

#[derive(Clone, Debug)]
pub enum Shape {
    Rectangle {
        rect: Rect,
        radii: [f64; 4],
    },
    Ellipse {
        center: Point,
        radii: Vec2,
    },
    Polygon {
        center: Point,
        radius: f64,
        sides: u32,
        rotation: f64,
        inner_radius: Option<f64>,
    },
    Line {
        start: Point,
        end: Point,
    },
    Custom(Path),
}
impl Shape {
    pub fn path(&self) -> Result<Path> {
        let b = match self {
            Self::Rectangle { rect, radii } => {
                if rect.width() < 0.
                    || rect.height() < 0.
                    || radii.iter().any(|r| !r.is_finite() || *r < 0.)
                {
                    return Err(Error::Invalid("rectangle"));
                }
                let mut r = *radii;
                let mut scale = 1.0_f64;
                for (size, sum) in [
                    (rect.width(), r[0] + r[1]),
                    (rect.width(), r[2] + r[3]),
                    (rect.height(), r[0] + r[3]),
                    (rect.height(), r[1] + r[2]),
                ] {
                    if sum > 0. {
                        scale = scale.min(size / sum);
                    }
                }
                r.iter_mut().for_each(|v| *v *= scale);
                let [tl, tr, br, bl] = r;
                let k = 0.5522847498307936;
                let (x, y, u, v) = (rect.x0, rect.y0, rect.x1, rect.y1);
                let mut b = BezPath::new();
                b.move_to((x + tl, y));
                b.line_to((u - tr, y));
                b.curve_to((u - tr + k * tr, y), (u, y + tr - k * tr), (u, y + tr));
                b.line_to((u, v - br));
                b.curve_to((u, v - br + k * br), (u - br + k * br, v), (u - br, v));
                b.line_to((x + bl, v));
                b.curve_to((x + bl - k * bl, v), (x, v - bl + k * bl), (x, v - bl));
                b.line_to((x, y + tl));
                b.curve_to((x, y + tl - k * tl), (x + tl - k * tl, y), (x + tl, y));
                b.close_path();
                b
            }
            Self::Ellipse { center, radii } => {
                if radii.x < 0.
                    || radii.y < 0.
                    || ![center.x, center.y, radii.x, radii.y]
                        .iter()
                        .all(|v| v.is_finite())
                {
                    return Err(Error::Invalid("ellipse"));
                }
                kurbo::Ellipse::new(*center, *radii, 0.).to_path(1e-6)
            }
            Self::Polygon {
                center,
                radius,
                sides,
                rotation,
                inner_radius,
            } => {
                if *sides < 3
                    || *sides > 10000
                    || !radius.is_finite()
                    || *radius < 0.
                    || inner_radius.is_some_and(|v| !v.is_finite() || v < 0. || v > *radius)
                {
                    return Err(Error::Invalid("polygon"));
                }
                let n = if inner_radius.is_some() {
                    sides * 2
                } else {
                    *sides
                };
                let points: Vec<_> = (0..n)
                    .map(|i| {
                        let a = rotation + std::f64::consts::TAU * f64::from(i) / f64::from(n);
                        let r = if i % 2 == 1 {
                            inner_radius.unwrap_or(*radius)
                        } else {
                            *radius
                        };
                        *center + Vec2::new(a.cos() * r, a.sin() * r)
                    })
                    .collect();
                let path = Path::polyline(&points, true);
                path.validate()?;
                return Ok(path);
            }
            Self::Line { start, end } => {
                let path = Path::polyline(&[*start, *end], false);
                path.validate()?;
                return Ok(path);
            }
            Self::Custom(p) => {
                p.validate()?;
                return Ok(p.clone());
            }
        };
        let p = Path::from_bez(&b);
        p.validate()?;
        Ok(p)
    }
}
