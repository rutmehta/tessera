//! Strongly typed identifiers and digests.
//!
//! Identifiers are opaque newtypes so an `ImageId` can never be passed where a
//! `MaskId` is expected. Their serialized forms are stable and part of the
//! on-disk schema.

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::error::EngineError;

/// A 256-bit BLAKE3 digest, serialized as 64 lowercase hex characters.
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Default)]
pub struct Digest(pub [u8; 32]);

impl Digest {
    /// Digest of `bytes` under a domain-separation `context`, so equal bytes
    /// hashed for different purposes never collide.
    pub fn derive(context: &str, bytes: &[u8]) -> Self {
        let mut hasher = blake3::Hasher::new_derive_key(context);
        hasher.update(bytes);
        Self(*hasher.finalize().as_bytes())
    }

    /// Raw bytes.
    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }

    /// Lowercase hex encoding.
    pub fn to_hex(&self) -> String {
        let mut s = String::with_capacity(64);
        for b in self.0 {
            s.push(char::from_digit(u32::from(b >> 4), 16).unwrap_or('0'));
            s.push(char::from_digit(u32::from(b & 0xf), 16).unwrap_or('0'));
        }
        s
    }

    /// Short form (first 12 hex chars) for logs and UI.
    pub fn short(&self) -> String {
        self.to_hex()[..12].to_owned()
    }
}

impl fmt::Display for Digest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.to_hex())
    }
}

impl fmt::Debug for Digest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Digest({})", self.short())
    }
}

impl FromStr for Digest {
    type Err = EngineError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if s.len() != 64 || !s.is_ascii() {
            return Err(EngineError::invalid("digest", "expected 64 hex characters"));
        }
        let mut out = [0u8; 32];
        for (i, byte) in out.iter_mut().enumerate() {
            *byte = u8::from_str_radix(&s[2 * i..2 * i + 2], 16)
                .map_err(|_| EngineError::invalid("digest", "non-hex character"))?;
        }
        Ok(Self(out))
    }
}

impl Serialize for Digest {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&self.to_hex())
    }
}

impl<'de> Deserialize<'de> for Digest {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        s.parse().map_err(serde::de::Error::custom)
    }
}

/// Stable identity of an image (a master file or a virtual copy).
///
/// 128 bits, generated once when the image is first indexed and stored in its
/// sidecar so it survives moves, renames and index rebuilds. Serialized as 32
/// lowercase hex characters.
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Default)]
pub struct ImageId(pub u128);

impl fmt::Display for ImageId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:032x}", self.0)
    }
}

impl fmt::Debug for ImageId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "ImageId({self})")
    }
}

impl FromStr for ImageId {
    type Err = EngineError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if s.len() != 32 {
            return Err(EngineError::invalid(
                "image_id",
                "expected 32 hex characters",
            ));
        }
        u128::from_str_radix(s, 16)
            .map(Self)
            .map_err(|_| EngineError::invalid("image_id", "non-hex character"))
    }
}

impl Serialize for ImageId {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for ImageId {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        s.parse().map_err(serde::de::Error::custom)
    }
}

macro_rules! numeric_id {
    ($(#[$meta:meta])* $name:ident($inner:ty), $prefix:literal) => {
        $(#[$meta])*
        #[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Default, Serialize, Deserialize)]
        #[serde(transparent)]
        pub struct $name(pub $inner);

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(f, concat!($prefix, "{}"), self.0)
            }
        }

        impl fmt::Debug for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(f, concat!(stringify!($name), "({})"), self.0)
            }
        }
    };
}

numeric_id!(
    /// A local adjustment (mask + parameters) within one recipe. Allocated
    /// monotonically by the recipe and never reused.
    MaskId(u32),
    "mask#"
);
numeric_id!(
    /// A retouch operation (heal, clone, remove, skin) within one recipe.
    RetouchId(u32),
    "retouch#"
);
numeric_id!(
    /// A history entry within one recipe. Allocated monotonically; entries are
    /// never removed, so ids are never reused.
    HistoryEntryId(u64),
    "step#"
);
numeric_id!(
    /// A named group of history entries (e.g. "Agent base edit") that can be
    /// faded or toggled as a unit.
    HistoryGroupId(u32),
    "group#"
);
numeric_id!(
    /// A person (face cluster) in the library.
    PersonId(u64),
    "person#"
);
numeric_id!(
    /// A burst / near-duplicate group computed by the culler.
    SimilarityGroupId(u64),
    "similar#"
);
numeric_id!(
    /// A job submitted to a [`crate::jobs::Scheduler`].
    JobId(u64),
    "job#"
);
numeric_id!(
    /// A cancellation/bookkeeping group of jobs (typically one per image or per viewport).
    JobGroupId(u64),
    "jobs#"
);

