//! # cutlass-render
//!
//! The GPU compositor. One [`RenderGraph`] is walked for **both** the realtime
//! preview and the final export — the only differences are the time-step and
//! quality knobs in [`RenderMode`]. This "preview == export" rule is what keeps
//! a template faithful between what the user sees and what they ship.
//!
//! Backed by `wgpu`, so the same graph runs on Metal, Vulkan, D3D12, and GLES.
//! Native UIs hand us a surface to draw into via `raw-window-handle`
//! (CAMetalLayer / HWND / ANativeWindow / Wayland), keeping the UI native while
//! the pixels are 100% shared engine code.

use cutlass_anim::{Fps, Time};
use cutlass_core::scene::SceneId;
use cutlass_core::SceneSnapshot;

/// How a frame is produced. Same graph, different fidelity/timing.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum RenderMode {
    /// Realtime: render at the surface's cadence, may drop quality to keep up.
    Preview { scale: f32 },
    /// Deterministic, frame-accurate: fixed timestep, fixed RNG seeds, full
    /// quality. Required so exports are reproducible and golden-image testable.
    Export { fps: Fps },
}

/// A request to render one frame of a scene at a given time.
#[derive(Debug, Clone, Copy)]
pub struct FrameRequest {
    pub time: Time,
    pub mode: RenderMode,
}

/// Where rendered pixels go. Abstracted so the same compositor targets an
/// on-screen native surface (preview) or an offscreen buffer (export).
pub trait RenderTarget {
    fn width(&self) -> u32;
    fn height(&self) -> u32;
}

/// The compositor: turns an immutable [`SceneSnapshot`] + [`FrameRequest`] into
/// pixels on a [`RenderTarget`].
///
/// The renderer takes a **snapshot**, never the live editable scene (see
/// `cutlass_core::snapshot`): this is what lets it run on its own thread without
/// racing edits. The concrete wgpu implementation lands with the first render
/// spike; the trait lets the rest of the system build against it now.
pub trait Renderer {
    type Error;
    fn render(
        &mut self,
        snapshot: &SceneSnapshot,
        request: FrameRequest,
        target: &mut dyn RenderTarget,
    ) -> Result<(), Self::Error>;
}

/// The visual clips to composite at `t`, in paint order (back to front).
///
/// A tiny piece of real, testable engine logic that does not need the GPU: the
/// render graph and golden-image harness both rely on it. Track-aware (honors
/// lane enable/solo) and operates on an immutable [`SceneSnapshot`].
pub fn compositing_clip_ids(snapshot: &SceneSnapshot, t: Time) -> Vec<SceneId> {
    snapshot.compositing_clips_at(t)
}
