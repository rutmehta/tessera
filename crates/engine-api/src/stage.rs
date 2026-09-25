//! Pipeline stages, parameter hashing and memoization keys.
//!
//! The raw pipeline runs a fixed sequence of stages (spec 04 §3). Each stage
//! owns one parameter struct in the recipe; its [`ParamHash`] is chained with
//! the hashes of every upstream stage so that a stage's cache key changes
//! whenever anything that feeds it changes, and never otherwise.

use std::fmt;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::id::{Digest, ImageId};
use crate::tile::TileCoord;

/// Pipeline stages in execution order. The discriminant is the position.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[repr(u8)]
pub enum StageId {
    /// Container parse and raw unpack to CFA samples.
    Decode = 0,
    /// Black/white levels, linearisation tables, highlight reconstruction.
    Linearize = 1,
    /// Raw-domain denoise (classical or neural, possibly joint with demosaic).
    Denoise = 2,
    /// CFA → RGB.
    Demosaic = 3,
    /// Lens corrections that must precede colour: CA, vignetting, softness.
    Lens = 4,
    /// Camera profile (DCP matrices, HueSatMap, LookTable) into the working space.
    CameraProfile = 5,
    /// White balance via chromatic adaptation.
    WhiteBalance = 6,
    /// Capture sharpening and detail-stage noise reduction.
    Detail = 7,
    /// Exposure, tone controls, local contrast, curves, display transform.
    Tone = 8,
    /// Vibrance/saturation, HSL, colour grading, point colour, LUTs.
    Color = 9,
    /// Masked local adjustments and retouching.
    Locals = 10,
    /// Vignette, grain, lens blur and other creative effects.
    Effects = 11,
    /// The single composed inverse map: distortion, Upright, transform, crop.
    Geometry = 12,
    /// Gamut mapping and output encoding.
    Output = 13,
}

impl StageId {
    /// Every stage in pipeline order.
    pub const ALL: [StageId; 14] = [
        Self::Decode,
        Self::Linearize,
        Self::Denoise,
        Self::Demosaic,
        Self::Lens,
        Self::CameraProfile,
        Self::WhiteBalance,
        Self::Detail,
        Self::Tone,
        Self::Color,
        Self::Locals,
        Self::Effects,
        Self::Geometry,
        Self::Output,
    ];

    /// Number of stages.
    pub const COUNT: usize = Self::ALL.len();

    /// Position in the pipeline (0-based).
    pub const fn index(self) -> usize {
        self as usize
    }

    /// Stage at `index`, if any.
    pub fn from_index(index: usize) -> Option<Self> {
        Self::ALL.get(index).copied()
    }

    /// The following stage, or `None` after [`StageId::Output`].
    pub fn next(self) -> Option<Self> {
        Self::from_index(self.index() + 1)
    }

    /// Stable snake_case name (matches the serde form).
    pub const fn name(self) -> &'static str {
        match self {
            Self::Decode => "decode",
            Self::Linearize => "linearize",
            Self::Denoise => "denoise",
            Self::Demosaic => "demosaic",
            Self::Lens => "lens",
            Self::CameraProfile => "camera_profile",
            Self::WhiteBalance => "white_balance",
            Self::Detail => "detail",
            Self::Tone => "tone",
            Self::Color => "color",
            Self::Locals => "locals",
            Self::Effects => "effects",
            Self::Geometry => "geometry",
            Self::Output => "output",
        }
    }
}

impl fmt::Display for StageId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

/// Hash of a stage's parameters, or (when chained) of a stage's parameters
/// together with everything upstream of it.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Default, Serialize, Deserialize,
)]
#[serde(transparent)]
pub struct ParamHash(pub Digest);

impl ParamHash {
    /// Hash of one stage's parameters in canonical JSON form.
    pub fn of<P: Serialize + ?Sized>(stage: StageId, params: &P) -> Self {
        let mut bytes = stage.name().as_bytes().to_vec();
        bytes.push(0);
        bytes.extend_from_slice(&canonical_json(params));
        Self(Digest::derive("engine-api 2026 stage-params v1", &bytes))
    }

    /// Chains an upstream cumulative hash with this stage's own hash.
    pub fn chain(upstream: ParamHash, stage: ParamHash) -> Self {
        let mut bytes = [0u8; 64];
        bytes[..32].copy_from_slice(upstream.0.as_bytes());
        bytes[32..].copy_from_slice(stage.0.as_bytes());
        Self(Digest::derive("engine-api 2026 stage-chain v1", &bytes))
    }
}

impl fmt::Display for ParamHash {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&self.0, f)
    }
}

/// Parameters owned by exactly one pipeline stage.
///
/// The default [`param_hash`](StageParams::param_hash) hashes the canonical
/// JSON form (object keys sorted, `-0.0` folded to `0.0`), so two values that
/// serialize identically hash identically regardless of field declaration
/// order. Implementations must not put non-deterministic state (maps with
/// random iteration order, timestamps, caches) in their serialized form.
pub trait StageParams: Serialize {
    /// The stage these parameters configure.
    const STAGE: StageId;

