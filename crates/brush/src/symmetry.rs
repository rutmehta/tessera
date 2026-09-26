//! Paint symmetry: every dab is replayed through a group of isometries.

/// A 2-D isometry `p' = M·p + t`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Iso {
    /// Row-major linear part `[a, b, c, d]`: `x' = a·x + b·y`, `y' = c·x + d·y`.
    pub m: [f32; 4],
    /// Translation.
    pub t: [f32; 2],
}

impl Iso {
    /// Identity.
    pub const IDENTITY: Iso = Iso {
        m: [1.0, 0.0, 0.0, 1.0],
        t: [0.0, 0.0],
    };

    /// Maps a point.
    pub fn point(&self, x: f32, y: f32) -> (f32, f32) {
        (
            self.m[0] * x + self.m[1] * y + self.t[0],
            self.m[2] * x + self.m[3] * y + self.t[1],
        )
    }

    /// Maps a direction angle.
    pub fn angle(&self, a: f32) -> f32 {
        let (s, c) = a.sin_cos();
        (self.m[2] * c + self.m[3] * s).atan2(self.m[0] * c + self.m[1] * s)
    }

    /// True for reflections (orientation-reversing).
    pub fn mirrored(&self) -> bool {
        self.m[0] * self.m[3] - self.m[1] * self.m[2] < 0.0
    }

    fn about(cx: f32, cy: f32, m: [f32; 4]) -> Iso {
        // p' = M(p − c) + c
        Iso {
            m,
            t: [cx - (m[0] * cx + m[1] * cy), cy - (m[2] * cx + m[3] * cy)],
        }
    }
}

/// Symmetry mode (spec 02 §4 "Paint Symmetry").
#[derive(serde::Serialize, serde::Deserialize, Debug, Clone, Copy, PartialEq, Default)]
pub enum Symmetry {
    /// Off.
    #[default]
    None,
    /// Mirror across the vertical line `x`.
    Vertical {
        /// Axis.
        x: f32,
    },
    /// Mirror across the horizontal line `y`.
    Horizontal {
        /// Axis.
        y: f32,
    },
    /// Both axes (4 copies).
    Dual {
        /// Vertical axis.
        x: f32,
        /// Horizontal axis.
        y: f32,
    },
    /// Mirror across the diagonal through `(cx, cy)` (x ↔ y).
    Diagonal {
        /// Centre x.
        cx: f32,
        /// Centre y.
        cy: f32,
    },
    /// `count` rotated copies about a centre.
    Radial {
        /// Centre x.
        cx: f32,
        /// Centre y.
        cy: f32,
        /// Segments (≥ 1).
        count: u32,
    },
    /// Radial with a mirror in every segment (`2 × count` copies).
    Mandala {
        /// Centre x.
        cx: f32,
        /// Centre y.
        cy: f32,
        /// Segments (≥ 1).
        count: u32,
    },
}

impl Symmetry {
    /// The group elements, identity first.
    pub fn transforms(&self) -> Vec<Iso> {
        let rot = |a: f32| {
            let (s, c) = a.sin_cos();
            [c, -s, s, c]
        };
        match *self {
            Symmetry::None => vec![Iso::IDENTITY],
            Symmetry::Vertical { x } => {
                vec![Iso::IDENTITY, Iso::about(x, 0.0, [-1.0, 0.0, 0.0, 1.0])]
            }
            Symmetry::Horizontal { y } => {
                vec![Iso::IDENTITY, Iso::about(0.0, y, [1.0, 0.0, 0.0, -1.0])]
            }
            Symmetry::Dual { x, y } => vec![
                Iso::IDENTITY,
                Iso::about(x, y, [-1.0, 0.0, 0.0, 1.0]),
                Iso::about(x, y, [1.0, 0.0, 0.0, -1.0]),
                Iso::about(x, y, [-1.0, 0.0, 0.0, -1.0]),
            ],
            Symmetry::Diagonal { cx, cy } => {
                vec![Iso::IDENTITY, Iso::about(cx, cy, [0.0, 1.0, 1.0, 0.0])]
            }
            Symmetry::Radial { cx, cy, count } => (0..count.max(1))
                .map(|k| {
                    Iso::about(
                        cx,
                        cy,
                        rot(std::f32::consts::TAU * k as f32 / count.max(1) as f32),
                    )
                })
                .collect(),
            Symmetry::Mandala { cx, cy, count } => {
                let n = count.max(1);
                let mut v = Vec::with_capacity(2 * n as usize);
                for k in 0..n {
                    let r = rot(std::f32::consts::TAU * k as f32 / n as f32);
                    v.push(Iso::about(cx, cy, r));
                    // Rotation ∘ mirror across the vertical axis.
                    v.push(Iso::about(cx, cy, [-r[0], r[1], -r[2], r[3]]));
                }
                v
            }
        }
    }
}
