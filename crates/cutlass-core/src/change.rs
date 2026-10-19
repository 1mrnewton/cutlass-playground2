//! Granular change description.
//!
//! Applying an edit reports a [`ChangeSet`] — *which* layers changed, *what*
//! timeline span is now dirty, and whether the tree shape changed — so the UI
//! and renderer update incrementally instead of re-reading the whole document
//! per keystroke:
//!
//! * the **layer panel** rebuilds only when `structural`, otherwise refreshes
//!   the listed `layers`;
//! * the UI re-fetches only the dirty subtrees (`Session::layer_json`);
//! * the **renderer** invalidates cached frames only inside `time`.
//!
//! A `ChangeSet` is cost-proportional to the edit, never to the document.

use crate::scene::SceneId;
use cutlass_anim::TimeRange;
use serde::{Deserialize, Serialize};

/// The footprint of one applied edit (or an undo/redo of one).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ChangeSet {
    /// Stable ids of layers whose props, content, or position changed. For a
    /// structural edit this includes the affected subtree(s) and their parent.
    pub layers: Vec<SceneId>,
    /// The union of timeline spans the edit affected, if any — the region the
    /// renderer/timeline must invalidate. `None` means "no timed region"
    /// (e.g. a pure rename).
    pub time: Option<TimeRange>,
    /// Whether the tree shape changed (insert / remove / reparent). The UI
    /// rebuilds its layer tree only when this is set.
    pub structural: bool,
}

impl ChangeSet {
    pub fn new() -> Self {
        Self::default()
    }

    /// Mark a layer id dirty (deduplicated; order preserved).
    pub fn touch(&mut self, id: SceneId) {
        if !self.layers.contains(&id) {
            self.layers.push(id);
        }
    }

    /// Grow the dirty timeline span to also cover `range`.
    pub fn touch_time(&mut self, range: TimeRange) {
        self.time = Some(match self.time {
            Some(cur) => cur.union(range),
            None => range,
        });
    }

    pub fn mark_structural(&mut self) {
        self.structural = true;
    }

    /// Fold another change set into this one (used to total a `Batch`).
    pub fn merge(&mut self, other: ChangeSet) {
        for id in other.layers {
            self.touch(id);
        }
        if let Some(r) = other.time {
            self.touch_time(r);
        }
        self.structural |= other.structural;
    }

    pub fn is_empty(&self) -> bool {
        self.layers.is_empty() && self.time.is_none() && !self.structural
    }
}
