//! # cutlass-time
//!
//! The single time primitive for the whole editor. A [`Time`] is an **exact**
//! instant on the timeline, stored as a signed integer count of *ticks* at a
//! fixed base ([`TICKS_PER_SECOND`]).
//!
//! ## Why fixed-base integer ticks (not `f64` seconds, not `value/rate`)
//!
//! A video editor does frame-exact arithmetic constantly (splits, trims, ripple
//! edits, A/V sync). Two representations are commonly used:
//!
//! * `f64` seconds — *rejected*: `0.1 + 0.2 != 0.3`; accumulation drift causes
//!   off-by-one-frame gaps; not `Eq`/`Ord`/`Hash`; not reproducible across
//!   platforms. (This is what Cutlass used originally and is being replaced.)
//! * rational `value/rate` (OpenTimelineIO style) — exact, but every comparison
//!   and addition needs rate reconciliation (a multiply/branch), and it isn't
//!   trivially `Hash`/`Ord`.
//!
//! We use the third option: a **single global tick base** chosen to divide every
//! common frame and audio rate. Then a `Time` is just an `i64`, so equality,
//! ordering, hashing, and arithmetic are branch-free integer ops — the fastest
//! possible representation — while remaining *exact* for all real rates. This is
//! the "flicks" idea (Facebook's 705,600,000 Hz tick).
//!
//! `705_600_000 = 2^7 · 3^2 · 5^4 · 7^2 · 11 · 13`, which divides 24, 25, 30,
//! 48, 50, 60, 90, 120, 1000 (ms), 44_100, 48_000, and the NTSC 30000/1001
//! family exactly, so frame/sample conversions are lossless.

use serde::{Deserialize, Serialize};
use std::ops::{Add, Mul, Neg, Sub};

/// Ticks per second. See the crate docs for why this constant was chosen.
pub const TICKS_PER_SECOND: i64 = 705_600_000;

/// An exact instant (or signed offset) on the timeline, in [`TICKS_PER_SECOND`]
/// ticks. Cheap to copy, compare, hash, and do arithmetic on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Time(i64);

impl Time {
    pub const ZERO: Time = Time(0);

    /// Construct directly from a raw tick count.
    #[inline]
    pub const fn from_ticks(ticks: i64) -> Self {
        Time(ticks)
    }

    /// The raw tick count.
    #[inline]
    pub const fn ticks(self) -> i64 {
        self.0
    }

    /// Nearest-tick conversion from floating seconds (for UI input / import).
    #[inline]
    pub fn from_seconds(seconds: f64) -> Self {
        Time((seconds * TICKS_PER_SECOND as f64).round() as i64)
    }

    /// Approximate seconds, for shader uniforms and human-facing display only.
    /// Never feed this back into timeline math — keep `Time` as the truth.
    #[inline]
    pub fn as_seconds(self) -> f64 {
        self.0 as f64 / TICKS_PER_SECOND as f64
    }

    /// Exact instant of a given frame index at `fps`.
    ///
    /// `ticks = frame * TICKS_PER_SECOND * den / num`, computed in `i128` to
    /// avoid overflow, exact whenever `num` divides `TICKS_PER_SECOND * den`
    /// (true for all supported rates).
    #[inline]
    pub fn from_frame(frame: i64, fps: Fps) -> Self {
        let num = fps.num as i128;
        let den = fps.den as i128;
        let ticks = (frame as i128 * TICKS_PER_SECOND as i128 * den) / num;
        Time(ticks as i64)
    }

    /// Frame index containing this instant (floor), at `fps`.
    #[inline]
    pub fn to_frame_floor(self, fps: Fps) -> i64 {
        let num = fps.num as i128;
        let den = fps.den as i128;
        // frame = floor(ticks * num / (TICKS_PER_SECOND * den))
        let n = self.0 as i128 * num;
        let d = TICKS_PER_SECOND as i128 * den;
        n.div_euclid(d) as i64
    }

    /// Nearest frame index to this instant, at `fps`.
    #[inline]
    pub fn to_frame_round(self, fps: Fps) -> i64 {
        let num = fps.num as i128;
        let den = fps.den as i128;
        let n = self.0 as i128 * num * 2 + TICKS_PER_SECOND as i128 * den; // +0.5 frame
        let d = TICKS_PER_SECOND as i128 * den * 2;
        n.div_euclid(d) as i64
    }

    #[inline]
    pub const fn is_zero(self) -> bool {
        self.0 == 0
    }

    #[inline]
    pub fn min(self, other: Time) -> Time {
        Time(self.0.min(other.0))
    }

    #[inline]
    pub fn max(self, other: Time) -> Time {
        Time(self.0.max(other.0))
    }

    /// Clamp into `[lo, hi]`.
    #[inline]
    pub fn clamp(self, lo: Time, hi: Time) -> Time {
        Time(self.0.clamp(lo.0, hi.0))
    }
}

impl Add for Time {
    type Output = Time;
    #[inline]
    fn add(self, rhs: Time) -> Time {
        Time(self.0 + rhs.0)
    }
}

impl Sub for Time {
    type Output = Time;
    #[inline]
    fn sub(self, rhs: Time) -> Time {
        Time(self.0 - rhs.0)
    }
}

impl Neg for Time {
    type Output = Time;
    #[inline]
    fn neg(self) -> Time {
        Time(-self.0)
    }
}

impl Mul<i64> for Time {
    type Output = Time;
    #[inline]
    fn mul(self, rhs: i64) -> Time {
        Time(self.0 * rhs)
    }
}

