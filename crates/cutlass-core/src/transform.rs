//! Animatable 2D transform shared by every layer.

use cutlass_anim::Track;
use glam::{Mat3, Vec2};
use serde::{Deserialize, Serialize};

/// A 2D affine transform whose components are individually animatable.
///
/// Kept as separate tracks (rather than one matrix track) so the timeline can
/// keyframe position, scale, rotation, and opacity independently — exactly how
/// editors expose transform animation to users.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Transform2D {
    pub position: Track<Vec2>,
    pub scale: Track<Vec2>,
    /// Rotation in radians.
    pub rotation: Track<f32>,
    /// Anchor point (pivot) in local space, in the same units as `position`.
    /// Animatable to match After Effects / Lottie.
    pub anchor: Track<Vec2>,
    /// Normalized opacity in `0..=1`.
    pub opacity: Track<f32>,
}

impl Default for Transform2D {
    fn default() -> Self {
        Self {
            position: Track::constant(Vec2::ZERO),
            scale: Track::constant(Vec2::ONE),
            rotation: Track::constant(0.0),
            anchor: Track::constant(Vec2::ZERO),
            opacity: Track::constant(1.0),
        }
    }
}

impl Transform2D {
    /// Evaluate the transform to a matrix at time `t`.
    ///
    /// Order matches AE: translate to position, rotate, scale, then shift by the
    /// (negated) anchor so rotation/scale pivot around it.
    pub fn matrix_at(&self, t: cutlass_anim::Time) -> Mat3 {
        use cutlass_anim::Sample;
        let pos = self.position.sample(t);
        let scale = self.scale.sample(t);
        let rot = self.rotation.sample(t);
        let anchor = self.anchor.sample(t);
        Mat3::from_translation(pos)
            * Mat3::from_angle(rot)
            * Mat3::from_scale(scale)
            * Mat3::from_translation(-anchor)
    }

    /// Sampled opacity at `t`.
    pub fn opacity_at(&self, t: cutlass_anim::Time) -> f32 {
        use cutlass_anim::Sample;
        self.opacity.sample(t)
    }
}
