//! # cutlass-ffi
//!
//! The boundary every native UI talks to. Designed as **commands in / events
//! out**, not fine-grained getters/setters, so it stays small and stable
//! across all six platforms:
//!
//! * Apple (SwiftUI) and Android (Compose) bind via **UniFFI** (auto-generated
//!   Swift/Kotlin) — one Rust API serves three Apple targets + Android.
//! * Windows (WinUI) and Linux (GTK/Qt) bind via the **C ABI** (cbindgen).
//!
//! The core owns all state; the UI sends [`cutlass_core::Command`]s and
//! subscribes to [`Event`]s. Commands/events are JSON at this layer so the
//! boundary is language-agnostic until UniFFI/cbindgen typed wrappers are
//! generated.

use cutlass_core::{ChangeSet, Command, Document, Editor, SceneId, SceneSnapshot, SharedScene};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

/// Events the core emits for the UI to react to. UI subscribes; it never polls
/// engine internals.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Event {
    /// A granular description of what an edit (or undo/redo) touched. The UI
    /// refreshes only the listed layers — re-fetched individually via
    /// [`Session::layer_json`] — rebuilds its layer tree only when
    /// `change.structural`, and the renderer invalidates only `change.time`.
    /// This replaces re-serializing the whole document per keystroke.
    Changed { change: ChangeSet },
    PlayheadMoved { seconds: f64 },
    Error { message: String },
}

/// A live editing session: owns an [`Editor`] and a queue of outbound events.
/// Native UIs hold an opaque handle to this (UniFFI object / C pointer).
///
/// The session also publishes an immutable [`SceneSnapshot`] after every change
/// via a [`SharedScene`]. A UI's render thread calls [`Session::shared_scene`]
/// once and then pulls snapshots every frame — it never reaches back into the
/// editor's live (mutating) document.
pub struct Session {
    editor: Editor,
    outbox: Vec<Event>,
    shared: Arc<SharedScene>,
}

impl Session {
    pub fn new(document: Document) -> Self {
        let editor = Editor::new(document);
        let shared = Arc::new(SharedScene::new(editor.snapshot()));
        Self {
            editor,
            outbox: Vec::new(),
            shared,
        }
    }

    /// Handle the UI's render thread keeps to pull the latest snapshot each
    /// frame. Clone-and-keep; it stays valid for the life of the session.
    pub fn shared_scene(&self) -> Arc<SharedScene> {
        Arc::clone(&self.shared)
    }

    /// The current immutable snapshot (e.g. for a one-off offscreen export).
    pub fn snapshot(&self) -> Arc<SceneSnapshot> {
        self.editor.snapshot()
    }

    /// Apply a command supplied by the UI (already deserialized). On success the
    /// new snapshot is published and a granular [`Event::Changed`] is queued.
    pub fn dispatch(&mut self, cmd: Command) {
        match self.editor.apply(cmd) {
            Ok(change) => {
                self.shared.publish(self.editor.snapshot());
                self.outbox.push(Event::Changed { change });
            }
            Err(e) => self.outbox.push(Event::Error {
                message: e.to_string(),
            }),
        }
    }

    /// Apply a command serialized as JSON (the language-agnostic path used
    /// before typed UniFFI/cbindgen wrappers exist).
    pub fn dispatch_json(&mut self, json: &str) {
        match serde_json::from_str::<Command>(json) {
            Ok(cmd) => self.dispatch(cmd),
            Err(e) => self.outbox.push(Event::Error {
                message: format!("bad command: {e}"),
            }),
        }
    }

    /// Drain pending events for the UI to consume.
    pub fn poll_events(&mut self) -> Vec<Event> {
        std::mem::take(&mut self.outbox)
    }

    /// Snapshot the **whole** document as JSON. The UI calls this once to load a
    /// project; thereafter it updates incrementally from [`Event::Changed`] +
    /// [`Session::layer_json`], never re-parsing the whole document per edit.
    pub fn document_json(&self) -> String {
        self.editor.document.to_json().unwrap_or_default()
    }

    /// Serialize just one layer subtree by stable id — the incremental read the
    /// UI uses to refresh the layers named in an [`Event::Changed`]. Returns
    /// `None` if the id is unknown (e.g. it was removed).
    pub fn layer_json(&self, id: SceneId) -> Option<String> {
        self.editor
            .document
            .scene
            .layer(id)
            .and_then(|layer| serde_json::to_string(&layer).ok())
    }

    /// The document schema version (constant for the session; the UI reads it
    /// once rather than on every change).
    pub fn schema_version(&self) -> u32 {
        self.editor.document.schema_version
    }

    pub fn undo(&mut self) -> bool {
        match self.editor.undo() {
            Some(change) => {
                self.shared.publish(self.editor.snapshot());
                self.outbox.push(Event::Changed { change });
                true
            }
            None => false,
        }
    }

    pub fn redo(&mut self) -> bool {
        match self.editor.redo() {
            Some(change) => {
                self.shared.publish(self.editor.snapshot());
                self.outbox.push(Event::Changed { change });
                true
            }
            None => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cutlass_core::scene::{Layer, LayerContent, Scene};
    use cutlass_core::{Fps, Time};

    fn session() -> Session {
        Session::new(Document::new("t", Scene::new(100, 100, Fps::from_int(30))))
    }

    #[test]
    fn dispatch_emits_granular_change_and_subtree_is_readable() {
        let mut s = session();
        let layer = Layer::new("Alpha", LayerContent::Group, Time::from_seconds(1.0));
        let id = layer.id;
        s.dispatch(Command::AddLayer {
            parent: None,
            index: None,
            layer: Box::new(layer),
        });

        let events = s.poll_events();
        assert_eq!(events.len(), 1, "exactly one granular event per edit");
        match &events[0] {
            Event::Changed { change } => {
                assert!(change.structural, "an add is structural");
                assert!(change.layers.contains(&id));
            }
            other => panic!("expected Changed, got {other:?}"),
        }

        // The UI re-reads only the dirty subtree, not the whole document.
        let json = s.layer_json(id).expect("subtree json");
        assert!(json.contains("Alpha"));
        assert!(s.layer_json(SceneId::new()).is_none(), "unknown id → None");
    }

    #[test]
    fn undo_emits_change_event_and_bad_json_reports_error() {
        let mut s = session();
        let layer = Layer::new("A", LayerContent::Group, Time::from_seconds(1.0));
        s.dispatch(Command::AddLayer {
            parent: None,
            index: None,
            layer: Box::new(layer),
        });
        let _ = s.poll_events();

        assert!(s.undo());
        assert!(matches!(
            s.poll_events().as_slice(),
            [Event::Changed { .. }]
        ));

        s.dispatch_json("{ not a command }");
        assert!(matches!(s.poll_events().as_slice(), [Event::Error { .. }]));
    }
}
