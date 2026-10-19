//! # cutlass-ml
//!
//! The AI / computer-vision / DSP layer. Every model capability is exposed as a
//! **trait** with a swappable [`Provider`], so a fully free-and-open-source
//! build can run entirely on local/open models (whisper.cpp, piper, ONNX) while
//! optional hosted providers (for generative video/image) can be enabled by
//! users who want them. Nothing here is required for the deterministic editor
//! to function — these features *augment* it.
//!
//! `cutlass-ml` knows nothing about the document or commands. It only turns
//! media in → analysis/results out. The [`cutlass-agent`](../cutlass_agent)
//! crate is what translates those results into engine commands.
//!
//! This is a placeholder: traits + types are defined; concrete providers land
//! behind feature flags as each capability is implemented.

use cutlass_media::VideoFrame;
use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum MlError {
    #[error("no provider configured for capability: {0}")]
    NoProvider(&'static str),
    #[error("model error: {0}")]
    Model(String),
}

/// Where a capability runs. FOSS-first: `Local` is always preferred; `Hosted`
/// is opt-in for features only practical via external APIs (e.g. text-to-video).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Provider {
    Local,
    Hosted,
}

// --- Audio analysis & speech ------------------------------------------------

/// A time range in seconds.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Span {
    pub start: f64,
    pub end: f64,
}

/// One transcribed, time-coded caption segment.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Caption {
    pub span: Span,
    pub text: String,
}

/// Result of speech-to-text over an audio track.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct Transcript {
    pub captions: Vec<Caption>,
    pub language: String,
}

/// Speech-to-text (auto-captions / subtitles). Implemented by e.g. whisper.cpp.
pub trait SpeechToText {
    fn transcribe(&self, source: &str, language: Option<&str>) -> Result<Transcript, MlError>;
}

/// Detects silent spans in an audio track — the analysis behind "cut all
/// silences". Pure DSP; ships in the FOSS core.
pub trait SilenceDetector {
    fn detect(&self, source: &str, threshold_db: f32, min_len: f64) -> Result<Vec<Span>, MlError>;
}

/// Text-to-speech voiceover.
pub trait TextToSpeech {
    /// Synthesize `text` with `voice`, returning a path/handle to the audio.
    fn synthesize(&self, text: &str, voice: &str) -> Result<String, MlError>;
}

// --- Vision -----------------------------------------------------------------

/// Per-frame matte for background removal / chroma-free cutout.
pub trait Segmenter {
    /// Returns an alpha matte handle for the subject in `frame`.
    fn matte(&self, frame: &VideoFrame) -> Result<cutlass_media::FrameHandle, MlError>;
}

/// Frame enhancement: upscaling / denoise / restoration.
pub trait FrameEnhancer {
    fn enhance(&self, frame: &VideoFrame) -> Result<VideoFrame, MlError>;
}

// --- Generative (opt-in, typically Hosted) ----------------------------------

/// Generative media requests (text/image-to-video, image generation). These
/// are integrations, not reimplementations; keeping them behind a trait lets
/// the FOSS build omit them entirely.
pub trait Generator {
    fn text_to_video(&self, prompt: &str, seconds: f32) -> Result<String, MlError>;
    fn image_to_video(&self, image: &str, seconds: f32) -> Result<String, MlError>;
    fn text_to_image(&self, prompt: &str) -> Result<String, MlError>;
}

/// Registry of whichever providers are compiled/enabled in this build. The
/// agent borrows from here; missing capabilities surface as [`MlError::NoProvider`].
#[derive(Default)]
pub struct Models {
    pub stt: Option<Box<dyn SpeechToText>>,
    pub silence: Option<Box<dyn SilenceDetector>>,
    pub tts: Option<Box<dyn TextToSpeech>>,
    pub segmenter: Option<Box<dyn Segmenter>>,
    pub enhancer: Option<Box<dyn FrameEnhancer>>,
    pub generator: Option<Box<dyn Generator>>,
}
