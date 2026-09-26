//! Dab planning: smoothing → spacing → dynamics → symmetry.

use crate::dynamics::Sensors;
use crate::engine::Brush;
use crate::rng::Rng;
use crate::stroke::{InputPoint, PathPoint, Smoother, Spacer};
use crate::symmetry::Iso;

/// Hard cap on dabs per stroke (guards pathological spacing/input).
pub const MAX_DABS: usize = 4_000_000;

/// One stamp of the tip.
#[derive(Debug, Clone, PartialEq)]
pub struct Dab {
    /// Centre x, canvas pixels.
    pub x: f32,
    /// Centre y.
    pub y: f32,
    /// Diameter, pixels.
    pub size: f32,
    /// Angle, radians.
    pub angle: f32,
    /// Roundness `0.01..=1`.
    pub roundness: f32,
    /// Flow (per-dab alpha) `0..=1`.
    pub flow: f32,
    /// Opacity cap `0..=1`.
    pub opacity: f32,
    /// Horizontal tip flip.
    pub flip_x: bool,
    /// Vertical tip flip.
    pub flip_y: bool,
    /// Dual-brush stamp centres (canvas).
    pub dual: Vec<[f32; 2]>,
    /// Index of the spacing step that produced this dab.
    pub index: u64,
}

impl Dab {
    /// The stamp pose.
    pub fn pose(&self) -> crate::tip::Pose {
        crate::tip::Pose {
            size: self.size,
            angle: self.angle,
            roundness: self.roundness,
            flip_x: self.flip_x,
            flip_y: self.flip_y,
        }
    }

    fn transformed(&self, iso: &Iso) -> Dab {
        let (x, y) = iso.point(self.x, self.y);
        Dab {
            x,
            y,
            angle: iso.angle(self.angle),
            flip_y: self.flip_y ^ iso.mirrored(),
            dual: self
                .dual
                .iter()
                .map(|p| {
                    let (x, y) = iso.point(p[0], p[1]);
                    [x, y]
                })
                .collect(),
            ..self.clone()
        }
    }
}

/// Turns pointer samples into dabs. Deterministic for a given seed.
#[derive(Debug, Clone)]
pub struct Planner {
    brush: Brush,
    smoother: Smoother,
    spacer: Spacer,
    isos: Vec<Iso>,
    rng: Rng,
    index: u64,
    prev: Option<(InputPoint, f32)>,
    air_carry: f32,
    emitted: usize,
}

impl Planner {
    /// A planner for one stroke.
    pub fn new(brush: &Brush, seed: u64) -> Self {
        Self {
            smoother: Smoother::new(brush.smoothing),
            spacer: Spacer::default(),
            isos: brush.symmetry.transforms(),
            rng: Rng::new(seed),
            index: 0,
            prev: None,
            air_carry: 0.0,
            emitted: 0,
            brush: brush.clone(),
        }
    }

    /// Feeds a pointer sample.
    pub fn push(&mut self, p: InputPoint) -> Vec<Dab> {
        match self.smoother.push(p) {
            Some(q) => self.path_to(q),
            None => Vec::new(),
        }
    }

    /// Ends the stroke (smoothing catch-up).
    pub fn finish(&mut self) -> Vec<Dab> {
        match self.smoother.finish() {
            Some(q) => self.path_to(q),
            None => Vec::new(),
        }
    }

    /// Airbrush: `dt` seconds with the pen held still emit
    /// `rate · dt` dabs at the current position.
    pub fn tick(&mut self, dt: f32) -> Vec<Dab> {
        let rate = self.brush.airbrush.unwrap_or(0.0);
        let Some(p) = self.spacer.last() else {
            return Vec::new();
        };
        if rate <= 0.0 || dt <= 0.0 {
            return Vec::new();
        }
        self.air_carry += rate * dt;
        let n = self.air_carry.floor();
        self.air_carry -= n;
        let mut out = Vec::new();
        for _ in 0..n as usize {
            self.emit(&p, &mut out);
        }
        out
    }

