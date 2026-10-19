//! The scene graph.
//!
//! ## Two representations, on purpose
//!
//! * [`Layer`] is the **portable subtree** — the shape that commands carry,
//!   templates store, and serialization round-trips (a friendly nested tree).
//! * [`Scene`] is the **runtime arena** — a generational [`slotmap`] of [`Node`]s
//!   linked by fast internal [`NodeId`]s, with a [`SceneId`] → [`NodeId`] index
//!   for O(1) lookup.
//!
//! Why both: the renderer walks the arena every frame and commands target nodes
//! constantly, so we want O(1) id lookup and cheap reparent/reorder (an arena
//! gives both). But the *persisted* identity must be stable across save/load and
//! undo/redo, so commands and storage use the UUID-backed [`SceneId`], not the
//! arena key (which changes when a node is removed and re-added by undo).

use crate::audio::AudioClip;
use crate::command::CommandError;
use crate::timewarp::TimeWarp;
use crate::track::{TrackKind, TrackMeta};
use crate::transform::Transform2D;
use cutlass_anim::{Fps, Time, TimeRange};
use cutlass_effects::EffectGraph;
use cutlass_shapes::ShapeElement;
use cutlass_text::TextBlock;
use serde::{Deserialize, Serialize};
use slotmap::{new_key_type, SlotMap};
use std::collections::HashMap;
use uuid::Uuid;

new_key_type! {
    /// Fast, generational, in-memory handle into the [`Scene`] arena.
    ///
    /// Not stable across undo/remove+re-add and never serialized — use
    /// [`SceneId`] for anything persistent or command-facing.
    pub struct NodeId;
}

/// Stable, persistent identity of a node. Safe to store in commands, templates,
/// and saved documents; survives undo/redo and round-trips through disk.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SceneId(pub Uuid);

impl SceneId {
    pub fn new() -> Self {
        SceneId(Uuid::new_v4())
    }
}

impl Default for SceneId {
    fn default() -> Self {
        Self::new()
    }
}

/// The visual payload of a layer. Each variant is fully data-driven so it
/// serializes into templates and renders identically on every platform.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum LayerContent {
    /// A media clip (video/image) referencing a project source by logical id.
    Media {
        source: String,
        /// Trim into the source.
        in_point: Time,
        out_point: Time,
        /// Optional time remap (speed/ramp/reverse). `None` plays 1× forward.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        warp: Option<TimeWarp>,
    },
    /// An audio-only clip (music, voiceover, SFX) with gain/pan automation and
    /// fades. Boxed for the same packing reasons as text/shape.
    Audio(Box<AudioClip>),
    /// Boxed: text/shape payloads are large; boxing keeps `Node`/`Command`
    /// small so the arena and undo stack pack tightly (cache + memory).
    Text(Box<TextBlock>),
    Shape(Box<ShapeElement>),
    /// A **timeline lane**. A top-level `Track` node's children are the clips on
    /// that lane; lanes stack in compositing order. See [`crate::track`].
    Track(Box<TrackMeta>),
    /// A **composition** container (precomp): groups visual children inside a
    /// clip. Distinct from [`LayerContent::Track`], which is a timeline lane.
    Group,
}

impl LayerContent {
    /// For a media/audio clip, the **source time** to read at clip-local timeline
    /// time `local` (`0` at the clip's start), honoring any [`TimeWarp`]. Returns
    /// `None` for non-clip content (text/shape/group).
    pub fn source_time_at(&self, local: Time) -> Option<Time> {
        match self {
            LayerContent::Media {
                in_point, warp, ..
            } => Some(match warp {
                Some(w) => w.source_at(local, *in_point),
                None => *in_point + local,
            }),
            LayerContent::Audio(c) => Some(match &c.warp {
                Some(w) => w.source_at(local, c.in_point),
                None => c.in_point + local,
            }),
            _ => None,
        }
    }

    /// Instantaneous playback speed of a clip at `local` (`1.0` if un-warped).
    /// `None` for non-clip content.
    pub fn speed_at(&self, local: Time) -> Option<f64> {
        match self {
            LayerContent::Media { warp, .. } => {
                Some(warp.as_ref().map_or(1.0, |w| w.speed_at(local)))
            }
            LayerContent::Audio(c) => Some(c.warp.as_ref().map_or(1.0, |w| w.speed_at(local))),
            _ => None,
        }
    }

