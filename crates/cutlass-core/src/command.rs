//! Command / undo system.
//!
//! Every document edit — manual or AI — is a [`Command`]. Applying a command
//! **returns its inverse**, so undo is "apply the inverse" and redo is "apply
//! the inverse of the inverse." No document snapshots, no cloning the world:
//! the undo stack holds only the small deltas needed to reverse each edit.
//!
//! [`Command::Batch`] groups edits into a single atomic, single-undo-step
//! transaction (e.g. the agent's "cut silences" = one remove + N inserts = one
//! undo entry).

use crate::change::ChangeSet;
use crate::document::Document;
use crate::scene::{Layer, LayerContent, Scene, SceneId};
use crate::snapshot::SceneSnapshot;
use crate::track::TrackMeta;
use cutlass_anim::{Time, TimeRange};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum CommandError {
    #[error("scene node not found: {0:?}")]
    NotFound(SceneId),
    #[error("invalid reparent (would create a cycle)")]
    Cycle,
    #[error("scene node is not a track: {0:?}")]
    NotATrack(SceneId),
}

/// A single, coarse-grained, **invertible** edit. Native UIs and the AI agent
/// both send these; nothing mutates the document except by applying a command.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Command {
    /// Insert a layer subtree. `index = None` appends. The layer is boxed so the
    /// `Command` enum (stored in the undo/redo stacks) stays small.
    AddLayer {
        parent: Option<SceneId>,
        index: Option<usize>,
        layer: Box<Layer>,
    },
    RemoveLayer {
        id: SceneId,
    },
    Rename {
        id: SceneId,
        name: String,
    },
    SetTiming {
        id: SceneId,
        start: Time,
        duration: Time,
    },
    Reparent {
        id: SceneId,
        new_parent: Option<SceneId>,
        index: usize,
    },
    /// Set a track lane's state (mute/lock/solo/blend/kind). Target must be a
    /// [`LayerContent::Track`](crate::LayerContent::Track) node.
    SetTrackMeta {
        id: SceneId,
        meta: Box<TrackMeta>,
    },
    /// An atomic group: applied in order, undone as one step.
    Batch(Vec<Command>),
}

impl Command {
    /// Apply to a scene, returning the command that exactly reverses it.
    ///
    /// Consumes `self` (no clone): the inverse is built from captured prior
    /// state, which is the whole point — undo costs O(change), not O(document).
    pub fn apply(self, scene: &mut Scene) -> Result<Command, CommandError> {
        match self {
            Command::AddLayer {
                parent,
                index,
                layer,
            } => {
                let id = scene.insert(parent, index, *layer)?;
                Ok(Command::RemoveLayer { id })
            }
            Command::RemoveLayer { id } => {
                let removed = scene.remove(id)?;
                Ok(Command::AddLayer {
                    parent: removed.parent,
                    index: Some(removed.index),
                    layer: Box::new(removed.layer),
                })
            }
            Command::Rename { id, name } => {
                let node = scene.get_mut(id).ok_or(CommandError::NotFound(id))?;
                let old = std::mem::replace(&mut node.name, name);
                Ok(Command::Rename { id, name: old })
            }
            Command::SetTiming {
                id,
                start,
                duration,
            } => {
                let node = scene.get_mut(id).ok_or(CommandError::NotFound(id))?;
                let old = node.range;
                node.range = cutlass_anim::TimeRange::new(start, duration);
                Ok(Command::SetTiming {
                    id,
                    start: old.start,
                    duration: old.duration,
                })
            }
            Command::Reparent {
                id,
                new_parent,
                index,
            } => {
                let (old_parent, old_index) = scene.reparent(id, new_parent, index)?;
                Ok(Command::Reparent {
                    id,
                    new_parent: old_parent,
                    index: old_index,
                })
            }
            Command::SetTrackMeta { id, meta } => {
                let node = scene.get_mut(id).ok_or(CommandError::NotFound(id))?;
                match &mut node.content {
                    LayerContent::Track(m) => {
                        let old = std::mem::replace(m.as_mut(), *meta);
                        Ok(Command::SetTrackMeta {
                            id,
                            meta: Box::new(old),
                        })
                    }
                    _ => Err(CommandError::NotATrack(id)),
                }
            }
            Command::Batch(cmds) => {
                let mut inverses = Vec::with_capacity(cmds.len());
                for c in cmds {
                    inverses.push(c.apply(scene)?);
                }
                // Reverse so undo unwinds in the opposite order it was applied.
                inverses.reverse();
                Ok(Command::Batch(inverses))
            }
        }
    }

