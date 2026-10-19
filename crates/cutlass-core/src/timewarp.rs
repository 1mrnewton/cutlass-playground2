//! Time remapping: the mapping from **timeline time → source time** for a clip.
//!
//! A plain trim (`in_point`/`out_point`) plays a clip at 1× forward. Real editors
//! need more: constant speed changes, **speed ramps** (slow-mo into fast), reverse,
//! and freeze frames — CapCut's whole speed pipeline. [`TimeWarp`] is the single
//! primitive that expresses all of it, answering the one question the
//! decoder/mixer asks every frame: *"at this clip-local time, which source time do
//! I read?"*
//!
//! Two authoring shapes, one evaluation contract:
//!
//! * [`TimeWarp::Constant`] — a fixed multiplier (`1.0` normal, `2.0` 2×, `0.5`
//!   slow-mo, negative reverse, `0.0` freeze).
//! * [`TimeWarp::Ramp`] — a speed multiplier *keyframed* over clip-local time
//!   (a `cutlass-anim` `Track<f32>`, same engine as every other animatable).
//!   Source advances by the **integral** of speed; piecewise-linear segments
//!   integrate analytically (trapezoid area), so it's exact for the common case
//!   and O(keys), not O(samples).
//!
//! Time stays exact: source positions are computed in integer ticks
//! (`cutlass-time`), so A/V sync doesn't drift.

use cutlass_anim::{Sample, Time, Track};
use serde::{Deserialize, Serialize};

/// How a clip maps timeline time to source time. See the module docs.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum TimeWarp {
    /// Constant speed multiplier. `1.0` normal, `2.0` double-speed, `0.5`
    /// slow-mo, negative reverse, `0.0` freeze.
    Constant(f64),
    /// Speed multiplier keyframed over **clip-local** time. Source time is the
    /// running integral of this curve.
    Ramp(Track<f32>),
}

impl Default for TimeWarp {
    fn default() -> Self {
        TimeWarp::Constant(1.0)
    }
}

impl TimeWarp {
    /// Normal 1× playback.
    pub fn normal() -> Self {
        TimeWarp::Constant(1.0)
    }

    /// Instantaneous speed multiplier at clip-local time `local`.
    pub fn speed_at(&self, local: Time) -> f64 {
        match self {
            TimeWarp::Constant(s) => *s,
            TimeWarp::Ramp(track) => track.sample(local) as f64,
        }
    }

    /// Source time read at clip-local timeline time `local`, given the clip's
    /// `source_in` (its in-point). Source advances by ∫₀^local speed dt.
    pub fn source_at(&self, local: Time, source_in: Time) -> Time {
        let advance = match self {
            TimeWarp::Constant(speed) => local.ticks() as f64 * speed,
            TimeWarp::Ramp(track) => integrate_speed(track, local),
        };
        source_in + Time::from_ticks(advance.round() as i64)
    }

    /// Whether the clip is playing backwards at `local`.
    pub fn is_reversed_at(&self, local: Time) -> bool {
        self.speed_at(local) < 0.0
    }
}

/// Exact area under the speed curve from `0` to `upto`, in ticks·(unitless
/// speed) = source ticks. Linear segments → trapezoids; the curve holds its end
/// values before the first and after the last key.
fn integrate_speed(track: &Track<f32>, upto: Time) -> f64 {
    let upto = upto.ticks() as f64;
    if upto <= 0.0 {
        return 0.0;
    }
    let keys = &track.keys;
    match keys.as_slice() {
        [] => return track.default as f64 * upto,
        [only] => return only.value as f64 * upto,
        _ => {}
    }

    let first = keys.first().expect("len >= 2");
    let first_t = first.time.ticks() as f64;
    if upto <= first_t {
        return first.value as f64 * upto;
    }

    // Region before the first key holds the first value.
    let mut acc = first.value as f64 * first_t.max(0.0);

    // Interior piecewise-linear segments.
    for seg in keys.windows(2) {
        let (a, b) = (&seg[0], &seg[1]);
        let (t0, t1) = (a.time.ticks() as f64, b.time.ticks() as f64);
        if t1 <= t0 {
            continue; // coincident keys contribute no area
        }
        let (v0, v1) = (a.value as f64, b.value as f64);
        if upto >= t1 {
            acc += 0.5 * (v0 + v1) * (t1 - t0);
        } else if upto > t0 {
            let frac = (upto - t0) / (t1 - t0);
            let v = v0 + (v1 - v0) * frac;
            return acc + 0.5 * (v0 + v) * (upto - t0);
        }
    }

    // Region after the last key holds the last value.
    let last = keys.last().expect("len >= 2");
    let last_t = last.time.ticks() as f64;
    if upto > last_t {
        acc += last.value as f64 * (upto - last_t);
    }
    acc
}

#[cfg(test)]
mod tests {
    use super::*;
    use cutlass_anim::Keyframe;

    fn secs(s: f64) -> Time {
        Time::from_seconds(s)
    }

    fn approx(a: f64, b: f64) {
        assert!((a - b).abs() < 1e-9, "expected {b}, got {a}");
    }

    #[test]
    fn constant_speed_maps_linearly() {
        let w = TimeWarp::Constant(2.0);
        assert_eq!(w.source_at(secs(1.0), Time::ZERO), secs(2.0));
        approx(w.speed_at(secs(0.5)), 2.0);
    }

    #[test]
    fn reverse_and_freeze() {
        let rev = TimeWarp::Constant(-1.0);
        assert_eq!(rev.source_at(secs(1.0), secs(5.0)), secs(4.0));
        assert!(rev.is_reversed_at(secs(0.0)));

        let freeze = TimeWarp::Constant(0.0);
        assert_eq!(freeze.source_at(secs(3.0), secs(2.0)), secs(2.0));
    }

    #[test]
    fn linear_ramp_integrates_to_trapezoid_area() {
        // Speed ramps 1× → 3× across [0, 2s].
        let mut track = Track::constant(1.0);
        track.insert(Keyframe::new(secs(0.0), 1.0));
        track.insert(Keyframe::new(secs(2.0), 3.0));
        let w = TimeWarp::Ramp(track);

        // Full area = (1+3)/2 · 2s = 4s of source.
        assert_eq!(w.source_at(secs(2.0), Time::ZERO), secs(4.0));
        // Midpoint: speed = 2×, area = (1+2)/2 · 1s = 1.5s.
        assert_eq!(w.source_at(secs(1.0), Time::ZERO), secs(1.5));
        approx(w.speed_at(secs(1.0)), 2.0);
    }

    #[test]
    fn ramp_holds_values_before_first_and_after_last_key() {
        // Constant 2× expressed as a 2-key ramp on [1s, 2s].
        let mut track = Track::constant(2.0);
        track.insert(Keyframe::new(secs(1.0), 2.0));
        track.insert(Keyframe::new(secs(2.0), 2.0));
        let w = TimeWarp::Ramp(track);

        // [0,1] pre-hold (2×) + [1,2] segment (2×) + [2,3] post-hold (2×) = 6s.
        assert_eq!(w.source_at(secs(3.0), Time::ZERO), secs(6.0));
    }
}
