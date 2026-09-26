//! Brush dynamics: per-dab variation driven by pen input and a seeded RNG.

/// Speed (px/s) at which the velocity control reads 0.5.
pub const VELOCITY_REF: f32 = 1000.0;

/// Per-dab sensor readings.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Sensors {
    /// Pressure `0..=1`.
    pub pressure: f32,
    /// Pointer speed, px/s.
    pub velocity: f32,
    /// Tilt `[x, y]`, each `-1..=1`.
    pub tilt: [f32; 2],
    /// Barrel rotation, radians.
    pub rotation: f32,
    /// Stroke direction, radians.
    pub direction: f32,
    /// Index of the dab along the stroke.
    pub index: u64,
}

/// What drives a dynamic parameter.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Control {
    /// Constant 1.
    #[default]
    Off,
    /// Pen pressure.
    Pressure,
    /// Slower strokes read higher: `1 / (1 + v / VELOCITY_REF)`.
    Velocity,
    /// Tilt magnitude.
    Tilt,
    /// Linear fade to 0 over this many dabs.
    Fade(u32),
}

impl Control {
    /// Control value `0..=1`.
    pub fn value(&self, s: &Sensors) -> f32 {
        match *self {
            Control::Off => 1.0,
            Control::Pressure => s.pressure.clamp(0.0, 1.0),
            Control::Velocity => 1.0 / (1.0 + s.velocity.max(0.0) / VELOCITY_REF),
            Control::Tilt => s.tilt[0].hypot(s.tilt[1]).clamp(0.0, 1.0),
            Control::Fade(n) => {
                if n == 0 {
                    1.0
                } else {
                    (1.0 - s.index as f32 / n as f32).clamp(0.0, 1.0)
                }
            }
        }
    }
}

/// A controlled, jittered multiplier in `[minimum, 1]`.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Jitter {
    /// Random reduction `0..=1`.
    pub jitter: f32,
    /// Sensor.
    pub control: Control,
    /// Floor of the controlled value `0..=1`.
    pub minimum: f32,
}

impl Jitter {
    /// Controlled value without jitter.
    pub fn controlled(&self, s: &Sensors) -> f32 {
        let m = self.minimum.clamp(0.0, 1.0);
        m + (1.0 - m) * self.control.value(s)
    }

    /// Controlled value reduced by `jitter · r` (`r` uniform `[0, 1)`), not
    /// below the minimum when one is set.
    pub fn factor(&self, s: &Sensors, r: f32) -> f32 {
        let v = self.controlled(s) * (1.0 - self.jitter.clamp(0.0, 1.0) * r);
        v.max(self.minimum.clamp(0.0, 1.0)).clamp(0.0, 1.0)
    }
}

/// What sets the dab angle in addition to the tip angle.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AngleControl {
    /// Tip angle only.
    #[default]
    Off,
    /// Follow the stroke direction.
    Direction,
    /// Pressure × 360°.
    Pressure,
    /// Tilt direction.
    Tilt,
    /// Barrel rotation.
    Rotation,
}

/// Shape dynamics, scattering and transfer (spec 02 §4).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Dynamics {
    /// Size multiplier.
    pub size: Jitter,
    /// Angle jitter `0..=1` (1 = ±180°).
    pub angle_jitter: f32,
    /// Angle control.
    pub angle_control: AngleControl,
    /// Roundness multiplier.
    pub roundness: Jitter,
    /// Flow multiplier (transfer).
    pub flow: Jitter,
    /// Opacity multiplier (transfer).
    pub opacity: Jitter,
    /// Scatter as a multiple of the diameter.
    pub scatter: f32,
    /// Scatter along the stroke too (otherwise perpendicular only).
    pub scatter_both_axes: bool,
    /// Dabs per spacing step (≥ 1).
    pub count: u32,
    /// Random reduction of `count`, `0..=1`.
    pub count_jitter: f32,
    /// Random horizontal flip.
    pub flip_x_jitter: bool,
    /// Random vertical flip.
    pub flip_y_jitter: bool,
}

impl Default for Dynamics {
    fn default() -> Self {
        Self {
            size: Jitter::default(),
            angle_jitter: 0.0,
            angle_control: AngleControl::Off,
            roundness: Jitter::default(),
            flow: Jitter::default(),
            opacity: Jitter::default(),
            scatter: 0.0,
            scatter_both_axes: false,
            count: 1,
            count_jitter: 0.0,
            flip_x_jitter: false,
            flip_y_jitter: false,
        }
    }
}

impl Dynamics {
    /// Angle contributed by the control, radians.
    pub fn control_angle(&self, s: &Sensors) -> f32 {
        match self.angle_control {
            AngleControl::Off => 0.0,
            AngleControl::Direction => s.direction,
            AngleControl::Pressure => s.pressure * std::f32::consts::TAU,
            AngleControl::Tilt => {
                if s.tilt == [0.0, 0.0] {
                    0.0
                } else {
                    s.tilt[1].atan2(s.tilt[0])
                }
            }
            AngleControl::Rotation => s.rotation,
        }
    }
}