    fn path_to(&mut self, q: InputPoint) -> Vec<Dab> {
        let velocity = match self.prev {
            Some((a, v)) => {
                let dt = (q.time - a.time) as f32;
                if dt > 0.0 {
                    let inst = (q.x - a.x).hypot(q.y - a.y) / dt;
                    0.5 * v + 0.5 * inst
                } else {
                    v
                }
            }
            None => 0.0,
        };
        self.prev = Some((q, velocity));
        let pp = PathPoint {
            x: q.x,
            y: q.y,
            pressure: q.pressure.clamp(0.0, 1.0),
            tilt: q.tilt,
            rotation: q.rotation,
            velocity,
            direction: self.spacer.last().map_or(0.0, |l| l.direction),
        };
        let (size, spacing, dynamics) = (self.brush.size, self.brush.spacing, self.brush.dynamics);
        let step = move |p: &PathPoint| {
            let s = Sensors {
                pressure: p.pressure,
                velocity: p.velocity,
                tilt: p.tilt,
                rotation: p.rotation,
                direction: p.direction,
                index: 0,
            };
            spacing.max(0.01) * size * dynamics.size.controlled(&s)
        };
        let mut centres = Vec::new();
        let limit = MAX_DABS.saturating_sub(self.emitted);
        self.spacer.advance(pp, &step, &mut centres, limit);
        let mut out = Vec::new();
        for c in &centres {
            self.emit(c, &mut out);
        }
        out
    }

    fn emit(&mut self, p: &PathPoint, out: &mut Vec<Dab>) {
        let b = &self.brush;
        let d = &b.dynamics;
        let s = Sensors {
            pressure: p.pressure,
            velocity: p.velocity,
            tilt: p.tilt,
            rotation: p.rotation,
            direction: p.direction,
            index: self.index,
        };
        let r_count = self.rng.unit();
        let n = ((d.count.max(1) as f32) * (1.0 - d.count_jitter.clamp(0.0, 1.0) * r_count))
            .round()
            .max(1.0) as u32;
        for _ in 0..n {
            let r: [f32; 9] = std::array::from_fn(|_| self.rng.unit());
            let size = b.size * d.size.factor(&s, r[0]);
            let angle = b.tip.angle
                + d.control_angle(&s)
                + d.angle_jitter.clamp(0.0, 1.0) * std::f32::consts::PI * (2.0 * r[1] - 1.0);
            let roundness = (b.tip.roundness * d.roundness.factor(&s, r[2])).clamp(0.01, 1.0);
            let flow = (b.flow * d.flow.factor(&s, r[3])).clamp(0.0, 1.0);
            let opacity = (b.opacity * d.opacity.factor(&s, r[4])).clamp(0.0, 1.0);
            let (mut x, mut y) = (p.x, p.y);
            if d.scatter > 0.0 {
                let (sn, cs) = p.direction.sin_cos();
                let across = d.scatter * size * (2.0 * r[5] - 1.0);
                x += -sn * across;
                y += cs * across;
                if d.scatter_both_axes {
                    let along = d.scatter * size * (2.0 * r[6] - 1.0);
                    x += cs * along;
                    y += sn * along;
                }
            }
            let mut dual = Vec::new();
            if let Some(db) = &b.dual {
                for _ in 0..db.count.max(1) {
                    let (u, v) = (self.rng.signed(), self.rng.signed());
                    dual.push([x + db.scatter * size * u, y + db.scatter * size * v]);
                }
            }
            let dab = Dab {
                x,
                y,
                size,
                angle,
                roundness,
                flow,
                opacity,
                flip_x: d.flip_x_jitter && r[7] < 0.5,
                flip_y: d.flip_y_jitter && r[8] < 0.5,
                dual,
                index: self.index,
            };
            for iso in &self.isos {
                if self.emitted >= MAX_DABS {
                    return;
                }
                out.push(dab.transformed(iso));
                self.emitted += 1;
            }
        }
        self.index += 1;
    }
}