    /// Apply, returning both the inverse (for undo) **and** a [`ChangeSet`]
    /// describing what changed, so the UI/renderer update incrementally instead
    /// of re-reading the whole document. A `Batch` totals its children's changes.
    pub fn apply_tracked(self, scene: &mut Scene) -> Result<(Command, ChangeSet), CommandError> {
        match self {
            Command::Batch(cmds) => {
                let mut inverses = Vec::with_capacity(cmds.len());
                let mut changes = ChangeSet::new();
                for c in cmds {
                    let (inv, ch) = c.apply_tracked(scene)?;
                    inverses.push(inv);
                    changes.merge(ch);
                }
                inverses.reverse();
                Ok((Command::Batch(inverses), changes))
            }
            other => {
                // Footprint is computed against the pre-apply scene (it needs the
                // soon-to-change state), then the mutation runs.
                let change = other.footprint(scene);
                let inverse = other.apply(scene)?;
                Ok((inverse, change))
            }
        }
    }

    /// What applying this command will touch, evaluated against the **pre-apply**
    /// scene. Best-effort and infallible — a missing target is reported by id and
    /// the real error surfaces from [`Command::apply`]. `Batch` is handled by
    /// [`Command::apply_tracked`], which accumulates its children.
    fn footprint(&self, scene: &Scene) -> ChangeSet {
        let mut cs = ChangeSet::new();
        match self {
            Command::AddLayer { parent, layer, .. } => {
                cs.mark_structural();
                if let Some(p) = parent {
                    cs.touch(*p);
                }
                collect_layer(layer, &mut cs);
            }
            Command::RemoveLayer { id } => {
                cs.mark_structural();
                cs.touch(*id);
                if let Some(parent) = parent_id(scene, *id) {
                    cs.touch(parent);
                }
                if let Some(sub) = scene.layer(*id) {
                    collect_layer(&sub, &mut cs);
                }
            }
            Command::Rename { id, .. } => cs.touch(*id),
            Command::SetTiming {
                id,
                start,
                duration,
            } => {
                cs.touch(*id);
                if let Some(node) = scene.get(*id) {
                    cs.touch_time(node.range); // old span
                }
                cs.touch_time(TimeRange::new(*start, *duration)); // new span
            }
            Command::Reparent {
                id, new_parent, ..
            } => {
                cs.mark_structural();
                cs.touch(*id);
                if let Some(np) = new_parent {
                    cs.touch(*np);
                }
                if let Some(node) = scene.get(*id) {
                    if let Some(parent) = parent_id(scene, *id) {
                        cs.touch(parent);
                    }
                    cs.touch_time(node.range);
                }
            }
            Command::SetTrackMeta { id, .. } => {
                cs.touch(*id);
                // Lane state — especially solo — can change compositing/audio for
                // the whole timeline, so invalidate the full duration.
                cs.touch_time(TimeRange::new(Time::ZERO, scene.duration));
            }
            Command::Batch(_) => {}
        }
        cs
    }
}

/// Stable id of a node's parent, if any.
fn parent_id(scene: &Scene, id: SceneId) -> Option<SceneId> {
    let node = scene.get(id)?;
    let pk = node.parent?;
    scene.node(pk).map(|p| p.id())
}

/// Add every id and timeline span in a portable subtree to the change set.
fn collect_layer(layer: &Layer, cs: &mut ChangeSet) {
    cs.touch(layer.id);
    cs.touch_time(TimeRange::new(layer.start, layer.duration));
    for child in &layer.children {
        collect_layer(child, cs);
    }
}

/// Owns the document, the playback clock, and the undo/redo history.
///
/// History is two stacks of *inverse* commands. Memory is proportional to the
/// edits made, not the document size.
pub struct Editor {
    pub document: Document,
    pub playhead: Time,
    undo: Vec<Command>,
    redo: Vec<Command>,
}

impl Editor {
    pub fn new(document: Document) -> Self {
        Self {
            document,
            playhead: Time::ZERO,
            undo: Vec::new(),
            redo: Vec::new(),
        }
    }

    /// Apply an edit, recording its inverse for undo. A new edit clears redo.
    /// Returns the [`ChangeSet`] so the caller can push granular updates to the
    /// UI/renderer instead of re-reading the document.
    pub fn apply(&mut self, cmd: Command) -> Result<ChangeSet, CommandError> {
        let (inverse, change) = cmd.apply_tracked(&mut self.document.scene)?;
        self.undo.push(inverse);
        self.redo.clear();
        Ok(change)
    }

