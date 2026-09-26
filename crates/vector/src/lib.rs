//! Resolution-independent vector geometry and rasterization.
mod edit;
mod interop;
pub use interop::*;
mod fill;
mod transform;
pub use edit::*;
pub use transform::*;
mod geometry;
mod render;
pub use fill::*;
pub use render::*;
mod stroke;
pub use geometry::*;
pub use stroke::*;
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("invalid vector input: {0}")]
    Invalid(&'static str),
}
pub type Result<T> = std::result::Result<T, Error>;

#[cfg(test)]
mod tests {
    #[test]
    fn rectangle_boolean_areas() {
        let a = crate::Shape::Rectangle {
            rect: crate::Rect::new(0., 0., 2., 2.),
            radii: [0.; 4],
        }
        .path()
        .unwrap();
        let b = crate::Shape::Rectangle {
            rect: crate::Rect::new(1., 0., 3., 2.),
            radii: [0.; 4],
        }
        .path()
        .unwrap();
        for (op, area) in [
            (crate::Operation::Combine, 6.),
            (crate::Operation::Subtract, 2.),
            (crate::Operation::Intersect, 2.),
            (crate::Operation::Exclude, 4.),
        ] {
            assert!((a.boolean(&b, op, 0.0001).unwrap().area(0.0001).unwrap() - area).abs() < 1e-9);
        }
    }
}
