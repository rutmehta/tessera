//! Non-destructive planar premultiplied RGBA transformations.
pub mod adaptive;
pub mod displacement;
pub mod free;
pub mod op;
pub mod perspective;
pub mod puppet;
pub mod sample;
pub mod seam;
pub mod vanishing;
pub mod warp;
pub use op::{Operation, TransformOp};
pub use sample::Kernel;
pub type Point = [f64; 2];
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("invalid transform: {0}")]
    Invalid(String),
}
pub type Result<T> = std::result::Result<T, Error>;
#[derive(Clone, Debug, PartialEq)]
pub struct Image {
    pub width: usize,
    pub height: usize,
    pub planes: [Vec<f32>; 4],
}
impl Image {
    pub fn new(width: usize, height: usize, planes: [Vec<f32>; 4]) -> Result<Self> {
        let image = Self {
            width,
            height,
            planes,
        };
        image.validate()?;
        Ok(image)
    }
    /// Revalidate public fields before rendering; callers may construct or mutate them directly.
    pub fn validate(&self) -> Result<()> {
        if self.width == 0
            || self.height == 0
            || self.width.checked_mul(self.height).is_none_or(|n| {
                n > 100_000_000
                    || self
                        .planes
                        .iter()
                        .any(|p| p.len() != n || p.iter().any(|v| !v.is_finite()))
            })
        {
            return Err(Error::Invalid(
                "nonempty finite planar RGBA required (maximum 100 MP)".into(),
            ));
        }
        Ok(())
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rejects_malformed_image() {
        assert!(Image::new(2, 2, std::array::from_fn(|_| vec![0.; 3])).is_err());
    }
}
