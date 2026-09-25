//! The single error type used across engine crate boundaries.
//!
//! Crates may use richer private errors internally, but anything that crosses
//! a crate boundary, the scheduler, the tool API or the FFI layer is an
//! [`EngineError`]. It is `Clone` and serializable so it can be cached with a
//! failed job, sent across threads, and returned verbatim to MCP clients.

use serde::{Deserialize, Serialize};

/// Convenience alias used by every fallible contract in the engine.
pub type EngineResult<T> = Result<T, EngineError>;

/// Every failure the engine reports across a crate boundary.
///
/// Serialized as `{"code": "<snake_case_variant>", ...fields}` so tool clients
/// can branch on `code` without parsing messages.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error, Serialize, Deserialize)]
#[serde(tag = "code", rename_all = "snake_case")]
pub enum EngineError {
    /// A caller supplied an argument outside its documented domain.
    #[error("invalid argument `{name}`: {reason}")]
    InvalidArgument {
        /// Parameter or field name.
        name: String,
        /// Human-readable explanation.
        reason: String,
    },

    /// A referenced entity (image, mask, snapshot, profile, job…) does not exist.
    #[error("{kind} not found: {id}")]
    NotFound {
        /// Entity kind, e.g. `"image"`, `"mask"`, `"snapshot"`.
        kind: String,
        /// The identifier that failed to resolve, as displayed.
        id: String,
    },

    /// The operation is valid but not supported by this build, camera, format or backend.
    #[error("unsupported: {what}")]
    Unsupported {
        /// What is unsupported.
        what: String,
    },

    /// The work was cancelled through its [`crate::jobs::CancellationToken`].
    /// Not a failure: callers should drop the result silently.
    #[error("cancelled")]
    Cancelled,

    /// Filesystem or OS I/O failure.
    #[error("i/o error{}: {message}", path.as_deref().map(|p| format!(" at {p}")).unwrap_or_default())]
    Io {
        /// Path involved, when known.
        path: Option<String>,
        /// OS error text.
        message: String,
    },

    /// A codec or container could not be decoded (raw, JPEG, XMP, sidecar JSON…).
    #[error("decode error ({format}): {message}")]
    Decode {
        /// Format or container, e.g. `"CR3"`, `"xmp"`, `"recipe-json"`.
        format: String,
        /// Details.
        message: String,
    },

    /// Encoding output failed.
    #[error("encode error ({format}): {message}")]
    Encode {
        /// Target format.
        format: String,
        /// Details.
        message: String,
    },

    /// A document was written by a newer schema than this build understands
    /// in a way `#[serde(default)]` cannot absorb.
    #[error("{document} schema version {found} is newer than supported {supported}")]
    SchemaVersion {
        /// Which document (`"recipe"`, `"library"`…).
        document: String,
        /// Version found on disk.
        found: u32,
        /// Highest version this build reads.
        supported: u32,
    },

    /// Colour management failure (bad ICC profile, singular matrix, CMM error).
    #[error("colour management: {message}")]
    Color {
        /// Details.
        message: String,
    },

    /// GPU device, allocation or kernel failure.
    #[error("gpu: {message}")]
    Gpu {
        /// Details.
        message: String,
    },

    /// ML model load or inference failure.
    #[error("model `{model}`: {message}")]
    Model {
        /// Model identifier.
        model: String,
        /// Details.
        message: String,
    },

    /// A bounded resource (tile cache, VRAM, queue) is exhausted.
    #[error("resource exhausted: {resource}")]
    ResourceExhausted {
        /// Which resource.
        resource: String,
    },

    /// A concurrent writer changed the target since it was read (sidecar
    /// vector-clock conflict, stale recipe hash on a tool call…).
    #[error("conflict: {message}")]
    Conflict {
        /// Details.
        message: String,
    },

    /// An invariant was violated inside the engine. Always a bug.
    #[error("internal error: {message}")]
    Internal {
        /// Details.
        message: String,
    },
}

impl EngineError {
    /// Shorthand for [`EngineError::InvalidArgument`].
    pub fn invalid(name: impl Into<String>, reason: impl Into<String>) -> Self {
        Self::InvalidArgument {
            name: name.into(),
            reason: reason.into(),
        }
    }

    /// Shorthand for [`EngineError::NotFound`].
    pub fn not_found(kind: impl Into<String>, id: impl std::fmt::Display) -> Self {
        Self::NotFound {
            kind: kind.into(),
            id: id.to_string(),
        }
    }

    /// Shorthand for [`EngineError::Internal`].
    pub fn internal(message: impl Into<String>) -> Self {
        Self::Internal {
            message: message.into(),
        }
    }

    /// Wraps an I/O error with the path it concerns.
    pub fn io_at(path: impl AsRef<std::path::Path>, err: &std::io::Error) -> Self {
        Self::Io {
            path: Some(path.as_ref().display().to_string()),
            message: err.to_string(),
        }
    }

    /// True for [`EngineError::Cancelled`]; schedulers use this to avoid
    /// logging cancellations as failures.
    pub fn is_cancelled(&self) -> bool {
        matches!(self, Self::Cancelled)
    }
}

impl From<std::io::Error> for EngineError {
    fn from(err: std::io::Error) -> Self {
        Self::Io {
            path: None,
            message: err.to_string(),
        }
    }
}

impl From<serde_json::Error> for EngineError {
    fn from(err: serde_json::Error) -> Self {
        Self::Decode {
            format: "json".into(),
            message: err.to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serializes_with_code_tag() {
        let err = EngineError::not_found("mask", 7);
        let json = serde_json::to_value(&err).unwrap();
        assert_eq!(json["code"], "not_found");
        assert_eq!(json["kind"], "mask");
        let back: EngineError = serde_json::from_value(json).unwrap();
        assert_eq!(back, err);
    }

    #[test]
    fn display_is_human_readable() {
        let err = EngineError::Io {
            path: Some("/a/b".into()),
            message: "denied".into(),
        };
        assert_eq!(err.to_string(), "i/o error at /a/b: denied");
        assert_eq!(EngineError::Cancelled.to_string(), "cancelled");
        assert!(EngineError::Cancelled.is_cancelled());
    }
}
