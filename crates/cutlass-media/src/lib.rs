//! # cutlass-media
//!
//! Decode/encode/demux sit behind traits with per-platform implementations.
//! This is deliberate: hardware codecs and **codec licensing** differ wildly
//! across targets. FFmpeg (GPL + patent concerns) is fine on desktop but a
//! problem on the App Store, so Apple/Android ship native encoders
//! (VideoToolbox / MediaCodec) selected at compile time behind this boundary.

use thiserror::Error;

#[derive(Debug, Error)]
pub enum MediaError {
    #[error("unsupported codec: {0}")]
    UnsupportedCodec(String),
    #[error("decode failed: {0}")]
    Decode(String),
    #[error("encode failed: {0}")]
    Encode(String),
    #[error("io error: {0}")]
    Io(String),
}

/// A decoded frame handed to the renderer. Pixel data is referenced by an
/// opaque handle so zero-copy GPU upload paths (hardware decode surfaces) are
/// possible without changing this contract.
#[derive(Debug, Clone)]
pub struct VideoFrame {
    pub pts_seconds: f64,
    pub width: u32,
    pub height: u32,
    pub format: PixelFormat,
    pub handle: FrameHandle,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PixelFormat {
    Nv12,
    Rgba8,
    /// Backend-managed hardware surface (e.g. CVPixelBuffer / AHardwareBuffer).
    HardwareSurface,
}

/// Opaque reference to frame storage owned by a backend.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FrameHandle(pub u64);

/// Random-access video decoder. Implemented per platform.
pub trait VideoDecoder {
    /// Seek + decode the frame at (or just before) `seconds`.
    fn frame_at(&mut self, seconds: f64) -> Result<VideoFrame, MediaError>;
    fn duration_seconds(&self) -> f64;
}

/// Settings for a deterministic, frame-accurate export.
#[derive(Debug, Clone)]
pub struct ExportSettings {
    pub width: u32,
    pub height: u32,
    pub fps: f64,
    pub codec: String,
    pub bitrate_kbps: u32,
}

/// Frame-accurate encoder used by the export pipeline.
pub trait VideoEncoder {
    fn push(&mut self, frame: &VideoFrame) -> Result<(), MediaError>;
    fn finish(&mut self) -> Result<(), MediaError>;
}

/// A block of decoded PCM audio. Samples are **frame-major interleaved**
/// (`[c0,c1, c0,c1, …]`) in `-1.0..=1.0`, the layout most mixers and audio
/// callbacks want.
#[derive(Debug, Clone)]
pub struct AudioBuffer {
    pub sample_rate: u32,
    pub channels: u16,
    pub samples: Vec<f32>,
}

impl AudioBuffer {
    /// Number of sample-frames (one value per channel per frame).
    pub fn frames(&self) -> usize {
        if self.channels == 0 {
            0
        } else {
            self.samples.len() / self.channels as usize
        }
    }
}

/// Random-access audio decoder. Implemented per platform; resamples to the
/// host's mix rate so the mixer sees one canonical sample rate.
pub trait AudioDecoder {
    /// Decode `[start, start + duration)` seconds of PCM.
    fn decode(&mut self, start_seconds: f64, duration_seconds: f64)
        -> Result<AudioBuffer, MediaError>;
    fn duration_seconds(&self) -> f64;
    fn sample_rate(&self) -> u32;
    fn channels(&self) -> u16;
}

/// Min/max envelope for one channel, used to draw a waveform.
#[derive(Debug, Clone, Default)]
pub struct ChannelPeaks {
    pub min: Vec<f32>,
    pub max: Vec<f32>,
}

/// Downsampled waveform envelope. A **derived cache** keyed by source — kept out
/// of the document model on purpose (the same reason decoded frames aren't in
/// the model): it's an artifact the UI/decoder recomputes, not authored state.
#[derive(Debug, Clone)]
pub struct WaveformPeaks {
    pub sample_rate: u32,
    pub samples_per_peak: u32,
    pub channels: Vec<ChannelPeaks>,
}

impl WaveformPeaks {
    /// Reduce a decoded buffer to one (min, max) pair per `samples_per_peak`
    /// frames, per channel.
    pub fn compute(buf: &AudioBuffer, samples_per_peak: u32) -> Self {
        let spp = samples_per_peak.max(1) as usize;
        let ch = buf.channels as usize;
        let frames = buf.frames();
        let buckets = if ch == 0 { 0 } else { frames.div_ceil(spp) };

        let mut channels = vec![ChannelPeaks::default(); ch];
        for b in 0..buckets {
            let start = b * spp;
            let end = ((b + 1) * spp).min(frames);
            let mut mn = vec![f32::INFINITY; ch];
            let mut mx = vec![f32::NEG_INFINITY; ch];
            for f in start..end {
                for (c, (mnc, mxc)) in mn.iter_mut().zip(mx.iter_mut()).enumerate() {
                    let s = buf.samples[f * ch + c];
                    *mnc = mnc.min(s);
                    *mxc = mxc.max(s);
                }
            }
            for (c, peaks) in channels.iter_mut().enumerate() {
                peaks.min.push(if mn[c].is_finite() { mn[c] } else { 0.0 });
                peaks.max.push(if mx[c].is_finite() { mx[c] } else { 0.0 });
            }
        }

        WaveformPeaks {
            sample_rate: buf.sample_rate,
            samples_per_peak,
            channels,
        }
    }

    /// Number of peak buckets per channel.
    pub fn peak_count(&self) -> usize {
        self.channels.first().map(|c| c.max.len()).unwrap_or(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn peaks_capture_min_and_max_per_bucket() {
        // Stereo, 4 frames: L = [0.1, -0.5, 0.9, -0.2], R = [0.0, 0.3, -0.7, 0.4]
        let buf = AudioBuffer {
            sample_rate: 48_000,
            channels: 2,
            samples: vec![0.1, 0.0, -0.5, 0.3, 0.9, -0.7, -0.2, 0.4],
        };
        let peaks = WaveformPeaks::compute(&buf, 2);
        assert_eq!(peaks.peak_count(), 2); // 4 frames / 2 per peak
        // Bucket 0 = frames 0..2, bucket 1 = frames 2..4.
        assert_eq!(peaks.channels[0].min, vec![-0.5, -0.2]);
        assert_eq!(peaks.channels[0].max, vec![0.1, 0.9]);
        assert_eq!(peaks.channels[1].min, vec![0.0, -0.7]);
        assert_eq!(peaks.channels[1].max, vec![0.3, 0.4]);
    }
}
