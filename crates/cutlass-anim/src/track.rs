//! Keyframe tracks: an ordered list of values in time that interpolate using the
//! After Effects / Lottie tangent model.

use crate::easing::{Bezier, Interp, Tangent};
use crate::value::cubic_bezier;
use crate::{Lerp, Sample, Time};
use serde::{Deserialize, Serialize};

/// Spatial motion-path handles for a keyframe (Lottie `to` / `ti`): value-space
/// offsets (e.g. pixels for position) defining the curve *through space* between
/// this keyframe and its neighbor. Boxed on the keyframe so non-spatial tracks
/// pay nothing.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SpatialTangents<T> {
    /// Offset leaving this key (Lottie `to`).
    pub out: T,
    /// Offset arriving at this key (Lottie `ti`).
    #[serde(rename = "in")]
    pub in_: T,
}

/// A single keyframe: a value pinned at a point in time, with per-key temporal
/// handles (`in`/`out`) and an [`Interp`] mode describing how it eases toward
/// the next key. Optional [`SpatialTangents`] turn straight segments into curved
/// motion paths.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Keyframe<T> {
    pub time: Time,
    pub value: T,
    #[serde(default)]
    pub interp: Interp,
    /// Handle leaving this key (Lottie `o`); used as the segment's first control
    /// point when this key's [`Interp`] is `Bezier`.
    #[serde(default = "Tangent::linear_out")]
    pub out_tangent: Tangent,
    /// Handle arriving at this key (Lottie `i`); used as the *previous* segment's
    /// second control point.
    #[serde(default = "Tangent::linear_in")]
    pub in_tangent: Tangent,
    #[serde(default = "none", skip_serializing_if = "Option::is_none")]
    pub spatial: Option<Box<SpatialTangents<T>>>,
}

fn none<T>() -> Option<Box<SpatialTangents<T>>> {
    None
}

impl<T> Keyframe<T> {
    /// A linear keyframe with neutral handles.
    pub fn new(time: Time, value: T) -> Self {
        Self {
            time,
            value,
            interp: Interp::Linear,
            out_tangent: Tangent::linear_out(),
            in_tangent: Tangent::linear_in(),
            spatial: None,
        }
    }

    /// Step interpolation (hold value until the next key).
    pub fn hold(mut self) -> Self {
        self.interp = Interp::Hold;
        self
    }

    /// Bézier easing from this key's `out` handle to the next key's `in` handle.
    pub fn bezier(mut self, out: Tangent, in_: Tangent) -> Self {
        self.interp = Interp::Bezier;
        self.out_tangent = out;
        self.in_tangent = in_;
        self
    }

    /// Set just the out handle (and switch to Bézier) — pairs with the next
    /// key's [`Keyframe::with_in_tangent`].
    pub fn with_out_tangent(mut self, out: Tangent) -> Self {
        self.interp = Interp::Bezier;
        self.out_tangent = out;
        self
    }

    /// Set just the in handle (the previous segment's easing arriving here).
    pub fn with_in_tangent(mut self, in_: Tangent) -> Self {
        self.in_tangent = in_;
        self
    }

    /// Attach spatial motion-path handles (Lottie `to`/`ti`).
    pub fn with_spatial(mut self, out: T, in_: T) -> Self {
        self.spatial = Some(Box::new(SpatialTangents { out, in_ }));
        self
    }
}

/// An animatable property: zero or more keyframes plus a constant fallback.
///
/// * 0 keyframes  → always returns the constant `default`.
/// * 1 keyframe   → constant at that keyframe's value.
/// * 2+ keyframes → piecewise interpolated, clamped at the ends.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Track<T> {
    pub default: T,
    #[serde(default = "Vec::new")]
    pub keys: Vec<Keyframe<T>>,
}

impl<T: Clone> Track<T> {
    /// A non-animated property holding a single constant value.
    pub fn constant(value: T) -> Self {
        Self {
            default: value,
            keys: Vec::new(),
        }
    }

    /// Whether this property actually animates (has 2+ keyframes).
    pub fn is_animated(&self) -> bool {
        self.keys.len() >= 2
    }

    /// Insert a keyframe, keeping the track sorted by time.
    pub fn insert(&mut self, key: Keyframe<T>) {
        let idx = self.keys.partition_point(|k| k.time <= key.time);
        self.keys.insert(idx, key);
    }
}