    /// Move the playhead. This is session state, not a document edit, so it is
    /// intentionally **not** part of undo history.
    pub fn set_playhead(&mut self, t: Time) {
        self.playhead = t;
    }

    /// Freeze the current scene into an immutable, `Send + Sync` snapshot for the
    /// render/export thread(s). Publish it via a
    /// [`SharedScene`](crate::SharedScene) after each change.
    pub fn snapshot(&self) -> Arc<SceneSnapshot> {
        Arc::new(SceneSnapshot::new(self.document.scene.clone()))
    }

    /// Undo the last edit, returning what changed (`None` if nothing to undo).
    pub fn undo(&mut self) -> Option<ChangeSet> {
        let inverse = self.undo.pop()?;
        match inverse.apply_tracked(&mut self.document.scene) {
            Ok((redo_cmd, change)) => {
                self.redo.push(redo_cmd);
                Some(change)
            }
            Err(_) => None,
        }
    }

    /// Redo the last undone edit, returning what changed (`None` if none).
    pub fn redo(&mut self) -> Option<ChangeSet> {
        let cmd = self.redo.pop()?;
        match cmd.apply_tracked(&mut self.document.scene) {
            Ok((undo_cmd, change)) => {
                self.undo.push(undo_cmd);
                Some(change)
            }
            Err(_) => None,
        }
    }

    pub fn undo_depth(&self) -> usize {
        self.undo.len()
    }

