//! First-class audio model: clips, mix automation, and fades.
//!
//! Audio is half of a video editor, yet it had no home in the document. This
//! module gives it one:
//!
//! * An [`AudioClip`] is a [`LayerContent`](crate::LayerContent) variant, so it
//!   lives on the timeline with the same sample-accurate `cutlass-time::Time`
//!   placement as everything else (the owning node's `range` positions it).
//! * [`AudioParams`] carry **gain and pan automation** as `cutlass-anim`
//!   `Track`s (so volume rides and pans are keyframed by the *same* engine as
//!   transforms), plus mute and fades.
//!
//! The math here is intentionally pure and unit-tested: the future realtime
//! mixer (the audio thread from the threading model) and the deterministic
//! exporter both call [`AudioParams::stereo_gain_at`] to get the exact linear
//! gain for a clip at an instant — "preview == export" for audio too.
//!
//! Waveform *peaks* are a derived UI/decode artifact and deliberately live
//! outside the document (see `cutlass-media`), the same way we keep decoded
//! frames out of the model.

use crate::timewarp::TimeWarp;
use cutlass_anim::{Sample, Time, TimeRange, Track};
use serde::{Deserialize, Serialize};

/// Channel layout of an audio source/clip. Extend as real multichannel lands.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum ChannelLayout {
    Mono,
    #[default]
    Stereo,
}

impl ChannelLayout {
    /// Number of channels in this layout.
    pub fn channels(self) -> u16 {
        match self {
            ChannelLayout::Mono => 1,
            ChannelLayout::Stereo => 2,
        }
    }
}

/// Shape of a fade ramp. Position `p` runs `0..=1` across the fade and the
/// function returns a linear gain multiplier (`0..=1`). A fade-*out* evaluates
/// the same curve at its *remaining* fraction, so one curve drives both ends and
/// a symmetric crossfade lines up.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum FadeCurve {
    /// Constant-slope amplitude ramp.
    #[default]
    Linear,
    /// `sin(p·π/2)` — constant *power* across a crossfade. The safe default for
    /// music/dialogue so the sum doesn't dip or bump in the middle.
    EqualPower,
    /// `p²` — slow, gentle start ("smooth" / exponential-in).
    Exponential,
    /// `1 − (1 − p)²` — fast start, gentle finish (logarithmic-in).
    Logarithmic,
}

impl FadeCurve {
    /// Gain multiplier at fade position `p` (clamped to `0..=1`).
    pub fn gain(self, p: f32) -> f32 {
        let p = p.clamp(0.0, 1.0);
        match self {
            FadeCurve::Linear => p,
            FadeCurve::EqualPower => (p * std::f32::consts::FRAC_PI_2).sin(),
            FadeCurve::Exponential => p * p,
            FadeCurve::Logarithmic => 1.0 - (1.0 - p) * (1.0 - p),
        }
    }
}

/// Convert decibels to a linear amplitude multiplier (`0 dB → 1.0`). Use
/// [`AudioParams::muted`] for true silence; very negative dB underflows to ~0.
#[inline]
pub fn db_to_linear(db: f32) -> f32 {
    10f32.powf(db / 20.0)
}

/// Mix automation for an audio source: keyframeable gain (dB) and pan, plus mute
/// and fades. Shared shape so audio-only clips and (later) video-clip audio use
/// the same controls.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AudioParams {
    /// Gain automation in **decibels**; `0 dB` is unity. Sampled per frame.
    pub gain_db: Track<f32>,
    /// Pan automation: `-1.0` hard left, `0.0` center, `+1.0` hard right.
    pub pan: Track<f32>,
    /// Hard mute (overrides gain/fades to silence).
    pub muted: bool,
    /// Fade-in length measured from the clip's start.
    pub fade_in: Time,
    /// Fade-out length measured back from the clip's end.
    pub fade_out: Time,
    pub fade_curve: FadeCurve,
}

impl Default for AudioParams {
    fn default() -> Self {
        Self {
            gain_db: Track::constant(0.0),
            pan: Track::constant(0.0),
            muted: false,
            fade_in: Time::ZERO,
            fade_out: Time::ZERO,
            fade_curve: FadeCurve::default(),
        }
    }
}

impl AudioParams {
    /// Linear fade multiplier for a clip occupying `range` at time `t`: `1.0`
    /// outside the fade regions, tapering via [`FadeCurve`] near the ends.
    pub fn fade_gain_at(&self, t: Time, range: TimeRange) -> f32 {
        let mut g = 1.0f32;
        if self.fade_in > Time::ZERO && t < range.start + self.fade_in {
            let elapsed = (t - range.start).max(Time::ZERO);
            let p = elapsed.as_seconds() as f32 / self.fade_in.as_seconds() as f32;
            g *= self.fade_curve.gain(p);
        }
        let end = range.end();
        if self.fade_out > Time::ZERO && t > end - self.fade_out {
            let remaining = (end - t).max(Time::ZERO);
            let p = remaining.as_seconds() as f32 / self.fade_out.as_seconds() as f32;
            g *= self.fade_curve.gain(p);
        }
        g
    }