    /// Whether this content paints pixels (vs. audio or an organizational lane).
    pub fn is_visual(&self) -> bool {
        matches!(
            self,
            LayerContent::Media { .. }
                | LayerContent::Text(_)
                | LayerContent::Shape(_)
                | LayerContent::Group
        )
    }
}

/// A portable layer subtree: the unit commands carry and templates store.
///
/// This is the *serialized* shape (nested children). At runtime it is flattened
/// into the [`Scene`] arena; on save/extract it is rebuilt from the arena.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Layer {
    pub id: SceneId,
    pub name: String,
    pub content: LayerContent,
    pub transform: Transform2D,
    #[serde(default)]
    pub effects: EffectGraph,
    /// When this layer is visible on the global timeline.
    pub start: Time,
    pub duration: Time,
    #[serde(default)]
    pub children: Vec<Layer>,
}

impl Layer {
    pub fn new(name: impl Into<String>, content: LayerContent, duration: Time) -> Self {
        Self {
            id: SceneId::new(),
            name: name.into(),
            content,
            transform: Transform2D::default(),
            effects: EffectGraph::default(),
            start: Time::ZERO,
            duration,
            children: Vec::new(),
        }
    }

    pub fn at(mut self, start: Time) -> Self {
        self.start = start;
        self
    }
}

/// A runtime node in the arena. Holds the same payload as [`Layer`] but links to
/// children/parent by fast [`NodeId`] instead of owning a nested `Vec<Layer>`.
#[derive(Debug, Clone, PartialEq)]
pub struct Node {
    pub persistent_id: SceneId,
    pub parent: Option<NodeId>,
    pub children: Vec<NodeId>,
    pub name: String,
    pub content: LayerContent,
    pub transform: Transform2D,
    pub effects: EffectGraph,
    pub range: TimeRange,
}

impl Node {
    #[inline]
    pub fn id(&self) -> SceneId {
        self.persistent_id
    }

    /// Whether this node is active at the given global time.
    #[inline]
    pub fn is_active_at(&self, t: Time) -> bool {
        self.range.contains(t)
    }

    /// Whether this node carries an audio clip.
    #[inline]
    pub fn is_audio(&self) -> bool {
        matches!(self.content, LayerContent::Audio(_))
    }

    /// Whether this node is a timeline lane.
    #[inline]
    pub fn is_track(&self) -> bool {
        matches!(self.content, LayerContent::Track(_))
    }

    /// The lane state if this node is a track.
    #[inline]
    pub fn track_meta(&self) -> Option<&TrackMeta> {
        match &self.content {
            LayerContent::Track(m) => Some(m),
            _ => None,
        }
    }

    /// Mutable lane state if this node is a track.
    #[inline]
    pub fn track_meta_mut(&mut self) -> Option<&mut TrackMeta> {
        match &mut self.content {
            LayerContent::Track(m) => Some(m),
            _ => None,
        }
    }
}

/// What [`Scene::remove`] hands back so the operation is invertible: enough to
/// reconstruct the exact subtree at its exact position.
#[derive(Debug, Clone, PartialEq)]
pub struct RemovedLayer {
    pub parent: Option<SceneId>,
    pub index: usize,
    pub layer: Layer,
}

/// A composition: an arena of layers rendered at a fixed resolution / fps.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(into = "SceneDto", from = "SceneDto")]
pub struct Scene {
    pub width: u32,
    pub height: u32,
    pub fps: Fps,
    /// Total timeline duration.
    pub duration: Time,
    nodes: SlotMap<NodeId, Node>,
    /// Top-level nodes (tracks, or bare clips) in **compositing order**:
    /// index 0 is painted first (backmost), the last is frontmost. This is the
    /// one paint-order invariant; the renderer honors it via
    /// [`Scene::compositing_clips_at`].
    roots: Vec<NodeId>,
    index: HashMap<SceneId, NodeId>,
}

/// Semantic equality: two scenes are equal when their logical layer trees match,
/// regardless of internal arena key assignment. (`SlotMap` has no `PartialEq`,
/// and key identity is an implementation detail anyway.)
impl PartialEq for Scene {
    fn eq(&self, other: &Self) -> bool {
        self.width == other.width
            && self.height == other.height
            && self.fps == other.fps
            && self.duration == other.duration
            && self.roots.len() == other.roots.len()
            && self
                .roots
                .iter()
                .zip(&other.roots)
                .all(|(&a, &b)| self.layer_snapshot(a) == other.layer_snapshot(b))
    }
}