    pub fn redo_depth(&self) -> usize {
        self.redo.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scene::{Layer, LayerContent, Scene};
    use cutlass_anim::{Fps, Time};

    fn group(name: &str) -> Layer {
        Layer::new(name, LayerContent::Group, Time::from_seconds(1.0))
    }

    /// Append a layer at the root (boxes the payload like real call sites do).
    fn add(layer: Layer) -> Command {
        Command::AddLayer {
            parent: None,
            index: None,
            layer: Box::new(layer),
        }
    }

    fn doc() -> Document {
        Document::new("t", Scene::new(100, 100, Fps::from_int(30)))
    }

    #[test]
    fn add_then_undo_redo_restores_scene() {
        let mut ed = Editor::new(doc());
        let layer = group("A");
        let id = layer.id;

        ed.apply(add(layer)).unwrap();
        assert!(ed.document.scene.get(id).is_some());
        assert_eq!(ed.undo_depth(), 1);

        assert!(ed.undo().is_some());
        assert!(ed.document.scene.get(id).is_none());
        assert_eq!(ed.redo_depth(), 1);

        assert!(ed.redo().is_some());
        assert!(ed.document.scene.get(id).is_some());
    }

    #[test]
    fn remove_undo_restores_exact_position_and_subtree() {
        let mut ed = Editor::new(doc());
        let a = group("A");
        let b = group("B");
        let c = group("C");
        let (ia, ib, ic) = (a.id, b.id, c.id);
        for l in [a, b, c] {
            ed.apply(add(l)).unwrap();
        }
        // Remove the middle one, then undo: it must come back in the middle.
        ed.apply(Command::RemoveLayer { id: ib }).unwrap();
        let order: Vec<_> = ed.document.scene.roots().to_vec();
        assert_eq!(order.len(), 2);
        ed.undo();
        let restored: Vec<_> = ed.document.scene.root_nodes().map(|n| n.id()).collect();
        assert_eq!(restored, vec![ia, ib, ic]);
    }

    #[test]
    fn batch_is_one_atomic_undo_step() {
        let mut ed = Editor::new(doc());
        let batch = Command::Batch(vec![add(group("A")), add(group("B"))]);
        ed.apply(batch).unwrap();
        assert_eq!(ed.document.scene.len(), 2);
        assert_eq!(ed.undo_depth(), 1, "a batch is a single undo entry");

        ed.undo();
        assert_eq!(ed.document.scene.len(), 0, "undo reverses the whole batch");
        ed.redo();
        assert_eq!(ed.document.scene.len(), 2);
    }

    #[test]
    fn nested_add_and_reparent_roundtrip() {
        let mut ed = Editor::new(doc());
        let parent = group("P");
        let pid = parent.id;
        let child = group("C");
        let cid = child.id;
        ed.apply(add(parent)).unwrap();
        ed.apply(add(child)).unwrap();
        // Move C under P, then undo → C back at root.
        ed.apply(Command::Reparent {
            id: cid,
            new_parent: Some(pid),
            index: 0,
        })
        .unwrap();
        assert!(ed.document.scene.get(cid).unwrap().parent.is_some());
        ed.undo();
        assert!(ed.document.scene.get(cid).unwrap().parent.is_none());
    }

    #[test]
    fn reparent_into_own_descendant_is_rejected() {
        let mut scene = Scene::new(10, 10, Fps::from_int(30));
        let p = group("P");
        let pid = p.id;
        scene.insert_root(p);
        let c = group("C");
        let cid = c.id;
        scene.insert(Some(pid), None, c).unwrap();
        // Making P a child of C would create a cycle.
        assert!(matches!(
            scene.reparent(pid, Some(cid), 0),
            Err(CommandError::Cycle)
        ));
    }

    #[test]
    fn missing_target_errors() {
        let mut ed = Editor::new(doc());
        let ghost = crate::scene::SceneId::new();
        assert!(matches!(
            ed.apply(Command::RemoveLayer { id: ghost }),
            Err(CommandError::NotFound(_))
        ));
    }

    #[test]
    fn set_track_meta_is_invertible_and_typed() {
        use crate::track::{TrackKind, TrackMeta};
        let mut ed = Editor::new(doc());
        let track_id = ed.document.scene.add_track("Video", TrackKind::Video);

        // Mute the lane, then undo → it's audible again.
        let mut muted = TrackMeta::new(TrackKind::Video);
        muted.enabled = false;
        ed.apply(Command::SetTrackMeta {
            id: track_id,
            meta: Box::new(muted),
        })
        .unwrap();
        assert!(!ed.document.scene.get(track_id).unwrap().track_meta().unwrap().enabled);
        ed.undo();
        assert!(ed.document.scene.get(track_id).unwrap().track_meta().unwrap().enabled);

        // Targeting a non-track node is a typed error.
        let g = group("not a track");
        let gid = g.id;
        ed.apply(add(g)).unwrap();
        assert!(matches!(
            ed.apply(Command::SetTrackMeta {
                id: gid,
                meta: Box::new(TrackMeta::default()),
            }),
            Err(CommandError::NotATrack(_))
        ));
    }

    #[test]
    fn add_reports_structural_change_with_subtree_and_span() {
        let mut ed = Editor::new(doc());
        let mut parent = group("P"); // 0..1s
        let pid = parent.id;
        let child = Layer::new("C", LayerContent::Group, Time::from_seconds(2.0))
            .at(Time::from_seconds(1.0)); // 1..3s
        let cid = child.id;
        parent.children.push(child);

        let change = ed.apply(add(parent)).unwrap();
        assert!(change.structural);
        assert!(change.layers.contains(&pid) && change.layers.contains(&cid));
        let span = change.time.expect("an add touches a timeline span");
        assert_eq!(span.start, Time::ZERO);
        assert_eq!(span.end(), Time::from_seconds(3.0), "hull of 0..1 and 1..3");
    }

    #[test]
    fn set_timing_dirties_union_of_old_and_new_span() {
        let mut ed = Editor::new(doc());
        let g = group("A"); // 0..1s
        let id = g.id;
        ed.apply(add(g)).unwrap();

        let change = ed
            .apply(Command::SetTiming {
                id,
                start: Time::from_seconds(5.0),
                duration: Time::from_seconds(1.0),
            })
            .unwrap();
        assert!(!change.structural, "a timing tweak is not structural");
        assert_eq!(change.layers, vec![id]);
        let span = change.time.unwrap();
        assert_eq!(span.start, Time::ZERO, "covers the vacated span");
        assert_eq!(span.end(), Time::from_seconds(6.0), "and the new one");
    }

    #[test]
    fn batch_change_set_merges_children_and_undo_reports_change() {
        let mut ed = Editor::new(doc());
        let a = group("A");
        let b = group("B");
        let (ia, ib) = (a.id, b.id);

        let change = ed.apply(Command::Batch(vec![add(a), add(b)])).unwrap();
        assert!(change.structural);
        assert!(change.layers.contains(&ia) && change.layers.contains(&ib));

        // Undo/redo also report what they touched.
        let undo = ed.undo().expect("something to undo");
        assert!(undo.structural);
        assert!(ed.redo().expect("something to redo").structural);
    }
}
