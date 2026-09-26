use crate::{Error, Result};
use lyon_path::{Path, math::point};
use serde::{Deserialize, Serialize};

/// Stable wire commands, independent of lyon's private storage format.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "command", content = "points", rename_all = "snake_case")]
pub enum PathCommand {
    Move([f32; 2]),
    Line([f32; 2]),
    Quadratic([[f32; 2]; 2]),
    Cubic([[f32; 2]; 3]),
    Close,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PathText {
    pub commands: Vec<PathCommand>,
    pub offset: f32,
}
impl PathText {
    pub fn to_lyon(&self) -> Result<Path> {
        if !self.offset.is_finite() || self.commands.len() > 100_000 {
            return Err(Error::Invalid("invalid path offset or command budget"));
        }
        let mut builder = Path::builder();
        let mut begun = false;
        let mut open = false;
        for command in &self.commands {
            let coordinates: &[[f32; 2]] = match command {
                PathCommand::Move(p) | PathCommand::Line(p) => std::slice::from_ref(p),
                PathCommand::Quadratic(p) => p,
                PathCommand::Cubic(p) => p,
                PathCommand::Close => &[],
            };
            if coordinates
                .iter()
                .flatten()
                .any(|v| !v.is_finite() || v.abs() > 1_000_000.0)
            {
                return Err(Error::Invalid("invalid path coordinate"));
            }
            if !open && !matches!(command, PathCommand::Move(_)) {
                return Err(Error::Invalid("path command outside a contour"));
            }
            match command {
                PathCommand::Move(p) => {
                    if begun {
                        return Err(Error::Invalid("path must have one contour"));
                    }
                    builder.begin(point(p[0], p[1]));
                    begun = true;
                    open = true;
                }
                PathCommand::Line(p) => {
                    builder.line_to(point(p[0], p[1]));
                }
                PathCommand::Quadratic([c, p]) => {
                    builder.quadratic_bezier_to(point(c[0], c[1]), point(p[0], p[1]));
                }
                PathCommand::Cubic([a, b, p]) => {
                    builder.cubic_bezier_to(
                        point(a[0], a[1]),
                        point(b[0], b[1]),
                        point(p[0], p[1]),
                    );
                }
                PathCommand::Close => {
                    builder.end(true);
                    open = false;
                }
            }
        }
        if open {
            builder.end(false);
        }
        if !begun {
            return Err(Error::Invalid("empty path"));
        }
        Ok(builder.build())
    }
}
