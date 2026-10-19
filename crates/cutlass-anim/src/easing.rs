//! Temporal interpolation: how a keyframe eases toward the next one.
//!
//! Cutlass uses the After Effects / Lottie model, where easing is defined by
//! **per-keyframe tangent handles** rather than a single named curve per
//! segment. The segment from keyframe `A` to keyframe `B` is shaped by `A`'s
//! *out* handle and `B`'s *in* handle, which together form a cubic Bézier in
//! normalized easing space. This is what lets Cutlass round-trip real motion
//! design (and import Lottie/AE) instead of approximating it.

use serde::{Deserialize, Serialize};

/// How a keyframe interpolates toward the next keyframe.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum Interp {
    /// Hold the start value until the next keyframe (step).
    Hold,
    /// Straight line in value space.
    #[default]
    Linear,
    /// Cubic Bézier easing built from this key's out handle and the next key's
    /// in handle.
    Bezier,
}

/// A normalized easing handle (Lottie `i` / `o`): `x` is time influence and `y`
/// is value influence, both in `[0, 1]` for standard curves (may exceed for
/// overshoot). Anchors at `(0,0)`/`(1,1)` are implicit.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Tangent {
    pub x: f32,
    pub y: f32,
}

impl Tangent {
    pub const fn new(x: f32, y: f32) -> Self {
        Self { x, y }
    }

    /// Out handle that yields linear motion.
    pub fn linear_out() -> Self {
        Tangent::new(0.0, 0.0)
    }

    /// In handle that yields linear motion.
    pub fn linear_in() -> Self {
        Tangent::new(1.0, 1.0)
    }
}

impl Default for Tangent {
    fn default() -> Self {
        Tangent::linear_out()
    }
}

/// A cubic Bézier easing curve defined by its two control points; the anchors
/// are fixed at (0,0) and (1,1). Used internally to solve a segment's easing
/// from the two adjacent keyframes' handles.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Bezier {
    pub x1: f32,
    pub y1: f32,
    pub x2: f32,
    pub y2: f32,
}

impl Bezier {
    pub const fn new(x1: f32, y1: f32, x2: f32, y2: f32) -> Self {
        Self { x1, y1, x2, y2 }
    }

    /// Build the segment easing from the start key's out handle and the end
    /// key's in handle.
    pub fn from_handles(out: Tangent, in_: Tangent) -> Self {
        Bezier::new(out.x, out.y, in_.x, in_.y)
    }

    /// Solve for `y` given `x = t` via a few Newton-Raphson iterations.
    pub fn solve(&self, t: f32) -> f32 {
        let cx = |u: f32| {
            let mt = 1.0 - u;
            3.0 * mt * mt * u * self.x1 + 3.0 * mt * u * u * self.x2 + u * u * u
        };
        let cy = |u: f32| {
            let mt = 1.0 - u;
            3.0 * mt * mt * u * self.y1 + 3.0 * mt * u * u * self.y2 + u * u * u
        };
        // Find parameter u such that cx(u) ~= t.
        let mut u = t;
        for _ in 0..6 {
            let x = cx(u) - t;
            let dx = {
                let h = 1e-3;
                (cx(u + h) - cx(u - h)) / (2.0 * h)
            };
            if dx.abs() < 1e-6 {
                break;
            }
            u -= x / dx;
            u = u.clamp(0.0, 1.0);
        }
        cy(u)
    }
}
