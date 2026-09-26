//! Synchronous engine tool console and MCP transport.
pub mod actions;
mod catalog;
mod console;
mod delta;
pub mod documents;
mod documents_console;
mod exports;
mod mutations;
mod people;
mod people_ids;
mod pixels;
mod reads;
pub mod schema;
mod server;
pub use console::Console;
pub use image_core::resident::OutputMetrics;
mod preview;
pub use reads::{Comparison, Metrics};
pub use server::Server;

pub(crate) fn unsupported(what: impl Into<String>) -> engine_api::EngineError {
    engine_api::EngineError::Unsupported { what: what.into() }
}
