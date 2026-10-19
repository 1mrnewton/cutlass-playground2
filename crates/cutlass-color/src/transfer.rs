//! Transfer functions: the **encode/decode boundary** between an *encoded*
//! signal (what is stored/transmitted/displayed) and *linear light* (what all
//! compositing and effects math operates on).
//!
//! `decode` maps encoded → linear (an EOTF / inverse-OETF); `encode` maps linear
//! → encoded (an OETF / inverse-EOTF). They are exact inverses on `[0, 1]` (to
//! float precision) so a decode/encode round-trip is lossless.

use serde::{Deserialize, Serialize};

/// A per-channel transfer characteristic.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum TransferFunction {
    /// Identity — the signal already *is* linear light.
    #[default]
    Linear,
    /// IEC 61966-2-1 sRGB (piecewise: a linear toe + a 2.4 power segment).
    Srgb,
    /// Pure power law `v^gamma` on decode (e.g. display gamma 2.2 / 2.4). The
    /// stored value is the exponent applied when **decoding** to linear.
    Gamma(OrderedF32),
    /// ITU-R BT.709 camera OETF (a=1.099, transition at 0.018).
    Bt709,
    /// SMPTE ST 2084 (PQ), the HDR transfer used by HDR10 / Dolby Vision.
    /// Linear `1.0` corresponds to 10,000 cd/m² (absolute).
    Pq,
    /// ITU-R BT.2100 Hybrid Log-Gamma (scene-referred HDR, broadcast-friendly).
    Hlg,
}

/// A `Copy` float wrapper that is `Eq`/`Hash`-able so [`TransferFunction`] can
/// derive `Eq` (gamma values come from a small fixed set in practice).
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(transparent)]
pub struct OrderedF32(pub f32);

impl PartialEq for OrderedF32 {
    fn eq(&self, other: &Self) -> bool {
        self.0.to_bits() == other.0.to_bits()
    }
}
impl Eq for OrderedF32 {}

impl TransferFunction {
    /// Convenience constructor for a pure power-law transfer.
    pub fn gamma(g: f32) -> Self {
        TransferFunction::Gamma(OrderedF32(g))
    }

    /// Encoded signal → linear light (per channel).
    pub fn decode(self, v: f32) -> f32 {
        match self {
            TransferFunction::Linear => v,
            TransferFunction::Srgb => {
                if v <= 0.040_448_237 {
                    v / 12.92
                } else {
                    ((v + 0.055) / 1.055).powf(2.4)
                }
            }
            TransferFunction::Gamma(g) => signed_powf(v, g.0),
            TransferFunction::Bt709 => {
                // Inverse of the 709 OETF.
                if v < 0.081 {
                    v / 4.5
                } else {
                    ((v + 0.099) / 1.099).powf(1.0 / 0.45)
                }
            }
            TransferFunction::Pq => pq_eotf(v),
            TransferFunction::Hlg => hlg_eotf(v),
        }
    }

    /// Linear light → encoded signal (per channel).
    pub fn encode(self, v: f32) -> f32 {
        match self {
            TransferFunction::Linear => v,
            TransferFunction::Srgb => {
                if v <= 0.003_130_8 {
                    v * 12.92
                } else {
                    1.055 * v.powf(1.0 / 2.4) - 0.055
                }
            }
            TransferFunction::Gamma(g) => signed_powf(v, 1.0 / g.0),
            TransferFunction::Bt709 => {
                if v < 0.018 {
                    v * 4.5
                } else {
                    1.099 * v.powf(0.45) - 0.099
                }
            }
            TransferFunction::Pq => pq_oetf(v),
            TransferFunction::Hlg => hlg_oetf(v),
        }
    }
}

/// `v^g` that preserves sign (so negative working-space values — legal in a wide
/// gamut — round-trip instead of becoming NaN).
fn signed_powf(v: f32, g: f32) -> f32 {
    v.abs().powf(g).copysign(v)
}

// --- PQ (SMPTE ST 2084) -----------------------------------------------------

