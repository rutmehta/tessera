//! Stroke input, smoothing (pulled string with catch-up) and dab spacing.

/// One pointer sample, level-0 canvas pixels.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct InputPoint {
    /// X.
    pub x: f32,
    /// Y.
    pub y: f32,
    /// Pressure `0..=1` (1 without a tablet).
    pub pressure: f32,
    /// Tilt `[x, y]`, each `-1..=1`.
    pub tilt: [f32; 2],
    /// Barrel rotation, radians.
    pub rotation: f32,
    /// Timestamp, seconds.
    pub time: f64,
}

impl InputPoint {
    /// Full pressure, no tilt, time 0.
    pub fn at(x: f32, y: f32) -> Self {
        Self {
            x,
            y,
            pressure: 1.0,
            tilt: [0.0; 2],
            rotation: 0.0,
            time: 0.0,
        }
    }

    /// With pressure.
    pub fn pressure(mut self, p: f32) -> Self {
        self.pressure = p;
        self
    }

    /// With timestamp.
    pub fn time(mut self, t: f64) -> Self {
        self.time = t;
        self
    }
}

/// Stroke smoothing ("pulled string" stabiliser).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Smoothing {
    /// String length in pixels; the brush only moves once the pointer is
    /// farther than this. 0 disables smoothing.
    pub string_length: f32,
    /// On stroke end, move the brush to the last pointer position.
    pub catch_up: bool,
}

impl Default for Smoothing {
    fn default() -> Self {
        Self {
            string_length: 0.0,
            catch_up: true,
        }
    }
}

/// Pulled-string stabiliser state.
#[derive(Debug, Clone)]
pub(crate) struct Smoother {
    cfg: Smoothing,
    brush: Option<InputPoint>,
    last_raw: Option<InputPoint>,
}

impl Smoother {
    pub(crate) fn new(cfg: Smoothing) -> Self {
        Self {
            cfg,
            brush: None,
            last_raw: None,
        }
    }

    /// Feeds a pointer sample; returns the new brush position if it moved.
    pub(crate) fn push(&mut self, p: InputPoint) -> Option<InputPoint> {
        self.last_raw = Some(p);
        let l = self.cfg.string_length;
        let Some(b) = self.brush else {
            self.brush = Some(p);
            return Some(p);
        };
        if l <= 0.0 {
            self.brush = Some(p);
            return Some(p);
        }
        let d = (p.x - b.x).hypot(p.y - b.y);
        if d <= l {
            return None;
        }
        let k = (d - l) / d;
        let nb = InputPoint {
            x: b.x + (p.x - b.x) * k,
            y: b.y + (p.y - b.y) * k,
            ..p
        };
        self.brush = Some(nb);
        Some(nb)
    }

    /// Stroke end: the catch-up point, if any.
    pub(crate) fn finish(&mut self) -> Option<InputPoint> {
        let (b, r) = (self.brush?, self.last_raw?);
        if self.cfg.catch_up && self.cfg.string_length > 0.0 && (b.x != r.x || b.y != r.y) {
            self.brush = Some(r);
            Some(r)
        } else {
            None
        }
    }
}

/// A point on the smoothed path with derived sensors.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct PathPoint {
    /// X.
    pub x: f32,
    /// Y.
    pub y: f32,
    /// Pressure.
    pub pressure: f32,
    /// Tilt.
    pub tilt: [f32; 2],
    /// Rotation.
    pub rotation: f32,
    /// Speed px/s.
    pub velocity: f32,
    /// Direction of travel, radians.
    pub direction: f32,
}

impl PathPoint {
    fn lerp(a: &PathPoint, b: &PathPoint, t: f32, direction: f32) -> PathPoint {
        let l = |u: f32, v: f32| u + (v - u) * t;
        PathPoint {
            x: l(a.x, b.x),
            y: l(a.y, b.y),
            pressure: l(a.pressure, b.pressure),
            tilt: [l(a.tilt[0], b.tilt[0]), l(a.tilt[1], b.tilt[1])],
            rotation: l(a.rotation, b.rotation),
            velocity: l(a.velocity, b.velocity),
            direction,
        }
    }
}

/// Smallest distance between dabs, pixels.
pub const MIN_STEP: f32 = 0.5;

/// Places dabs at arc-length intervals along a polyline, carrying the
/// remainder across segments so spacing is independent of how the path
/// was sampled.
#[derive(Debug, Clone, Default)]
pub(crate) struct Spacer {
    last: Option<PathPoint>,
    acc: f32,
}

impl Spacer {
    /// Extends the path to `p`, appending dab centres to `out`. `step`
    /// gives the spacing at a path point. The first point always gets a dab.
    pub(crate) fn advance(
        &mut self,
        p: PathPoint,
        step: &dyn Fn(&PathPoint) -> f32,
        out: &mut Vec<PathPoint>,
        limit: usize,
    ) {
        let Some(a) = self.last else {
            out.push(p);
            self.last = Some(p);
            self.acc = 0.0;
            return;
        };
        let len = (p.x - a.x).hypot(p.y - a.y);
        if len <= 1e-6 {
            self.last = Some(PathPoint {
                direction: a.direction,
                ..p
            });
            return;
        }
        let dir = (p.y - a.y).atan2(p.x - a.x);
        let mut s = 0.0f32;
        while out.len() < limit {
            let here = PathPoint::lerp(&a, &p, s / len, dir);
            let st = step(&here).max(MIN_STEP);
            let need = st - self.acc;
            if s + need > len + 1e-4 {
                self.acc += len - s;
                break;
            }
            s += need.max(0.0);
            self.acc = 0.0;
            out.push(PathPoint::lerp(&a, &p, (s / len).min(1.0), dir));
        }
        self.last = Some(PathPoint {
            direction: dir,
            ..p
        });
    }

    /// The last path point.
    pub(crate) fn last(&self) -> Option<PathPoint> {
        self.last
    }
}
