//! # cutlass-anim
//!
//! The single animation engine for the whole editor. Every animatable
//! property in Cutlass — transforms, text attributes, shape control points,
//! and shader uniforms — is driven by the *same* keyframe + easing model that
//! lives here. There are deliberately no per-feature animation systems: that
//! is how cross-platform editors end up with "text animates differently from
//! shapes" bugs.
//!
//! A property is animated by attaching a [`Track<T>`] of [`Keyframe<T>`]s to
//! it. Sampling a track at a given [`Time`] yields the interpolated value.

mod easing;
mod track;
mod value;

pub use easing::{Bezier, Interp, Tangent};
pub use track::{Keyframe, SpatialTangents, Track};
pub use value::{cubic_bezier, Lerp};

/// The canonical time primitive lives in `cutlass-time`; re-exported here so the
/// animation engine and its dependents have one `Time`/`Fps`/`TimeRange`.
pub use cutlass_time::{Fps, Time, TimeRange};

/// Anything that can be evaluated at a point in time.
///
/// Implemented by tracks, and by composite property bags, so the renderer can
/// uniformly ask "what is your value at `t`?" without caring how it animates.
pub trait Sample {
    type Output;
    fn sample(&self, t: Time) -> Self::Output;
}