numeric_id!(
    /// An open layered document (spec 02 §1). Session-scoped: allocated by
    /// whoever opens or creates the document, never written into a file, and
    /// a fresh id is issued whenever a document's lineage forks (a duplicate
    /// or clone), so it doubles as the document namespace of a
    /// [`crate::stage::NodeMemoKey`].
    DocumentId(u64),
    "doc#"
);
numeric_id!(
    /// A layer within one layered document. Unique within its document and
    /// never reused; nested (smart-object) documents have their own id
    /// space. `LayerId(0)` is [`LayerId::ROOT`]: the document root, never a
    /// real layer.
    LayerId(u64),
    "layer#"
);
numeric_id!(
    /// A saved selection (alpha channel) within one layered document.
    SelectionId(u64),
    "selection#"
);

impl LayerId {
    /// The document root. Real layers are numbered from 1.
    pub const ROOT: LayerId = LayerId(0);

    /// True for [`LayerId::ROOT`].
    pub const fn is_root(self) -> bool {
        self.0 == 0
    }
}

macro_rules! string_id {
    ($(#[$meta:meta])* $name:ident) => {
        $(#[$meta])*
        #[derive(Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Default, Serialize, Deserialize)]
        #[serde(transparent)]
        pub struct $name(pub String);

        impl $name {
            /// Creates the identifier from any string.
            pub fn new(s: impl Into<String>) -> Self {
                Self(s.into())
            }

            /// The identifier text.
            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(&self.0)
            }
        }

        impl fmt::Debug for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(f, concat!(stringify!($name), "({:?})"), self.0)
            }
        }

        impl From<&str> for $name {
            fn from(s: &str) -> Self {
                Self(s.to_owned())
            }
        }
    };
}

string_id!(
    /// A camera profile (DCP or native), e.g. `"native/standard"` or `"adobe/Adobe Color"`.
    ProfileId
);
string_id!(
    /// A style: preset, creative look, LUT or learned user style.
    StyleId
);
string_id!(
    /// A lens profile in the lens database.
    LensProfileId
);
string_id!(
    /// An ML model, e.g. `"segment/subject"`. Paired with a version in [`ModelRef`].
    ModelId
);

/// A specific version of an ML model. Stored with every ML-derived result
/// (masks, denoise, removals) so the result can be reproduced or deliberately
/// regenerated when the model changes.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct ModelRef {
    /// Model identifier.
    pub id: ModelId,
    /// Model version string (semver or content digest).
    pub version: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn digest_hex_round_trip() {
        let d = Digest::derive("test", b"hello");
        let hex = d.to_hex();
        assert_eq!(hex.len(), 64);
        assert_eq!(hex.parse::<Digest>().unwrap(), d);
        let json = serde_json::to_string(&d).unwrap();
        assert_eq!(serde_json::from_str::<Digest>(&json).unwrap(), d);
        assert_ne!(d, Digest::derive("other", b"hello"));
    }

    #[test]
    fn image_id_round_trip() {
        let id = ImageId(0x0123_4567_89ab_cdef_0011_2233_4455_6677);
        let json = serde_json::to_string(&id).unwrap();
        assert_eq!(json, "\"0123456789abcdef0011223344556677\"");
        assert_eq!(serde_json::from_str::<ImageId>(&json).unwrap(), id);
        assert!("xyz".parse::<ImageId>().is_err());
    }

    #[test]
    fn numeric_ids_are_transparent() {
        assert_eq!(serde_json::to_string(&MaskId(3)).unwrap(), "3");
        assert_eq!(MaskId(3).to_string(), "mask#3");
    }

    #[test]
    fn document_ids_are_transparent() {
        assert_eq!(serde_json::to_string(&DocumentId(7)).unwrap(), "7");
        assert_eq!(serde_json::to_string(&LayerId(12)).unwrap(), "12");
        assert_eq!(
            serde_json::from_str::<SelectionId>("3").unwrap(),
            SelectionId(3)
        );
        assert_eq!(DocumentId(7).to_string(), "doc#7");
        assert_eq!(LayerId(12).to_string(), "layer#12");
        assert_eq!(SelectionId(3).to_string(), "selection#3");
        assert!(LayerId::ROOT.is_root() && !LayerId(1).is_root());
        assert_eq!(LayerId::default(), LayerId::ROOT);
    }
}