impl<T> Sample for Track<T>
where
    T: Lerp,
{
    type Output = T;

    fn sample(&self, t: Time) -> T {
        match self.keys.as_slice() {
            [] => self.default.clone(),
            [only] => only.value.clone(),
            keys => {
                // Before first / after last: clamp.
                if t <= keys[0].time {
                    return keys[0].value.clone();
                }
                let last = &keys[keys.len() - 1];
                if t >= last.time {
                    return last.value.clone();
                }
                // Find the surrounding segment [a, b].
                let hi = keys.partition_point(|k| k.time <= t);
                let a = &keys[hi - 1];
                let b = &keys[hi];

                if a.interp == Interp::Hold {
                    return a.value.clone();
                }

                // Exact integer span; `max(1)` guards coincident keyframes.
                let span = (b.time - a.time).ticks().max(1) as f64;
                let u_lin = ((t - a.time).ticks() as f64 / span) as f32;

                // Temporal easing: shape the parameter using the two handles.
                let u = match a.interp {
                    Interp::Hold => 0.0, // handled above
                    Interp::Linear => u_lin,
                    Interp::Bezier => {
                        Bezier::from_handles(a.out_tangent, b.in_tangent).solve(u_lin)
                    }
                };

                // Spatial path: if both ends carry spatial handles, follow a
                // cubic through space; otherwise straight value lerp.
                match (&a.spatial, &b.spatial) {
                    (Some(sa), Some(sb)) => {
                        let c0 = a.value.offset(&sa.out);
                        let c1 = b.value.offset(&sb.in_);
                        cubic_bezier(&a.value, &c0, &c1, &b.value, u)
                    }
                    _ => a.value.lerp(&b.value, u),
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Time;
    use glam::Vec2;

    #[test]
    fn constant_track_ignores_time() {
        let t = Track::constant(5.0_f32);
        assert_eq!(t.sample(Time::from_seconds(0.0)), 5.0);
        assert_eq!(t.sample(Time::from_seconds(100.0)), 5.0);
    }

    #[test]
    fn linear_interpolation_midpoint() {
        let mut t = Track::constant(0.0_f32);
        t.insert(Keyframe::new(Time::from_seconds(0.0), 0.0));
        t.insert(Keyframe::new(Time::from_seconds(2.0), 10.0));
        assert_eq!(t.sample(Time::from_seconds(1.0)), 5.0);
    }

    #[test]
    fn clamps_before_first_and_after_last() {
        let mut t = Track::constant(0.0_f32);
        t.insert(Keyframe::new(Time::from_seconds(1.0), 3.0));
        t.insert(Keyframe::new(Time::from_seconds(2.0), 9.0));
        assert_eq!(t.sample(Time::from_seconds(0.0)), 3.0);
        assert_eq!(t.sample(Time::from_seconds(5.0)), 9.0);
    }

    #[test]
    fn hold_keeps_start_until_next_key() {
        let mut t = Track::constant(0.0_f32);
        t.insert(Keyframe::new(Time::from_seconds(0.0), 0.0).hold());
        t.insert(Keyframe::new(Time::from_seconds(1.0), 1.0));
        assert_eq!(t.sample(Time::from_seconds(0.5)), 0.0);
        assert_eq!(t.sample(Time::from_seconds(0.999)), 0.0);
    }

    #[test]
    fn insert_keeps_keys_sorted() {
        let mut t = Track::constant(0.0_f32);
        t.insert(Keyframe::new(Time::from_seconds(2.0), 2.0));
        t.insert(Keyframe::new(Time::from_seconds(0.0), 0.0));
        t.insert(Keyframe::new(Time::from_seconds(1.0), 1.0));
        let times: Vec<_> = t.keys.iter().map(|k| k.time.as_seconds()).collect();
        assert_eq!(times, vec![0.0, 1.0, 2.0]);
    }

    #[test]
    fn bezier_ease_out_is_ahead_of_linear_at_midpoint() {
        // A segment's easing uses the START key's `out` handle and the END key's
        // `in` handle (the AE/Lottie model). ease-out = cubic-bezier(0,0,0.58,1),
        // fast at the start, so by the temporal midpoint it leads linear's 0.5.
        let mut t = Track::constant(0.0_f32);
        t.insert(Keyframe::new(Time::from_seconds(0.0), 0.0).with_out_tangent(Tangent::new(0.0, 0.0)));
        t.insert(Keyframe::new(Time::from_seconds(1.0), 1.0).with_in_tangent(Tangent::new(0.58, 1.0)));
        let v = t.sample(Time::from_seconds(0.5));
        assert!(v > 0.5, "ease-out should lead linear at the midpoint, got {v}");
        assert!(v < 1.0);
    }

    #[test]
    fn spatial_motion_path_bows_off_the_straight_line() {
        // Two points on the x-axis; spatial handles bow the path upward in y, so
        // the midpoint must have non-zero y (a straight lerp would give y=0).
        let mut t = Track::constant(Vec2::ZERO);
        t.insert(
            Keyframe::new(Time::from_seconds(0.0), Vec2::new(0.0, 0.0))
                .with_spatial(Vec2::new(0.0, 50.0), Vec2::new(0.0, 0.0)),
        );
        t.insert(
            Keyframe::new(Time::from_seconds(1.0), Vec2::new(100.0, 0.0))
                .with_spatial(Vec2::new(0.0, 0.0), Vec2::new(0.0, 50.0)),
        );
        let mid = t.sample(Time::from_seconds(0.5));
        assert!(mid.y > 1.0, "motion path should bow off the line, y={}", mid.y);
    }
}