/// A frame rate as an exact rational `num/den` frames per second.
///
/// Stored on sequences/clips (not on every [`Time`]) so instants stay a single
/// integer while frame conversions remain exact, including NTSC `30000/1001`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Fps {
    pub num: u32,
    pub den: u32,
}

impl Fps {
    #[inline]
    pub const fn new(num: u32, den: u32) -> Self {
        Fps { num, den }
    }

    /// Integer rate (e.g. 30, 60).
    #[inline]
    pub const fn from_int(fps: u32) -> Self {
        Fps { num: fps, den: 1 }
    }

    /// Common NTSC drop rates: 23.976, 29.97, 59.94 → `n*1000/1001`.
    #[inline]
    pub const fn ntsc(whole: u32) -> Self {
        Fps {
            num: whole * 1000,
            den: 1001,
        }
    }

    #[inline]
    pub fn as_f64(self) -> f64 {
        self.num as f64 / self.den as f64
    }

    /// Duration of exactly one frame.
    #[inline]
    pub fn frame_duration(self) -> Time {
        Time::from_frame(1, self)
    }
}

impl Default for Fps {
    fn default() -> Self {
        Fps::from_int(30)
    }
}

/// A half-open span `[start, start + duration)` on the timeline.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TimeRange {
    pub start: Time,
    pub duration: Time,
}

impl TimeRange {
    #[inline]
    pub const fn new(start: Time, duration: Time) -> Self {
        TimeRange { start, duration }
    }

    #[inline]
    pub fn end(self) -> Time {
        self.start + self.duration
    }

    /// Half-open containment: `start <= t < end`.
    #[inline]
    pub fn contains(self, t: Time) -> bool {
        t >= self.start && t < self.end()
    }

    /// Intersection of two ranges, or `None` if they don't overlap.
    pub fn intersect(self, other: TimeRange) -> Option<TimeRange> {
        let start = self.start.max(other.start);
        let end = self.end().min(other.end());
        if end > start {
            Some(TimeRange::new(start, end - start))
        } else {
            None
        }
    }

    /// Smallest range covering both inputs (the convex hull on the timeline).
    /// Unlike [`TimeRange::intersect`], this never returns `None` — even
    /// disjoint spans produce the enclosing range. Used to accumulate the dirty
    /// timeline span of an edit.
    pub fn union(self, other: TimeRange) -> TimeRange {
        let start = self.start.min(other.start);
        let end = self.end().max(other.end());
        TimeRange::new(start, end - start)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frame_round_trip_is_exact_for_common_rates() {
        for fps in [
            Fps::from_int(24),
            Fps::from_int(25),
            Fps::from_int(30),
            Fps::from_int(60),
            Fps::ntsc(24),
            Fps::ntsc(30),
            Fps::ntsc(60),
        ] {
            for frame in [0_i64, 1, 2, 100, 999, 100_000] {
                let t = Time::from_frame(frame, fps);
                assert_eq!(t.to_frame_floor(fps), frame, "fps={fps:?} frame={frame}");
                assert_eq!(t.to_frame_round(fps), frame, "fps={fps:?} frame={frame}");
            }
        }
    }

    #[test]
    fn audio_sample_rates_divide_evenly() {
        // A single sample at 48k / 44.1k must be an exact integer tick count.
        assert_eq!(TICKS_PER_SECOND % 48_000, 0);
        assert_eq!(TICKS_PER_SECOND % 44_100, 0);
    }

    #[test]
    fn no_float_drift_when_summing_frames() {
        // The classic 0.1+0.2 problem: 10 frames at 30fps must equal exactly
        // 1/3 second in ticks, with zero accumulation error.
        let fps = Fps::from_int(30);
        let one_frame = fps.frame_duration();
        let mut acc = Time::ZERO;
        for _ in 0..30 {
            acc = acc + one_frame;
        }
        assert_eq!(acc, Time::from_frame(30, fps));
        assert_eq!(acc, Time::from_seconds(1.0));
    }

    #[test]
    fn ordering_and_arithmetic() {
        let a = Time::from_seconds(1.0);
        let b = Time::from_seconds(2.0);
        assert!(a < b);
        assert_eq!(b - a, a);
        assert_eq!(-a + b, a);
        assert_eq!(a * 2, b);
    }

    #[test]
    fn range_contains_is_half_open() {
        let r = TimeRange::new(Time::from_seconds(1.0), Time::from_seconds(2.0));
        assert!(!r.contains(Time::from_seconds(0.99)));
        assert!(r.contains(Time::from_seconds(1.0)));
        assert!(r.contains(Time::from_seconds(2.99)));
        assert!(!r.contains(r.end()));
    }

    #[test]
    fn range_intersection() {
        let a = TimeRange::new(Time::from_seconds(0.0), Time::from_seconds(2.0));
        let b = TimeRange::new(Time::from_seconds(1.0), Time::from_seconds(2.0));
        let i = a.intersect(b).unwrap();
        assert_eq!(i.start, Time::from_seconds(1.0));
        assert_eq!(i.end(), Time::from_seconds(2.0));
        let c = TimeRange::new(Time::from_seconds(5.0), Time::from_seconds(1.0));
        assert!(a.intersect(c).is_none());
    }

    #[test]
    fn range_union_covers_disjoint_spans() {
        let a = TimeRange::new(Time::from_seconds(0.0), Time::from_seconds(1.0));
        let b = TimeRange::new(Time::from_seconds(3.0), Time::from_seconds(1.0));
        let u = a.union(b);
        assert_eq!(u.start, Time::from_seconds(0.0));
        assert_eq!(u.end(), Time::from_seconds(4.0));
        // Union with self is idempotent.
        assert_eq!(a.union(a), a);
    }
}
