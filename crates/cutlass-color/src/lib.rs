//! # cutlass-color
//!
//! Color management for the editor. One rule governs everything:
//!
//! > **All compositing, blending, and effects math happens in linear light, in
//! > the working space. Encoding/decoding happens *only* at the I/O boundary.**
//!
//! * The **working space** is linear-light with [`WORKING_PRIMARIES`] (Rec.709 /
//!   sRGB gamut, D65) by default. A working color is a [`LinearRgba`].
//! * Inputs (color pickers, decoded media) are decoded to the working space with
//!   [`ColorSpace::decode`]; outputs (preview, export) are encoded with
//!   [`ColorSpace::encode`]. These CPU functions are the reference the GPU
//!   shaders mirror, so **preview == export** for color too.
//! * SDR (sRGB / Rec.709 / Display P3) and HDR (Rec.2020 + PQ/HLG) are all
//!   expressible, so "HDR-ready" is a real capability, not a comment.
//!
//! Splitting the two axes — [`Primaries`] (gamut) and [`TransferFunction`]
//! (encoding) — is what keeps this small: a [`ColorSpace`] is just one of each.

mod primaries;
mod transfer;

pub use primaries::{gamut_convert, Primaries, D65_XYZ};
pub use transfer::{OrderedF32, TransferFunction};

use glam::{Vec3, Vec4};
use serde::{Deserialize, Serialize};

/// The primaries of the linear working space. All effects/blend math is done in
/// linear light in this gamut; Rec.709 keeps SDR exact while still allowing
/// wide-gamut/HDR *output* via the boundary conversions.
pub const WORKING_PRIMARIES: Primaries = Primaries::Bt709;

/// A linear-light RGBA color in the **working space** — the only kind of color
/// the renderer and effects ever do arithmetic on. Alpha is linear (straight)
/// and never transferred.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct LinearRgba {
    pub r: f32,
    pub g: f32,
    pub b: f32,
    pub a: f32,
}

impl LinearRgba {
    pub const TRANSPARENT: LinearRgba = LinearRgba::new(0.0, 0.0, 0.0, 0.0);
    pub const WHITE: LinearRgba = LinearRgba::new(1.0, 1.0, 1.0, 1.0);
    pub const BLACK: LinearRgba = LinearRgba::new(0.0, 0.0, 0.0, 1.0);

    pub const fn new(r: f32, g: f32, b: f32, a: f32) -> Self {
        Self { r, g, b, a }
    }

    /// Interpret a `Vec4` that is *already* linear working-space RGBA.
    pub fn from_vec4(v: Vec4) -> Self {
        Self::new(v.x, v.y, v.z, v.w)
    }

    pub fn to_vec4(self) -> Vec4 {
        Vec4::new(self.r, self.g, self.b, self.a)
    }

    fn rgb(self) -> Vec3 {
        Vec3::new(self.r, self.g, self.b)
    }

    /// Decode an sRGB-encoded color (what color pickers and 8-bit assets use) to
    /// the working space. sRGB shares the working primaries, so this is a pure
    /// per-channel transfer decode.
    pub fn from_srgb(c: Vec4) -> Self {
        let tf = TransferFunction::Srgb;
        Self::new(tf.decode(c.x), tf.decode(c.y), tf.decode(c.z), c.w)
    }

    /// Encode this working-space color back to sRGB (e.g. to show a hex value).
    pub fn to_srgb(self) -> Vec4 {
        let tf = TransferFunction::Srgb;
        Vec4::new(tf.encode(self.r), tf.encode(self.g), tf.encode(self.b), self.a)
    }

    /// Decode an 8-bit sRGB color (the color-picker path).
    pub fn from_srgb8(r: u8, g: u8, b: u8, a: u8) -> Self {
        Self::from_srgb(Vec4::new(
            r as f32 / 255.0,
            g as f32 / 255.0,
            b as f32 / 255.0,
            a as f32 / 255.0,
        ))
    }
}

/// A fully-specified encoded color space: a gamut ([`Primaries`]) plus an
/// encoding ([`TransferFunction`]). This is what media is *in* and what output
/// is *to*; the working space is the linear interior between them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ColorSpace {
    pub primaries: Primaries,
    pub transfer: TransferFunction,
}

