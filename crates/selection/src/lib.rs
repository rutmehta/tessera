//! Pixel selection tools (spec 02 §3).
//!
//! Selections are soft single-channel masks. Algorithms run on the dense
//! [`Mask`]/[`Image`] types; [`Mask::to_raster`]/[`Mask::from_raster`]
//! convert to the compositor's F32 single-channel [`Raster`] selections
//! (`DocState::selection`) and [`channels::AlphaChannels`] persists them as
//! named alpha channels.
//!
//! | Tool | Module |
//! |---|---|
//! | Rectangular / elliptical / single row & column marquee | [`marquee`] |
//! | Lasso, polygonal lasso, magnetic lasso (live-wire) | [`lasso`] |
//! | Magic wand | [`wand`] |
//! | Quick selection (seeded region growing) | [`quick`] |
//! | Colour range, tonal ranges, skin tones | [`range`] |
//! | Focus area | [`focus`] |
//! | Object / subject / sky (ml-segment) | [`ml`] |
//! | Boolean ops, transform, grow/contract/border/smooth/feather | [`ops`] |
//! | Select and Mask refinement, decontaminate colours | [`refine`] |
//! | Marching-ants outlines | [`contour`] |
//! | engine-api `set_pixel_selection` | [`api`] |
//!
//! [`Raster`]: compositor::Raster
#![forbid(unsafe_code)]
#![warn(missing_docs)]
#![allow(clippy::needless_range_loop, clippy::too_many_arguments)]

pub mod api;
pub mod channels;
pub mod contour;
pub mod filter;
pub mod focus;
pub mod lasso;
pub mod marquee;
mod mask;
pub mod ml;
pub mod ops;
pub mod quick;
pub mod range;
pub mod refine;
pub mod wand;

pub use mask::{Image, Mask};
pub use ops::Combine;
