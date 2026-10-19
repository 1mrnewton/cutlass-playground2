//! Timeline tracks (lanes) — the explicit **timeline vs. composition** split.
//!
//! A video editor has two trees that pros keep separate:
//!
//! * the **timeline** — ordered *tracks* (lanes) of clips placed in time, each
//!   with its own mute / lock / solo / blend state, and
//! * the **composition** — the scene graph of visual layers *inside* a clip.
//!
//! Cutlass models a track as a top-level node ([`LayerContent::Track`]) whose
//! children are the clips on that lane. Tracks stack in a defined **compositing
//! order** (roots order, back-to-front), so paint order is an explicit, typed
//! property — not the old unwritten "index 0 is backmost" convention. This also
//! gives [`LayerContent::Group`] a single clear meaning: a *composition* /
//! precomp container nested inside a clip, never a lane.
//!
//! [`LayerContent::Track`]: crate::LayerContent::Track
//! [`LayerContent::Group`]: crate::LayerContent::Group

use serde::{Deserialize, Serialize};

/// What a lane carries. Organizational: scene queries still classify clips by
/// their actual content, but the kind drives default UI (height, controls) and
/// which subsystem consumes the lane (compositor vs. mixer).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum TrackKind {
    #[default]
    Video,
    Audio,
    /// Captions / titles.
    Subtitle,
    /// Stickers / effects layered above the main video.
    Overlay,
}

impl TrackKind {
    /// Whether this lane is mixed into audio (vs. composited into pixels).
    pub fn is_audio(self) -> bool {
        matches!(self, TrackKind::Audio)
    }
}

/// How a video track composites over the tracks beneath it. Consumed by the
/// compositor in `cutlass-render`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum BlendMode {
    #[default]
    Normal,
    Multiply,
    Screen,
    Overlay,
    Add,
    Subtract,
}

/// Per-lane state: the mute / lock / solo / blend controls every NLE exposes.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TrackMeta {
    pub kind: TrackKind,
    /// Visible (video) / audible (audio). `false` = hidden / muted.
    pub enabled: bool,
    /// Locked against editing. Does **not** affect playback.
    pub locked: bool,
    /// When *any* track in the scene is soloed, only soloed tracks play.
    pub solo: bool,
    pub blend: BlendMode,
}

impl Default for TrackMeta {
    fn default() -> Self {
        Self {
            kind: TrackKind::Video,
            enabled: true,
            locked: false,
            solo: false,
            blend: BlendMode::Normal,
        }
    }
}

impl TrackMeta {
    pub fn new(kind: TrackKind) -> Self {
        Self {
            kind,
            ..Default::default()
        }
    }

    /// Whether this lane plays at all, given whether *some* track is soloed.
    /// Soloing any track silences every non-soloed track.
    pub fn is_live(&self, any_solo: bool) -> bool {
        self.enabled && (!any_solo || self.solo)
    }
}