    /// Total linear gain (automation × fades, or `0` if muted) at `t` for a clip
    /// placed on `range`.
    pub fn gain_at(&self, t: Time, range: TimeRange) -> f32 {
        if self.muted {
            return 0.0;
        }
        db_to_linear(self.gain_db.sample(t)) * self.fade_gain_at(t, range)
    }

    /// Per-channel linear gains using an **equal-power** pan law, returned as
    /// `(left, right)`. Center keeps `left == right ≈ 0.707·gain` so panning
    /// doesn't change perceived loudness.
    pub fn stereo_gain_at(&self, t: Time, range: TimeRange) -> (f32, f32) {
        let g = self.gain_at(t, range);
        let pan = self.pan.sample(t).clamp(-1.0, 1.0);
        let angle = (pan + 1.0) * std::f32::consts::FRAC_PI_4; // 0..π/2 across L..R
        (g * angle.cos(), g * angle.sin())
    }
}

/// An audio-only clip (music, voiceover, SFX): a source trimmed to
/// `[in_point, out_point)` with mix automation. The owning node's `range`
/// (start + duration) positions it on the timeline, so timing is sample-accurate
/// via `cutlass-time`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AudioClip {
    /// Logical project source id (resolved to real media by the host).
    pub source: String,
    /// Trim into the source.
    pub in_point: Time,
    pub out_point: Time,
    #[serde(default)]
    pub channels: ChannelLayout,
    #[serde(default)]
    pub params: AudioParams,
    /// Optional time remap (speed/ramp/reverse). `None` plays 1× forward.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub warp: Option<TimeWarp>,
}

impl AudioClip {
    pub fn new(source: impl Into<String>, in_point: Time, out_point: Time) -> Self {
        Self {
            source: source.into(),
            in_point,
            out_point,
            channels: ChannelLayout::default(),
            params: AudioParams::default(),
            warp: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn approx(a: f32, b: f32) {
        assert!((a - b).abs() < 1e-4, "expected {b}, got {a}");
    }

    #[test]
    fn decibels_convert_to_linear() {
        approx(db_to_linear(0.0), 1.0);
        approx(db_to_linear(-6.0206), 0.5); // -6.02 dB ≈ half amplitude
        approx(db_to_linear(6.0206), 2.0);
    }

    #[test]
    fn fade_curves_hit_their_endpoints() {
        for c in [
            FadeCurve::Linear,
            FadeCurve::EqualPower,
            FadeCurve::Exponential,
            FadeCurve::Logarithmic,
        ] {
            approx(c.gain(0.0), 0.0);
            approx(c.gain(1.0), 1.0);
        }
        // Equal-power center is the classic -3 dB point.
        approx(FadeCurve::EqualPower.gain(0.5), std::f32::consts::FRAC_1_SQRT_2);
    }

    #[test]
    fn linear_fades_ramp_in_and_out() {
        let range = TimeRange::new(Time::from_seconds(1.0), Time::from_seconds(4.0)); // [1s, 5s)
        let p = AudioParams {
            fade_in: Time::from_seconds(1.0),
            fade_out: Time::from_seconds(2.0),
            ..Default::default()
        };
        approx(p.fade_gain_at(Time::from_seconds(1.0), range), 0.0); // silent at start
        approx(p.fade_gain_at(Time::from_seconds(1.5), range), 0.5); // half way in
        approx(p.fade_gain_at(Time::from_seconds(3.0), range), 1.0); // full in the middle
        approx(p.fade_gain_at(Time::from_seconds(4.0), range), 0.5); // half way out
    }

    #[test]
    fn mute_forces_silence_over_automation() {
        let range = TimeRange::new(Time::ZERO, Time::from_seconds(2.0));
        let p = AudioParams {
            gain_db: Track::constant(0.0),
            muted: true,
            ..Default::default()
        };
        approx(p.gain_at(Time::from_seconds(1.0), range), 0.0);
    }

    #[test]
    fn equal_power_pan_preserves_loudness() {
        let range = TimeRange::new(Time::ZERO, Time::from_seconds(2.0));
        let t = Time::from_seconds(1.0);

        let center = AudioParams::default();
        let (l, r) = center.stereo_gain_at(t, range);
        approx(l, r); // centered: balanced
        approx(l * l + r * r, 1.0); // unity power at 0 dB

        let left = AudioParams {
            pan: Track::constant(-1.0),
            ..Default::default()
        };
        let (l, r) = left.stereo_gain_at(t, range);
        approx(l, 1.0);
        approx(r, 0.0);
    }
}
