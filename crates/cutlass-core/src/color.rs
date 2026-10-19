//! Project-level color configuration.
//!
//! Records the two decisions that used to be an unwritten "colors are linear"
//! comment: the **working space** the document composites in, and the **output
//! space** it masters/displays to. Both are serialized, so a project's color
//! intent travels with it and the renderer/exporter read it instead of guessing.

use cutlass_color::{ColorSpace, LinearRgba, Primaries, TransferFunction, WORKING_PRIMARIES};
use glam::Vec4;
use serde::{Deserialize, Serialize};

/// The color pipeline for a [`Document`](crate::Document): linear working space
/// in + encoded output out.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct ColorConfig {
    /// Primaries of the linear working space — *all* compositing/effects math
    /// happens in linear light in this gamut.
    #[serde(default = "default_working")]
    pub working_primaries: Primaries,
    /// The space the project is delivered in. SDR sRGB by default; set to
    /// [`ColorSpace::REC2020_PQ`] / [`ColorSpace::REC2020_HLG`] for HDR.
    #[serde(default)]
    pub output: ColorSpace,
}

fn default_working() -> Primaries {
    WORKING_PRIMARIES
}

impl Default for ColorConfig {
    fn default() -> Self {
        Self {
            working_primaries: WORKING_PRIMARIES,
            output: ColorSpace::SRGB,
        }
    }
}

impl ColorConfig {
    /// Decode an encoded input color (a decoded media pixel, or a UI color
    /// picked in `space`) into the linear working space.
    pub fn decode_input(&self, c: Vec4, space: ColorSpace) -> LinearRgba {
        space.decode(c, self.working_primaries)
    }

    /// Encode a working-space color into the configured output space for
    /// preview/export. The GPU compositor mirrors this on its final stage so
    /// preview matches export.
    pub fn encode_output(&self, c: LinearRgba) -> Vec4 {
        self.output.encode(c, self.working_primaries)
    }

    /// Whether the output space is an HDR transfer (PQ or HLG).
    pub fn is_hdr(&self) -> bool {
        matches!(
            self.output.transfer,
            TransferFunction::Pq | TransferFunction::Hlg
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_is_sdr_srgb() {
        let cfg = ColorConfig::default();
        assert_eq!(cfg.working_primaries, Primaries::Bt709);
        assert_eq!(cfg.output, ColorSpace::SRGB);
        assert!(!cfg.is_hdr());
    }

    #[test]
    fn input_decode_then_output_encode_is_stable_for_srgb() {
        let cfg = ColorConfig::default();
        let picked = Vec4::new(0.25, 0.5, 0.75, 1.0);
        let working = cfg.decode_input(picked, ColorSpace::SRGB);
        let out = cfg.encode_output(working); // sRGB out == sRGB in
        assert!((out - picked).abs().max_element() < 1e-4, "{out:?}");
    }

    #[test]
    fn hdr_config_is_flagged() {
        let cfg = ColorConfig {
            working_primaries: Primaries::Bt2020,
            output: ColorSpace::REC2020_PQ,
        };
        assert!(cfg.is_hdr());
    }
}
