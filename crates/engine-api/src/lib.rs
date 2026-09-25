//! Shared contracts for the photo engine.
//!
//! Every engine crate (decode, pipeline, cache, catalog, culling, ML, tool
//! server, apps) links against this crate and nothing here depends on any of
//! them. The crate holds types, traits and the small amount of logic needed
//! to make those types trustworthy (hashing, history replay, colour-matrix
//! algebra). See `CONTRACTS.md` for the invariants implementers must keep.
//!
//! Module map:
//! - [`tile`]: tiled planar buffers, tile addressing and the [`tile::Pyramid`] trait.
//! - [`color`]: working spaces, 3×3 colour matrices, white points, illuminants, ICC handles.
//! - [`stage`]: the fixed pipeline stage order, stage parameter hashing, memo keys.
//! - [`recipe`]: the per-image edit document (settings, selection, history) and the `crs:` table.
//! - [`jobs`]: job/scheduler traits, priority classes, cancellation.
//! - [`tools`]: the typed tool API (commands and results) behind scripting and MCP.
//! - [`id`]: strongly typed identifiers shared by all of the above.
//! - [`error`]: the single [`EngineError`] type.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

pub mod color;
pub mod error;
pub mod id;
pub mod jobs;
pub mod recipe;
pub mod stage;
pub mod tile;
pub mod tools;

pub use error::{EngineError, EngineResult};

/// Version of the contracts in this crate. Bumped on any breaking change to a
/// public type or serialized schema; work packages record the version they
/// were built against.
pub const CONTRACT_VERSION: &str = "1.0.0";