impl ColorSpace {
    /// sRGB: the SDR default for UI colors and most assets.
    pub const SRGB: ColorSpace = ColorSpace::new(Primaries::Bt709, TransferFunction::Srgb);
    /// Rec.709 broadcast SDR (709 primaries + 709 OETF).
    pub const REC709: ColorSpace = ColorSpace::new(Primaries::Bt709, TransferFunction::Bt709);
    /// Display P3 (wide-gamut SDR displays).
    pub const DISPLAY_P3: ColorSpace = ColorSpace::new(Primaries::DisplayP3, TransferFunction::Srgb);
    /// Rec.2020 + PQ — the HDR10 mastering/output space.
    pub const REC2020_PQ: ColorSpace = ColorSpace::new(Primaries::Bt2020, TransferFunction::Pq);
    /// Rec.2020 + HLG — broadcast HDR.
    pub const REC2020_HLG: ColorSpace = ColorSpace::new(Primaries::Bt2020, TransferFunction::Hlg);
    /// The working space expressed as a `ColorSpace` (linear, working primaries).
    pub const LINEAR_WORKING: ColorSpace =
        ColorSpace::new(WORKING_PRIMARIES, TransferFunction::Linear);

    pub const fn new(primaries: Primaries, transfer: TransferFunction) -> Self {
        Self { primaries, transfer }
    }

    /// Decode an encoded color in *this* space into linear working-space RGBA
    /// with the given working primaries: per-channel transfer decode, then a
    /// gamut conversion into the working primaries.
    pub fn decode(self, c: Vec4, working: Primaries) -> LinearRgba {
        let lin = Vec3::new(
            self.transfer.decode(c.x),
            self.transfer.decode(c.y),
            self.transfer.decode(c.z),
        );
        let rgb = gamut_convert(self.primaries, working) * lin;
        LinearRgba::new(rgb.x, rgb.y, rgb.z, c.w)
    }

    /// Encode a linear working-space color into *this* space: gamut convert out
    /// of the working primaries, then per-channel transfer encode.
    pub fn encode(self, c: LinearRgba, working: Primaries) -> Vec4 {
        let rgb = gamut_convert(working, self.primaries) * c.rgb();
        Vec4::new(
            self.transfer.encode(rgb.x),
            self.transfer.encode(rgb.y),
            self.transfer.encode(rgb.z),
            c.a,
        )
    }
}

impl Default for ColorSpace {
    fn default() -> Self {
        ColorSpace::SRGB
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn srgb_boundary_round_trips() {
        let picked = Vec4::new(0.2, 0.5, 0.8, 1.0);
        let working = LinearRgba::from_srgb(picked);
        let back = working.to_srgb();
        assert!((back - picked).abs().max_element() < 1e-4, "{back:?}");
        // And the linear values differ from the encoded ones (decode happened).
        assert!(working.to_vec4().x < picked.x);
    }

    #[test]
    fn linear_working_space_is_a_no_op_boundary() {
        let c = LinearRgba::new(0.1, 0.4, 0.9, 0.5);
        let enc = ColorSpace::LINEAR_WORKING.encode(c, WORKING_PRIMARIES);
        let dec = ColorSpace::LINEAR_WORKING.decode(enc, WORKING_PRIMARIES);
        assert!((dec.to_vec4() - c.to_vec4()).abs().max_element() < 1e-6);
    }

    #[test]
    fn full_pipeline_round_trips_through_hdr_space() {
        // Author in sRGB → working → master to Rec.2020/PQ → decode back.
        let working = LinearRgba::from_srgb8(255, 128, 0, 255);
        let encoded = ColorSpace::REC2020_PQ.encode(working, WORKING_PRIMARIES);
        let back = ColorSpace::REC2020_PQ.decode(encoded, WORKING_PRIMARIES);
        assert!(
            (back.to_vec4() - working.to_vec4()).abs().max_element() < 1e-3,
            "got {back:?} want {working:?}"
        );
    }

    #[test]
    fn srgb8_white_is_unit_linear() {
        let w = LinearRgba::from_srgb8(255, 255, 255, 255);
        assert!((w.r - 1.0).abs() < 1e-5 && (w.g - 1.0).abs() < 1e-5 && (w.b - 1.0).abs() < 1e-5);
    }
}
