//! # cutlass-text
//!
//! Cutlass owns its entire text stack — shaping, layout, and rasterization —
//! instead of calling Core Text / DirectWrite / Android text APIs. Native text
//! engines shape and hint differently, which would make the *same template*
//! render differently on each OS. To guarantee a title is pixel-identical on
//! Linux and an iPad, we shape with our own HarfBuzz-equivalent, lay out
//! ourselves, rasterize ourselves, and **bundle fonts with templates**.
//!
//! This crate currently defines the data model. The shaping/layout/raster
//! pipeline (rustybuzz + cosmic-text + swash) plugs in behind [`TextEngine`].

use cutlass_anim::Track;
use glam::Vec4;
use serde::{Deserialize, Serialize};

/// Reference to a font that ships *with the project/template* so rendering is
/// deterministic across platforms. Never resolved against system fonts for
/// canvas content.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FontRef {
    /// Logical id, resolved against the project's bundled font set.
    pub family: String,
    pub weight: u16,
    pub italic: bool,
}

/// Horizontal alignment within the text box.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum Align {
    Start,
    Center,
    End,
    Justify,
}

/// Static styling for a run of text. Animatable bits (color, size, tracking)
/// are exposed separately as [`Track`]s so the one animation engine drives
/// them like every other property.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TextStyle {
    pub font: FontRef,
    pub align: Align,
    /// RGBA, linear space.
    pub fill: Track<Vec4>,
    /// Point size.
    pub size: Track<f32>,
    /// Letter spacing in ems.
    pub tracking: Track<f32>,
    pub line_height: f32,
}

/// A contiguous run of text sharing one style.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TextRun {
    pub text: String,
    pub style: TextStyle,
}

/// A text element: one or more runs laid out in a box.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TextBlock {
    pub runs: Vec<TextRun>,
    /// Box size in scene units; `None` height means auto.
    pub width: f32,
    pub height: Option<f32>,
}

/// Output of layout: positioned glyphs ready for the rasterizer/compositor.
#[derive(Debug, Clone, Default)]
pub struct ShapedText {
    pub glyphs: Vec<PositionedGlyph>,
    pub bounds: glam::Vec2,
}

#[derive(Debug, Clone, Copy)]
pub struct PositionedGlyph {
    pub glyph_id: u32,
    pub position: glam::Vec2,
    pub size: f32,
}

/// The shaping + layout entry point. Implemented by the rustybuzz/cosmic-text
/// backend; kept as a trait so it can be swapped or mocked in golden tests.
pub trait TextEngine {
    fn shape(&self, block: &TextBlock, t: cutlass_anim::Time) -> ShapedText;
}
