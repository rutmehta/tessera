//! Synchronous engine tool console and MCP transport.
mod catalog;
mod console;
mod delta;
mod exports;
mod mutations;
mod pixels;
mod reads;
pub mod schema;
mod server;
pub use console::Console;
mod preview;
pub use reads::{Comparison, Metrics};
pub use server::Server;

pub(crate) fn unsupported(what: impl Into<String>) -> engine_api::EngineError {
    engine_api::EngineError::Unsupported { what: what.into() }
}