impl Scene {
    pub fn new(width: u32, height: u32, fps: Fps) -> Self {
        Self {
            width,
            height,
            fps,
            duration: Time::ZERO,
            nodes: SlotMap::with_key(),
            roots: Vec::new(),
            index: HashMap::new(),
        }
    }

    /// Number of nodes in the scene (all depths).
    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }

    /// Top-level node keys in paint order.
    pub fn roots(&self) -> &[NodeId] {
        &self.roots
    }

    /// Iterate top-level nodes in paint order.
    pub fn root_nodes(&self) -> impl Iterator<Item = &Node> {
        self.roots.iter().map(move |&k| &self.nodes[k])
    }

    /// Snapshot a subtree by stable id into a portable [`Layer`]
    /// (non-destructive; used by tooling, agents, and templates).
    pub fn layer(&self, id: SceneId) -> Option<Layer> {
        self.index.get(&id).map(|&k| self.layer_snapshot(k))
    }

    /// Borrow the arena node behind a fast key.
    #[inline]
    pub fn node(&self, key: NodeId) -> Option<&Node> {
        self.nodes.get(key)
    }

    /// O(1) lookup by stable id.
    #[inline]
    pub fn get(&self, id: SceneId) -> Option<&Node> {
        self.index.get(&id).and_then(|&k| self.nodes.get(k))
    }

    /// O(1) mutable lookup by stable id.
    #[inline]
    pub fn get_mut(&mut self, id: SceneId) -> Option<&mut Node> {
        let key = *self.index.get(&id)?;
        self.nodes.get_mut(key)
    }

    /// Insert a layer subtree under `parent` (or at the root if `None`) at
    /// `index` (or appended if `None`). Returns the root's stable id.
    pub fn insert(
        &mut self,
        parent: Option<SceneId>,
        index: Option<usize>,
        layer: Layer,
    ) -> Result<SceneId, CommandError> {
        let parent_key = match parent {
            Some(pid) => Some(*self.index.get(&pid).ok_or(CommandError::NotFound(pid))?),
            None => None,
        };
        let root_id = layer.id;
        let node_key = self.build_subtree(parent_key, layer);
        let siblings = match parent_key {
            Some(pk) => &mut self.nodes[pk].children,
            None => &mut self.roots,
        };
        let at = index.unwrap_or(siblings.len()).min(siblings.len());
        siblings.insert(at, node_key);
        Ok(root_id)
    }

    /// Append a top-level layer subtree (convenience for the common root case).
    pub fn insert_root(&mut self, layer: Layer) -> SceneId {
        self.insert(None, None, layer)
            .expect("root insert has no missing parent")
    }

    /// Remove a subtree, returning everything needed to put it back.
    pub fn remove(&mut self, id: SceneId) -> Result<RemovedLayer, CommandError> {
        let node_key = *self.index.get(&id).ok_or(CommandError::NotFound(id))?;
        let parent_key = self.nodes[node_key].parent;
        let pos = {
            let siblings = match parent_key {
                Some(pk) => &mut self.nodes[pk].children,
                None => &mut self.roots,
            };
            let pos = siblings
                .iter()
                .position(|&c| c == node_key)
                .ok_or(CommandError::NotFound(id))?;
            siblings.remove(pos);
            pos
        };
        let parent_persistent = parent_key.map(|pk| self.nodes[pk].persistent_id);
        let layer = self.rebuild_layer_owning(node_key);
        Ok(RemovedLayer {
            parent: parent_persistent,
            index: pos,
            layer,
        })
    }

    /// Move a node to a new parent/position. Returns the previous
    /// `(parent, index)` so the move is invertible. Rejects cycles.
    pub fn reparent(
        &mut self,
        id: SceneId,
        new_parent: Option<SceneId>,
        index: usize,
    ) -> Result<(Option<SceneId>, usize), CommandError> {
        let node_key = *self.index.get(&id).ok_or(CommandError::NotFound(id))?;
        let new_parent_key = match new_parent {
            Some(p) => Some(*self.index.get(&p).ok_or(CommandError::NotFound(p))?),
            None => None,
        };
        if let Some(np) = new_parent_key {
            if np == node_key || self.is_descendant(np, node_key) {
                return Err(CommandError::Cycle);
            }
        }
        let old_parent_key = self.nodes[node_key].parent;
        let old_index = {
            let siblings = match old_parent_key {
                Some(pk) => &mut self.nodes[pk].children,
                None => &mut self.roots,
            };
            let pos = siblings
                .iter()
                .position(|&c| c == node_key)
                .ok_or(CommandError::NotFound(id))?;
            siblings.remove(pos);
            pos
        };
        let old_parent_persistent = old_parent_key.map(|pk| self.nodes[pk].persistent_id);
        self.nodes[node_key].parent = new_parent_key;
        let siblings = match new_parent_key {
            Some(pk) => &mut self.nodes[pk].children,
            None => &mut self.roots,
        };
        let at = index.min(siblings.len());
        siblings.insert(at, node_key);
        Ok((old_parent_persistent, old_index))
    }

    /// Stable ids of top-level nodes active at `t`, in paint order. Low-level;
    /// for "what the renderer paints" use [`Scene::compositing_clips_at`], which
    /// understands tracks.
    pub fn active_layers_at(&self, t: Time) -> Vec<SceneId> {
        self.roots
            .iter()
            .filter_map(|&k| {
                let n = &self.nodes[k];
                n.is_active_at(t).then_some(n.persistent_id)
            })
            .collect()
    }

    /// Add an empty timeline lane at the top (front) of the stack and return its
    /// stable id. Clips are added as children of the returned track.
    pub fn add_track(&mut self, name: impl Into<String>, kind: TrackKind) -> SceneId {
        let layer = Layer::new(
            name,
            LayerContent::Track(Box::new(TrackMeta::new(kind))),
            Time::ZERO,
        );
        self.insert_root(layer)
    }

    /// Iterate the top-level tracks (lanes) in compositing order (back to front).
    pub fn tracks(&self) -> impl Iterator<Item = &Node> {
        self.root_nodes().filter(|n| n.is_track())
    }

    /// Whether any track is soloed (which silences all non-soloed tracks).
    pub fn any_solo(&self) -> bool {
        self.tracks()
            .any(|n| n.track_meta().is_some_and(|m| m.solo))
    }

    /// The **visual** clips to paint at `t`, in compositing order (back to
    /// front). Expands live, non-audio tracks into their active visual children
    /// and honors enabled/solo; bare top-level visual nodes (no explicit lane)
    /// are included as-is. This is the renderer's input.
    pub fn compositing_clips_at(&self, t: Time) -> Vec<SceneId> {
        let any_solo = self.any_solo();
        let mut out = Vec::new();
        for &k in &self.roots {
            let node = &self.nodes[k];
            match &node.content {
                LayerContent::Track(meta) => {
                    if meta.kind.is_audio() || !meta.is_live(any_solo) {
                        continue;
                    }
                    for &c in &node.children {
                        let child = &self.nodes[c];
                        if child.content.is_visual() && child.is_active_at(t) {
                            out.push(child.persistent_id);
                        }
                    }
                }
                content if content.is_visual() && node.is_active_at(t) => {
                    out.push(node.persistent_id);
                }
                _ => {}
            }
        }
        out
    }

    /// Stable ids of audio clips to mix at `t`, honoring track enabled/solo.
    /// Walks subtrees but skips silenced (non-live) track lanes. Order is
    /// unspecified — the mixer sums, which is order-independent.
    pub fn active_audio_at(&self, t: Time) -> Vec<SceneId> {
        let any_solo = self.any_solo();
        let mut out = Vec::new();
        for &k in &self.roots {
            self.collect_audio(k, t, any_solo, &mut out);
        }
        out
    }

    /// First node (DFS preorder, in paint order) matching `pred`.
    pub fn find_node<F: Fn(&Node) -> bool>(&self, pred: F) -> Option<&Node> {
        fn walk<'a, F: Fn(&Node) -> bool>(
            scene: &'a Scene,
            key: NodeId,
            pred: &F,
        ) -> Option<&'a Node> {
            let node = &scene.nodes[key];
            if pred(node) {
                return Some(node);
            }
            node.children.iter().find_map(|&c| walk(scene, c, pred))
        }
        self.roots.iter().find_map(|&k| walk(self, k, &pred))
    }

    // --- internals ---------------------------------------------------------

    /// Collect active audio under `key`, skipping non-live track lanes.
    fn collect_audio(&self, key: NodeId, t: Time, any_solo: bool, out: &mut Vec<SceneId>) {
        let node = &self.nodes[key];
        if let LayerContent::Track(meta) = &node.content {
            if !meta.is_live(any_solo) {
                return;
            }
        }
        if node.is_audio() && node.is_active_at(t) {
            out.push(node.persistent_id);
        }
        for &c in &node.children {
            self.collect_audio(c, t, any_solo, out);
        }
    }

    fn build_subtree(&mut self, parent: Option<NodeId>, layer: Layer) -> NodeId {
        let Layer {
            id,
            name,
            content,
            transform,
            effects,
            start,
            duration,
            children,
        } = layer;
        let node_key = self.nodes.insert(Node {
            persistent_id: id,
            parent,
            children: Vec::new(),
            name,
            content,
            transform,
            effects,
            range: TimeRange::new(start, duration),
        });
        self.index.insert(id, node_key);
        let mut child_keys = Vec::with_capacity(children.len());
        for child in children {
            child_keys.push(self.build_subtree(Some(node_key), child));
        }
        self.nodes[node_key].children = child_keys;
        node_key
    }

    /// Remove `node_key` and its descendants from the arena, rebuilding the
    /// owned [`Layer`] subtree (used by `remove`).
    fn rebuild_layer_owning(&mut self, node_key: NodeId) -> Layer {
        let node = self.nodes.remove(node_key).expect("node exists");
        self.index.remove(&node.persistent_id);
        let children = node
            .children
            .iter()
            .map(|&c| self.rebuild_layer_owning(c))
            .collect();
        Layer {
            id: node.persistent_id,
            name: node.name,
            content: node.content,
            transform: node.transform,
            effects: node.effects,
            start: node.range.start,
            duration: node.range.duration,
            children,
        }
    }

    /// Non-destructive snapshot of a subtree into a [`Layer`] (used by save).
    fn layer_snapshot(&self, node_key: NodeId) -> Layer {
        let node = &self.nodes[node_key];
        Layer {
            id: node.persistent_id,
            name: node.name.clone(),
            content: node.content.clone(),
            transform: node.transform.clone(),
            effects: node.effects.clone(),
            start: node.range.start,
            duration: node.range.duration,
            children: node.children.iter().map(|&c| self.layer_snapshot(c)).collect(),
        }
    }

    fn is_descendant(&self, maybe_child: NodeId, ancestor: NodeId) -> bool {
        let mut cur = self.nodes[maybe_child].parent;
        while let Some(p) = cur {
            if p == ancestor {
                return true;
            }
            cur = self.nodes[p].parent;
        }
        false
    }
}

