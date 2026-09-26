use crate::*;
use kurbo::{ParamCurve, ParamCurveNearest};
#[derive(Clone, Copy, Debug)]
pub enum Handle {
    Incoming,
    Outgoing,
}
impl Path {
    fn anchor_mut(&mut self, s: usize, a: usize) -> Result<&mut Anchor> {
        self.subpaths
            .get_mut(s)
            .and_then(|s| s.anchors.get_mut(a))
            .ok_or(Error::Invalid("anchor index"))
    }
    pub fn move_anchor(&mut self, s: usize, a: usize, to: Point) -> Result<()> {
        if !to.x.is_finite() || !to.y.is_finite() {
            return Err(Error::Invalid("anchor point"));
        }
        let anchor = self.anchor_mut(s, a)?;
        let d = to - anchor.point;
        anchor.point = to;
        anchor.incoming += d;
        anchor.outgoing += d;
        Ok(())
    }
    pub fn set_handle(
        &mut self,
        s: usize,
        a: usize,
        handle: Handle,
        to: Point,
        mirror: bool,
    ) -> Result<()> {
        if !to.x.is_finite() || !to.y.is_finite() {
            return Err(Error::Invalid("handle"));
        }
        let a = self.anchor_mut(s, a)?;
        let opposite = a.point - (to - a.point);
        match handle {
            Handle::Incoming => {
                a.incoming = to;
                if mirror {
                    a.outgoing = opposite;
                }
            }
            Handle::Outgoing => {
                a.outgoing = to;
                if mirror {
                    a.incoming = opposite;
                }
            }
        }
        Ok(())
    }
    pub fn split_segment(&mut self, s: usize, index: usize, t: f64) -> Result<()> {
        if !(0.0..1.0).contains(&t) || t == 0. {
            return Err(Error::Invalid("split parameter"));
        }
        let s = self
            .subpaths
            .get_mut(s)
            .ok_or(Error::Invalid("subpath index"))?;
        let n = s.anchors.len();
        if n < 2 || index >= n || (!s.closed && index == n - 1) {
            return Err(Error::Invalid("segment index"));
        }
        let next = (index + 1) % n;
        let a = s.anchors[index];
        let b = s.anchors[next];
        let c = kurbo::CubicBez::new(a.point, a.outgoing, b.incoming, b.point);
        let l = c.subsegment(0.0..t);
        let r = c.subsegment(t..1.0);
        s.anchors[index].outgoing = l.p1;
        s.anchors[next].incoming = r.p2;
        s.anchors.insert(
            index + 1,
            Anchor {
                point: l.p3,
                incoming: l.p2,
                outgoing: r.p1,
            },
        );
        Ok(())
    }
    pub fn delete_anchor(&mut self, s: usize, a: usize) -> Result<Anchor> {
        self.anchor_mut(s, a)?;
        Ok(self.subpaths[s].anchors.remove(a))
    }
    pub fn hit_anchor(&self, p: Point, radius: f64) -> Option<(usize, usize)> {
        self.subpaths
            .iter()
            .enumerate()
            .flat_map(|(s, sub)| {
                sub.anchors
                    .iter()
                    .enumerate()
                    .map(move |(a, anchor)| (s, a, anchor.point.distance(p)))
            })
            .filter(|(_, _, d)| *d <= radius)
            .min_by(|a, b| a.2.total_cmp(&b.2))
            .map(|(s, a, _)| (s, a))
    }
    /// Returns subpath, segment and the cubic parameter at the nearest hit.
    pub fn hit_path(&self, p: Point, radius: f64) -> Option<(usize, usize, f64)> {
        let mut best = None;
        let mut distance = radius * radius;
        if radius < 0. || !radius.is_finite() {
            return None;
        }
        for (si, s) in self.subpaths.iter().enumerate() {
            let n = if s.closed {
                s.anchors.len()
            } else {
                s.anchors.len().saturating_sub(1)
            };
            for i in 0..n {
                let a = s.anchors[i];
                let b = s.anchors[(i + 1) % s.anchors.len()];
                let near =
                    kurbo::CubicBez::new(a.point, a.outgoing, b.incoming, b.point).nearest(p, 1e-6);
                if near.distance_sq <= distance {
                    distance = near.distance_sq;
                    best = Some((si, i, near.t));
                }
            }
        }
        best
    }
    /// Uniform Catmull-Rom interpolating pen, represented as editable cubics.
    pub fn curvature_pen(points: &[Point], closed: bool) -> Result<Self> {
        let mut p = Self::polyline(points, closed);
        p.validate()?;
        let n = points.len();
        if n < 2 {
            return Ok(p);
        }
        for i in 0..n {
            let before = if i > 0 {
                points[i - 1]
            } else if closed {
                points[n - 1]
            } else {
                points[0]
            };
            let after = if i + 1 < n {
                points[i + 1]
            } else if closed {
                points[0]
            } else {
                points[n - 1]
            };
            let tangent = (after - before) / 6.;
            p.subpaths[0].anchors[i].incoming = points[i] - tangent;
            p.subpaths[0].anchors[i].outgoing = points[i] + tangent;
        }
        Ok(p)
    }
}