const PQ_M1: f32 = 2610.0 / 16384.0;
const PQ_M2: f32 = 2523.0 / 4096.0 * 128.0;
const PQ_C1: f32 = 3424.0 / 4096.0;
const PQ_C2: f32 = 2413.0 / 4096.0 * 32.0;
const PQ_C3: f32 = 2392.0 / 4096.0 * 32.0;

/// Linear (1.0 == 10,000 cd/m²) → PQ code value `[0,1]`.
fn pq_oetf(l: f32) -> f32 {
    let l = l.max(0.0);
    let lm1 = l.powf(PQ_M1);
    ((PQ_C1 + PQ_C2 * lm1) / (1.0 + PQ_C3 * lm1)).powf(PQ_M2)
}

/// PQ code value `[0,1]` → linear (1.0 == 10,000 cd/m²).
fn pq_eotf(n: f32) -> f32 {
    let n = n.max(0.0);
    let np = n.powf(1.0 / PQ_M2);
    let num = (np - PQ_C1).max(0.0);
    let den = PQ_C2 - PQ_C3 * np;
    (num / den).powf(1.0 / PQ_M1)
}

// --- HLG (ARIB STD-B67 / BT.2100) -------------------------------------------

const HLG_A: f32 = 0.178_832_77;
const HLG_B: f32 = 1.0 - 4.0 * HLG_A; // 0.28466892
const HLG_C: f32 = 0.559_910_7; // 0.5 - a*ln(4a)

/// Scene-linear `[0,1]` → HLG signal `[0,1]`.
fn hlg_oetf(e: f32) -> f32 {
    let e = e.max(0.0);
    if e <= 1.0 / 12.0 {
        (3.0 * e).sqrt()
    } else {
        HLG_A * (12.0 * e - HLG_B).ln() + HLG_C
    }
}

/// HLG signal `[0,1]` → scene-linear `[0,1]`.
fn hlg_eotf(s: f32) -> f32 {
    let s = s.max(0.0);
    if s <= 0.5 {
        s * s / 3.0
    } else {
        (((s - HLG_C) / HLG_A).exp() + HLG_B) / 12.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn round_trips(tf: TransferFunction) {
        for i in 0..=100 {
            let v = i as f32 / 100.0;
            let back = tf.encode(tf.decode(v));
            assert!((back - v).abs() < 1e-4, "{tf:?} failed round trip at {v}: {back}");
        }
    }

    #[test]
    fn all_transfers_round_trip() {
        for tf in [
            TransferFunction::Linear,
            TransferFunction::Srgb,
            TransferFunction::gamma(2.2),
            TransferFunction::Bt709,
            TransferFunction::Pq,
            TransferFunction::Hlg,
        ] {
            round_trips(tf);
        }
    }

    #[test]
    fn srgb_anchor_values() {
        let srgb = TransferFunction::Srgb;
        assert!((srgb.decode(0.0)).abs() < 1e-6);
        assert!((srgb.decode(1.0) - 1.0).abs() < 1e-6);
        // Mid sRGB (0.5) is ~0.214 linear — the canonical sanity check.
        assert!((srgb.decode(0.5) - 0.214_04).abs() < 1e-3, "{}", srgb.decode(0.5));
        // The toe is linear: 0.04 / 12.92.
        assert!((srgb.decode(0.04) - 0.04 / 12.92).abs() < 1e-6);
    }

    #[test]
    fn pq_hits_reference_white() {
        // 100 cd/m² (SDR diffuse white) is 0.01 of 10,000; its PQ code is ~0.508.
        let code = TransferFunction::Pq.encode(0.01);
        assert!((code - 0.508_08).abs() < 1e-2, "pq code {code}");
        // Endpoints map to endpoints.
        assert!(TransferFunction::Pq.encode(0.0).abs() < 1e-5);
        assert!((TransferFunction::Pq.encode(1.0) - 1.0).abs() < 1e-4);
    }

    #[test]
    fn signed_power_preserves_negatives() {
        // Wide-gamut compositing can produce small negative channels; they must
        // survive a gamma round-trip rather than turning into NaN.
        let g = TransferFunction::gamma(2.2);
        let v = -0.2_f32;
        assert!(g.encode(v) < 0.0);
        assert!((g.decode(g.encode(v)) - v).abs() < 1e-4);
    }
}