/// Friendly, stable serialization shape: a nested layer tree. Decouples the
/// on-disk format from the runtime arena (the start of the schema/runtime split
/// the architecture review calls for).
#[derive(Serialize, Deserialize)]
struct SceneDto {
    width: u32,
    height: u32,
    fps: Fps,
    duration: Time,
    #[serde(default)]
    layers: Vec<Layer>,
}

impl From<Scene> for SceneDto {
    fn from(s: Scene) -> Self {
        let layers = s.roots.iter().map(|&r| s.layer_snapshot(r)).collect();
        SceneDto {
            width: s.width,
            height: s.height,
            fps: s.fps,
            duration: s.duration,
            layers,
        }
    }
}

impl From<SceneDto> for Scene {
    fn from(d: SceneDto) -> Self {
        let mut scene = Scene::new(d.width, d.height, d.fps);
        scene.duration = d.duration;
        for layer in d.layers {
            // Root inserts never fail (no missing parent).
            let _ = scene.insert(None, None, layer);
        }
        scene
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio::AudioClip;
    use crate::timewarp::TimeWarp;

    #[test]
    fn source_time_maps_through_warp_for_clips() {
        // 2× media clip: local 1s reads source 2s.
        let media = LayerContent::Media {
            source: "v".into(),
            in_point: Time::ZERO,
            out_point: Time::from_seconds(10.0),
            warp: Some(TimeWarp::Constant(2.0)),
        };
        assert_eq!(
            media.source_time_at(Time::from_seconds(1.0)),
            Some(Time::from_seconds(2.0))
        );
        assert!((media.speed_at(Time::from_seconds(1.0)).unwrap() - 2.0).abs() < 1e-9);

        // Un-warped audio: identity offset from the in-point.
        let audio = LayerContent::Audio(Box::new(AudioClip::new(
            "a",
            Time::from_seconds(1.0),
            Time::from_seconds(5.0),
        )));
        assert_eq!(
            audio.source_time_at(Time::from_seconds(2.0)),
            Some(Time::from_seconds(3.0))
        );

        // Non-clip content has no source mapping.
        assert_eq!(LayerContent::Group.source_time_at(Time::ZERO), None);
        assert_eq!(LayerContent::Group.speed_at(Time::ZERO), None);
    }

    fn audio_layer(name: &str, start: f64, dur: f64) -> Layer {
        let dur = Time::from_seconds(dur);
        Layer::new(
            name,
            LayerContent::Audio(Box::new(AudioClip::new(name, Time::ZERO, dur))),
            dur,
        )
        .at(Time::from_seconds(start))
    }

    #[test]
    fn active_audio_finds_nested_clips_in_their_time_window() {
        let mut s = Scene::new(100, 100, Fps::from_int(30));
        // A group acting as an audio "track" with two clips back to back.
        let track = Layer::new("Music track", LayerContent::Group, Time::from_seconds(10.0));
        let tid = s.insert_root(track);
        s.insert(Some(tid), None, audio_layer("A", 0.0, 2.0)).unwrap(); // [0,2)
        s.insert(Some(tid), None, audio_layer("B", 2.0, 2.0)).unwrap(); // [2,4)

        let at1 = s.active_audio_at(Time::from_seconds(1.0));
        assert_eq!(at1.len(), 1, "one clip active mid-A");
        assert_eq!(s.get(at1[0]).unwrap().name, "A");

        assert_eq!(s.active_audio_at(Time::from_seconds(2.5)).len(), 1); // into B
        assert!(s.active_audio_at(Time::from_seconds(5.0)).is_empty()); // past both
    }

    fn name_of(s: &Scene, id: SceneId) -> String {
        s.get(id).unwrap().name.clone()
    }

    #[test]
    fn compositing_respects_track_order_enable_and_solo() {
        let mut s = Scene::new(100, 100, Fps::from_int(30));
        let five = Time::from_seconds(5.0);

        // Back-to-front: BG (video) → Main (video) → Music (audio).
        let bg = s.add_track("BG", TrackKind::Video);
        s.insert(Some(bg), None, Layer::new("bg", LayerContent::Group, five))
            .unwrap();
        let main = s.add_track("Main", TrackKind::Video);
        s.insert(Some(main), None, Layer::new("clip", LayerContent::Group, five))
            .unwrap();
        let music = s.add_track("Music", TrackKind::Audio);
        s.insert(
            Some(music),
            None,
            Layer::new(
                "music",
                LayerContent::Audio(Box::new(AudioClip::new("m", Time::ZERO, five))),
                five,
            ),
        )
        .unwrap();

        let t = Time::from_seconds(1.0);
        // Paint order = track order; audio lane excluded from compositing.
        let paint: Vec<_> = s
            .compositing_clips_at(t)
            .into_iter()
            .map(|id| name_of(&s, id))
            .collect();
        assert_eq!(paint, vec!["bg", "clip"]);
        // Audio comes only from the (live) audio lane.
        assert_eq!(s.active_audio_at(t).len(), 1);

        // Mute BG → it drops out of compositing.
        s.get_mut(bg).unwrap().track_meta_mut().unwrap().enabled = false;
        let paint: Vec<_> = s
            .compositing_clips_at(t)
            .into_iter()
            .map(|id| name_of(&s, id))
            .collect();
        assert_eq!(paint, vec!["clip"]);

        // Re-enable BG, then solo Main → only Main composites; the muted music
        // lane is also silenced by the solo.
        s.get_mut(bg).unwrap().track_meta_mut().unwrap().enabled = true;
        s.get_mut(main).unwrap().track_meta_mut().unwrap().solo = true;
        let paint: Vec<_> = s
            .compositing_clips_at(t)
            .into_iter()
            .map(|id| name_of(&s, id))
            .collect();
        assert_eq!(paint, vec!["clip"]);
        assert!(s.active_audio_at(t).is_empty(), "solo silences other lanes");
    }
}
