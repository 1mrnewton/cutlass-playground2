//! Minimal serde model for the Lottie / Bodymovin JSON schema.
//!
//! Only the fields Cutlass currently maps are typed; everything else (and the
//! highly heterogeneous shape items) is kept as [`serde_json::Value`] and
//! interpreted in [`crate::convert`]. This keeps the importer resilient to the
//! parts of the format we don't model yet rather than failing to parse.

use serde::Deserialize;
use serde_json::Value;

/// A Lottie composition (the top-level animation object).
#[derive(Debug, Deserialize)]
pub struct Lottie {
    /// Schema version (e.g. "4.8.0"). Parsed for completeness; not yet used.
    #[serde(default)]
    #[allow(dead_code)]
    pub v: String,
    /// Composition name.
    #[serde(default)]
    pub nm: String,
    /// Frame rate.
    #[serde(default)]
    pub fr: f64,
    /// In point (frames). Parsed for completeness; not yet used.
    #[serde(default)]
    #[allow(dead_code)]
    pub ip: f64,
    /// Out point (frames).
    #[serde(default)]
    pub op: f64,
    pub w: u32,
    pub h: u32,
    #[serde(default)]
    pub layers: Vec<Layer>,
}

/// A single Lottie layer.
#[derive(Debug, Deserialize)]
pub struct Layer {
    #[serde(default)]
    pub nm: String,
    /// Layer type: 0 precomp, 1 solid, 2 image, 3 null, 4 shape, 5 text.
    #[serde(default)]
    pub ty: i64,
    /// In point (frames).
    #[serde(default)]
    pub ip: f64,
    /// Out point (frames).
    #[serde(default)]
    pub op: f64,
    /// Start time (frames). Parsed for completeness; not yet used.
    #[serde(default)]
    #[allow(dead_code)]
    pub st: f64,
    /// Transform ("ks").
    pub ks: Option<Transform>,
    /// Shape items (groups / paths / paints / modifiers), kept raw.
    #[serde(default)]
    pub shapes: Vec<Value>,
}

/// A layer transform. Each field is a Lottie animated-property object
/// (`{ "a": 0|1, "k": ... }`), kept raw so one set of helpers parses them all.
#[derive(Debug, Deserialize)]
pub struct Transform {
    /// Opacity (0..100).
    pub o: Option<Value>,
    /// Rotation (degrees).
    pub r: Option<Value>,
    /// Position.
    pub p: Option<Value>,
    /// Anchor point.
    pub a: Option<Value>,
    /// Scale (percent).
    pub s: Option<Value>,
}