    /// Hash of these parameters alone.
    fn param_hash(&self) -> ParamHash {
        ParamHash::of(Self::STAGE, self)
    }
}

/// Key for a memoized stage output tile: `(image, stage, cumulative params
/// hash, tile)`. The pyramid level is part of [`TileCoord`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct MemoKey {
    /// Image whose pipeline produced the tile.
    pub image_id: ImageId,
    /// Stage whose *output* the tile is.
    pub stage: StageId,
    /// Chained hash of all parameters from the first stage through `stage`
    /// (see [`crate::recipe::DevelopSettings::stage_chain`]).
    pub params_hash: ParamHash,
    /// Tile address, including pyramid level.
    pub tile: TileCoord,
}

impl MemoKey {
    /// Pyramid level of the tile.
    pub fn level(&self) -> u8 {
        self.tile.level
    }

    /// Stable content digest of the key, for on-disk cache file names.
    pub fn digest(&self) -> Digest {
        let mut bytes = Vec::with_capacity(16 + 1 + 32 + 9);
        bytes.extend_from_slice(&self.image_id.0.to_le_bytes());
        bytes.push(self.stage as u8);
        bytes.extend_from_slice(self.params_hash.0.as_bytes());
        bytes.push(self.tile.level);
        bytes.extend_from_slice(&self.tile.x.to_le_bytes());
        bytes.extend_from_slice(&self.tile.y.to_le_bytes());
        Digest::derive("engine-api 2026 memo-key v1", &bytes)
    }
}

/// Canonical JSON bytes: object keys sorted by byte order at every depth,
/// negative zero folded to zero, no whitespace. Serialization failures (which
/// only occur for types that cannot be JSON) hash as `null`.
pub fn canonical_json<T: Serialize + ?Sized>(value: &T) -> Vec<u8> {
    let value = serde_json::to_value(value).unwrap_or(Value::Null);
    let mut out = Vec::with_capacity(256);
    write_canonical(&value, &mut out);
    out
}

fn write_canonical(value: &Value, out: &mut Vec<u8>) {
    match value {
        Value::Object(map) => {
            let mut entries: Vec<_> = map.iter().collect();
            entries.sort_by(|a, b| a.0.as_bytes().cmp(b.0.as_bytes()));
            out.push(b'{');
            for (i, (k, v)) in entries.into_iter().enumerate() {
                if i > 0 {
                    out.push(b',');
                }
                out.extend_from_slice(Value::String(k.clone()).to_string().as_bytes());
                out.push(b':');
                write_canonical(v, out);
            }
            out.push(b'}');
        }
        Value::Array(items) => {
            out.push(b'[');
            for (i, v) in items.iter().enumerate() {
                if i > 0 {
                    out.push(b',');
                }
                write_canonical(v, out);
            }
            out.push(b']');
        }
        Value::Number(n) if n.as_f64() == Some(0.0) && n.is_f64() => out.push(b'0'),
        other => out.extend_from_slice(other.to_string().as_bytes()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stage_order_is_fixed() {
        for (i, s) in StageId::ALL.iter().enumerate() {
            assert_eq!(s.index(), i);
            assert_eq!(StageId::from_index(i), Some(*s));
            assert_eq!(serde_json::to_value(s).unwrap(), s.name());
        }
        assert!(StageId::Decode < StageId::Demosaic && StageId::Tone < StageId::Geometry);
        assert_eq!(StageId::Output.next(), None);
        assert_eq!(StageId::Decode.next(), Some(StageId::Linearize));
    }

    #[test]
    fn canonical_json_is_order_independent() {
        let a = serde_json::json!({"b": 1, "a": {"y": -0.0, "x": [1.5, 2]}});
        let b = serde_json::json!({"a": {"x": [1.5, 2], "y": 0.0}, "b": 1});
        assert_eq!(canonical_json(&a), canonical_json(&b));
        assert_eq!(
            String::from_utf8(canonical_json(&a)).unwrap(),
            r#"{"a":{"x":[1.5,2],"y":0},"b":1}"#
        );
    }

    #[derive(Serialize)]
    struct Exposure {
        ev: f32,
    }
    impl StageParams for Exposure {
        const STAGE: StageId = StageId::Tone;
    }

    #[test]
    fn param_hash_is_stage_scoped_and_chained() {
        let h = Exposure { ev: 1.0 }.param_hash();
        assert_eq!(h, Exposure { ev: 1.0 }.param_hash());
        assert_ne!(h, Exposure { ev: 1.5 }.param_hash());
        assert_ne!(h, ParamHash::of(StageId::Color, &Exposure { ev: 1.0 }));
        let up = ParamHash::default();
        assert_ne!(ParamHash::chain(up, h), ParamHash::chain(h, up));
    }

    #[test]
    fn memo_key_digest_distinguishes_fields() {
        let k = MemoKey {
            image_id: ImageId(1),
            stage: StageId::Demosaic,
            params_hash: ParamHash::default(),
            tile: TileCoord::new(0, 1, 2),
        };
        let mut k2 = k;
        k2.tile = TileCoord::new(0, 2, 1);
        assert_ne!(k.digest(), k2.digest());
        assert_eq!(k.level(), 0);
    }
}
