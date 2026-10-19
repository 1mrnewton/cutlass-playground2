//! RGB **primaries** (the gamut) and the linear 3×3 matrices that convert
//! between them through CIE XYZ.
//!
//! All sets here share the **D65** white point, so converting between gamuts is a
//! single matrix multiply with no chromatic adaptation. Transfer functions
//! (gamma/PQ/HLG) are orthogonal and live in [`crate::transfer`]; gamut math is
//! always done in **linear light**.

use glam::{Mat3, Vec3};
use serde::{Deserialize, Serialize};

/// CIE XYZ of the D65 white point (Y normalized to 1).
pub const D65_XYZ: [f32; 3] = [0.950_47, 1.0, 1.088_83];

/// A set of RGB primaries (a gamut), all referenced to D65.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum Primaries {
    /// ITU-R BT.709 / sRGB gamut (HD, the SDR default).
    #[default]
    Bt709,
    /// ITU-R BT.2020 gamut (UHD / HDR — much wider).
    Bt2020,
    /// Display P3 (Apple wide-gamut displays), D65 white.
    DisplayP3,
}

impl Primaries {
    /// Matrix taking **linear** RGB in these primaries to CIE XYZ.
    pub fn to_xyz(self) -> Mat3 {
        match self {
            Primaries::Bt709 => from_rows(
                [0.412_456_4, 0.357_576_1, 0.180_437_5],
                [0.212_672_9, 0.715_152_2, 0.072_175_0],
                [0.019_333_9, 0.119_192, 0.950_304_1],
            ),
            Primaries::Bt2020 => from_rows(
                [0.636_958, 0.144_616_9, 0.168_881],
                [0.262_700_2, 0.677_998_1, 0.059_301_7],
                [0.000_000_0, 0.028_072_7, 1.060_985_1],
            ),
            Primaries::DisplayP3 => from_rows(
                [0.486_570_9, 0.265_667_7, 0.198_217_3],
                [0.228_974_6, 0.691_738_5, 0.079_286_9],
                [0.000_000_0, 0.045_113_4, 1.043_944_4],
            ),
        }
    }

    /// Matrix taking CIE XYZ to **linear** RGB in these primaries.
    pub fn from_xyz(self) -> Mat3 {
        self.to_xyz().inverse()
    }
}

/// Linear matrix converting RGB from one gamut to another (`from` → `to`),
/// composed through XYZ. Identity when `from == to`.
pub fn gamut_convert(from: Primaries, to: Primaries) -> Mat3 {
    if from == to {
        Mat3::IDENTITY
    } else {
        to.from_xyz() * from.to_xyz()
    }
}

/// Build a [`Mat3`] from **row-major** rows (how color matrices are published),
/// since glam stores columns.
fn from_rows(r0: [f32; 3], r1: [f32; 3], r2: [f32; 3]) -> Mat3 {
    Mat3::from_cols(
        Vec3::new(r0[0], r1[0], r2[0]),
        Vec3::new(r0[1], r1[1], r2[1]),
        Vec3::new(r0[2], r1[2], r2[2]),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn white_maps_to_d65_for_every_gamut() {
        let white = Vec3::ONE;
        for p in [Primaries::Bt709, Primaries::Bt2020, Primaries::DisplayP3] {
            let xyz = p.to_xyz() * white;
            assert!((xyz.x - D65_XYZ[0]).abs() < 1e-3, "{p:?} X={}", xyz.x);
            assert!((xyz.y - D65_XYZ[1]).abs() < 1e-3, "{p:?} Y={}", xyz.y);
            assert!((xyz.z - D65_XYZ[2]).abs() < 1e-3, "{p:?} Z={}", xyz.z);
        }
    }

    #[test]
    fn gamut_round_trip_is_identity() {
        let m = gamut_convert(Primaries::Bt709, Primaries::Bt2020)
            * gamut_convert(Primaries::Bt2020, Primaries::Bt709);
        let i = Mat3::IDENTITY;
        for c in 0..3 {
            for r in 0..3 {
                assert!((m.col(c)[r] - i.col(c)[r]).abs() < 1e-4);
            }
        }
    }

    #[test]
    fn same_gamut_is_identity_fastpath() {
        assert_eq!(gamut_convert(Primaries::Bt2020, Primaries::Bt2020), Mat3::IDENTITY);
    }

    #[test]
    fn wide_to_narrow_can_exceed_unit_range() {
        // A saturated Rec.2020 green sits outside Rec.709 → at least one channel
        // goes negative (out of gamut), which is exactly what we must represent.
        let g2020 = Vec3::new(0.0, 1.0, 0.0);
        let g709 = gamut_convert(Primaries::Bt2020, Primaries::Bt709) * g2020;
        assert!(g709.min_element() < 0.0, "expected out-of-gamut channel: {g709:?}");
    }
}
