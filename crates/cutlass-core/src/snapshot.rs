//! Immutable, thread-safe scene snapshots and the edit→render handoff.
//!
//! ## The threading model
//!
//! The edit thread owns the mutable [`Document`](crate::Document) and applies
//! [`Command`](crate::Command)s. The renderer (and exporter) must never touch
//! that live, mutating state — instead they read an immutable [`SceneSnapshot`].
//!
//! * A snapshot **owns** its `Scene` and only ever hands out shared references,
//!   so a render thread physically cannot mutate or race the document.
//! * Tracks are intact in a snapshot, so a *single* snapshot renders any number
//!   of frames at any [`Time`]. Snapshot cost is therefore paid per **edit**,
//!   not per **frame** — the playhead can scrub and preview can run at 60fps
//!   with zero snapshot churn between edits.
//! * [`SharedScene`] is a lock-free, single-writer/many-reader cell: the edit
//!   thread [`publish`](SharedScene::publish)es after each change; render/export
//!   threads [`latest`](SharedScene::latest) every frame without blocking.
//!
//! Today snapshotting deep-clones the scene; the next optimization is structural
//! sharing (`Arc` per node) so unchanged subtrees aren't copied.

use crate::scene::{Node, Scene, SceneId};
use arc_swap::ArcSwap;
use cutlass_time::{Fps, Time};
use std::sync::Arc;

/// An immutable, `Send + Sync` view of a scene at one point in editing history.
#[derive(Debug, Clone)]
pub struct SceneSnapshot {
    scene: Scene,
}

impl SceneSnapshot {
    /// Freeze a scene into a snapshot (takes ownership of the copy).
    pub fn new(scene: Scene) -> Self {
        Self { scene }
    }

    /// Read-only access to the underlying scene. There is intentionally no
    /// `&mut` accessor: a snapshot is immutable by construction.
    pub fn scene(&self) -> &Scene {
        &self.scene
    }

    pub fn width(&self) -> u32 {
        self.scene.width
    }

    pub fn height(&self) -> u32 {
        self.scene.height
    }

    pub fn fps(&self) -> Fps {
        self.scene.fps
    }

    pub fn duration(&self) -> Time {
        self.scene.duration
    }

    pub fn len(&self) -> usize {
        self.scene.len()
    }

    pub fn is_empty(&self) -> bool {
        self.scene.is_empty()
    }

    pub fn get(&self, id: SceneId) -> Option<&Node> {
        self.scene.get(id)
    }

    pub fn root_nodes(&self) -> impl Iterator<Item = &Node> {
        self.scene.root_nodes()
    }

    /// Stable ids of top-level nodes active at `t`, in paint order.
    pub fn active_layers_at(&self, t: Time) -> Vec<SceneId> {
        self.scene.active_layers_at(t)
    }

    /// The visual clips to paint at `t`, in compositing order (track-aware).
    /// The renderer's input.
    pub fn compositing_clips_at(&self, t: Time) -> Vec<SceneId> {
        self.scene.compositing_clips_at(t)
    }

    /// Stable ids of audio clips to mix at `t` (track-aware: honors mute/solo).
    /// The mixer/audio thread reads this off the snapshot, never the live scene.
    pub fn active_audio_at(&self, t: Time) -> Vec<SceneId> {
        self.scene.active_audio_at(t)
    }
}

/// A lock-free handoff of the latest [`SceneSnapshot`] from the edit thread to
/// render/export threads.
///
/// Wrap in an `Arc` and share across threads. Writes (`publish`) never block
/// reads (`latest`) and vice versa.
#[derive(Debug)]
pub struct SharedScene {
    current: ArcSwap<SceneSnapshot>,
}

impl SharedScene {
    pub fn new(initial: Arc<SceneSnapshot>) -> Self {
        Self {
            current: ArcSwap::from(initial),
        }
    }

    /// Publish a new snapshot (edit thread, after a change).
    pub fn publish(&self, snapshot: Arc<SceneSnapshot>) {
        self.current.store(snapshot);
    }

    /// Load the latest snapshot (render/export thread, per frame). Cheap: one
    /// atomic load plus an `Arc` clone.
    pub fn latest(&self) -> Arc<SceneSnapshot> {
        self.current.load_full()
    }
}

// Compile-time guarantee that the handoff types can actually cross threads.
const _: fn() = || {
    fn assert_send_sync<T: Send + Sync + 'static>() {}
    assert_send_sync::<SceneSnapshot>();
    assert_send_sync::<SharedScene>();
    assert_send_sync::<Arc<SharedScene>>();
};

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scene::{Layer, LayerContent, Scene};
    use cutlass_time::Fps;

    fn scene_with(n: usize) -> Scene {
        let mut s = Scene::new(10, 10, Fps::from_int(30));
        for i in 0..n {
            s.insert_root(Layer::new(
                format!("L{i}"),
                LayerContent::Group,
                Time::from_seconds(1.0),
            ));
        }
        s
    }

    #[test]
    fn snapshot_is_frozen_independent_of_later_edits() {
        let snap = Arc::new(SceneSnapshot::new(scene_with(1)));
        let shared = SharedScene::new(Arc::clone(&snap));
        // Publish a bigger scene; the old snapshot must be unchanged.
        shared.publish(Arc::new(SceneSnapshot::new(scene_with(3))));
        assert_eq!(snap.len(), 1, "old snapshot stays frozen");
        assert_eq!(shared.latest().len(), 3, "latest reflects the publish");
    }

    #[test]
    fn publishes_are_visible_on_another_thread() {
        let shared = Arc::new(SharedScene::new(Arc::new(SceneSnapshot::new(scene_with(0)))));
        shared.publish(Arc::new(SceneSnapshot::new(scene_with(5))));

        let reader = Arc::clone(&shared);
        let seen = std::thread::spawn(move || reader.latest().len())
            .join()
            .unwrap();
        assert_eq!(seen, 5);
    }
}
