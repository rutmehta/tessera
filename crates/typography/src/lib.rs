//! Editable text engine.
mod geometry;
mod interop;
pub use geometry::TextPath;
pub use interop::*;
mod layout;
mod model;
mod path_data;
pub use path_data::*;
mod render;
pub use layout::*;
pub use model::*;
pub use render::*;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("{0}")]
    Invalid(&'static str),
    #[error("{0}")]
    Json(#[from] serde_json::Error),
    #[error("font unavailable: {0}")]
    MissingFont(String),
    #[error("invalid font")]
    Font,
    #[error("vertical text is not implemented")]
    UnsupportedVertical,
}
pub type Result<T> = std::result::Result<T, Error>;
