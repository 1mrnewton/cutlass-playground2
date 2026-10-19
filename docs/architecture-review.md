# Cutlass — Critical Architecture & Foundation Review

> **Status (remediation in progress).** The data-structure spine has been
> rebuilt per this review:
>
> - ✅ **C1 Time** — replaced `f64` seconds with exact integer-tick time in a new
>   `cutlass-time` crate (flicks base; `Fps`/`TimeRange`). Frame math is lossless;
>   `Time` is `Eq`/`Ord`/`Hash`.
> - ✅ **C2 / M1 Scene graph** — arena (`slotmap` `NodeId`) + stable `SceneId`
>   index, O(1) lookup, O(1) cycle-checked reparent.
> - ✅ **C3 / M5 Undo** — invertible commands (`apply` returns the inverse) +
>   atomic `Command::Batch`; no more whole-document clones.
> - ✅ **M3** — removed the `cached_frame` handle from the document model; `core`
>   no longer depends on `media`.
> - ✅ **C4** — wire format decoupled from runtime: `Scene` serializes through a
>   `SceneDto`, and loading now runs a **forward-migration pipeline** (parse →
>   `serde_json::Value` → migrate v(n)→v(n+1) → deserialize). A typed
>   `DocumentError` rejects future-version files; a missing version loads as v1;
>   the additive forward-compat policy (no `deny_unknown_fields`) is tested. The
>   production migration chain is empty at v1 but the engine is fully exercised
>   with synthetic steps.
> - ✅ **C5 Animation model** — keyframes now carry After Effects / Lottie
>   temporal handles (`in`/`out` + `Interp`), position keyframes carry spatial
>   handles (`to`/`ti`) for motion paths, and geometry animates via
>   `Track<BezierPath>` (path morphing). A new `cutlass-lottie` crate imports real
>   Lottie files (validated against `test-assets/Welcome.json`: 60fps comp,
>   morphing 32-vertex path across 4 keyframes with correct tangents).
> - ✅ **H1 / H2 Threading + render snapshot** — the renderer now consumes an
>   immutable, `Send + Sync` `SceneSnapshot` (owns its `Scene`, hands out only
>   shared refs) instead of the editable scene. `SharedScene` is a lock-free
>   (`arc-swap`) single-writer/many-reader handoff: the edit thread `publish`es a
>   snapshot per *edit*, render/export threads `latest()` per *frame* without
>   blocking. Wired through `Editor::snapshot()` and the `ffi::Session` boundary;
>   proven with a real render thread in the CLI harness.
> - ✅ **H3 Audio model** — audio is now first-class: `LayerContent::Audio`
>   (`AudioClip`) on the timeline with sample-accurate `Time`, gain/pan
>   **automation** as `cutlass-anim` `Track`s, mute, and fades (`FadeCurve`:
>   linear / equal-power / exp / log). Pure, tested mix math
>   (`AudioParams::stereo_gain_at`, equal-power pan, dB→linear) gives "preview ==
>   export" for audio; `Scene::active_audio_at` feeds the (future) mixer off a
>   snapshot. Waveform peaks live in `cutlass-media` (`WaveformPeaks`) as a
>   derived cache, kept out of the model.
> - ✅ **H4 Time-remap / speed** — clips carry an optional `TimeWarp`
>   (`Constant` multiplier or keyframed `Ramp`) answering "timeline time → source
>   time". Speed ramps integrate **analytically** (trapezoid area over linear
>   segments, in exact ticks), so slow-mo→fast, reverse, and freeze all work
>   without A/V drift. One entry point — `LayerContent::source_time_at` /
>   `speed_at` — for the decoder/mixer.
> - ✅ **H5 Tracks / lanes** — explicit **timeline vs. composition** split:
>   `LayerContent::Track` is a top-level lane (kind + enable/lock/solo/blend)
>   whose children are clips; `Group` now means *only* a composition/precomp
>   inside a clip. Paint order is the typed, documented `roots` order (back to
>   front) surfaced by `Scene::compositing_clips_at`; `active_audio_at` and
>   compositing both honor mute/solo. Lane state edits via the invertible
>   `Command::SetTrackMeta`.
> - ✅ **H6 Granular change events** — applying an edit now returns a
>   `ChangeSet` (dirty layer ids + the union of dirty timeline spans +
>   `structural`). `Editor::apply/undo/redo` surface it; the FFI emits it as
>   `Event::Changed` and exposes `Session::layer_json(id)` for per-subtree reads
>   — no more whole-document re-serialize per keystroke.
> - ✅ **H7 Color management** — new `cutlass-color` leaf crate fixes the working
>   space (linear, Rec.709/D65) and splits the two axes: `Primaries` (gamut +
>   matrices) and `TransferFunction` (sRGB/Rec.709/PQ/HLG). Encode/decode happen
>   only at the boundary (`ColorSpace::decode/encode`); the document carries a
>   `ColorConfig` (working + output), so SDR and HDR (Rec.2020+PQ/HLG) are
>   explicit and serialized.
> - ✅ **G1** — tests landed across time math (incl. range union), color
>   (transfer round-trips, gamut→D65, full sRGB↔HDR pipeline), track sampling
>   (incl. bezier + spatial), arena ops, inverse-undo, batch atomicity, serde
>   round-trip, **schema migration** (chain/missing/future/gap), path morph,
>   Lottie import, the cross-thread snapshot handoff, audio mix math / waveform
>   peaks, time-remap integration, track compositing/solo, and change-set
>   footprints. ~71 passing; clippy clean.
>
> Still open: a full job/scheduler runtime and `!Send` model isolation (rest of
> H1), and the **M/G items** below.

> Scope: an honest, adversarial review of the current foundation against the
> stated goal — a FOSS, AI-native, 6-platform CapCut alternative expected to
> exceed **300k LOC**. The bar here is not "does it compile" (it does, cleanly);
> it's "will this primitive survive 300k lines built on top of it, or will it
> force a painful rewrite?"
>
> **"Blast radius"** = how much code a *late* fix touches. High blast radius +
> demo-grade implementation = fix it now.

---

## Verdict

The **macro-architecture is sound** and worth keeping:

- Engine/UI split with a thin native shell per platform. ✅
- Data-driven document ("everything is data"). ✅
- Command bus with the AI agent as just another command source. ✅
- Clean, acyclic crate DAG. ✅ (mostly — one leak, see M3)

The **micro-foundation is demo-grade** in several load-bearing primitives. Five
are *architecturally critical*: they sit under everything and are expensive to
change once thousands of call sites assume them. None are visible as bugs today
because there's no renderer and no real data — which is exactly why now is the
time to fix them.

The single most important realization: **`test-assets/Welcome.json` is a Lottie
file** (After Effects → Bodymovin). It animates **bezier paths** and uses
**per-keyframe in/out tangents** and **spatial motion-path tangents**. If
Cutlass is to import Lottie/AE-style stickers, text presets, and templates (the
heart of a CapCut competitor), the current animation model **cannot represent
the data it must consume**. That alone reframes `cutlass-anim` from "done" to
"redesign before building on it."

---

## Severity index

| ID | Issue | Severity | Blast radius |
|----|-------|----------|--------------|
| C1 | ✅ Time is `f64` seconds (drift, non-deterministic, not `Eq`/`Ord`) | Critical | Every crate |
| C2 | ✅ Scene graph = `Vec<Layer>` + recursive O(n) lookup | Critical | core, render, agent, ffi |
| C3 | ✅ Undo = full-document `clone()` per command, unbounded, no coalescing | Critical | core, ffi, agent, UI |
| C4 | ✅ Persisted format **is** the in-memory struct layout (no schema/DTO split) | Critical | core, every saved doc/template |
| C5 | ✅ Animation model can't represent Lottie/AE (tangents, motion paths, animated paths) | Critical | anim, shapes, text, render |
| H1 | ◐ No concurrency/threading model (render/audio/export threads, `!Send` models) | High | core, render, media, ml, ffi |
| H2 | ✅ Renderer consumes the *editable* `Scene` (no immutable render snapshot) | High | core, render |
| H3 | ✅ Audio absent from the document model | High | core, media, render, anim |
| H4 | ✅ No time-remap/speed model (speed curves, optical flow have no home) | High | core, media, anim |
| H5 | ✅ No track/lane concept; `Group` redundant; paint-order invariant unstated | High | core, render, ffi |
| H6 | ✅ Change notifications are whole-document re-serialize; no diffs/dirty flags | High | ffi, core, UI |
| H7 | ✅ Color management pipeline absent (despite "float color / HDR-ready") | High | render, effects, every shader |
| M1 | `SceneId(Uuid)` as the hot-path key (16B random) | Medium | core, render |
| M2 | Determinism promises vs `f32`/transcendental reality | Medium | render, CI |
| M3 | `core → media` layering leak (`cached_frame: FrameHandle` in document) | Medium | core, media |
| M4 | Stringly-typed errors; `unwrap_or_default()` swallows errors at FFI | Medium | all |
| M5 | No command batch/transaction (one user action ⇒ many undo steps) | Medium | core, agent, ffi |
| M6 | Project/package I/O unfactored (filesystem will leak into core) | Medium | core |
| M7 | Math precision impedance (`Time` f64 vs transforms f32) | Medium | core, render |
| G1 | Zero tests; no property tests for math; no deserializer fuzzing | Gap | all |
| G2 | No CI; golden-image harness described but absent | Gap | all |
| G3 | Security: untrusted templates/models (path traversal, resource limits, shaders) | Gap | core, ml, render |
| G4 | Repo/workspace identity is `cutlass-playground2` | Gap | repo |
| G5 | No benchmarks for the hot paths being committed to | Gap | all |

---

## Critical issues (fix before building further)

### C1 — `Time` is `f64` seconds

`cutlass_anim::Time(pub f64)` in seconds is the spine of the whole system
(anim, core, render, media, agent, ffi all consume it). Problems:

- **Float drift.** Splits/trims/ripple edits computed by float arithmetic
  accumulate error. `0.1 + 0.2 != 0.3`. Over a long timeline this produces
  off-by-one-frame gaps, overlaps, and edits that don't line up.
- **Not `Eq`/`Ord`/`Hash`.** Can't key a map/btree by time, can't sort reliably,
  can't compare for exact frame equality — all things an editor does constantly.
- **Undermines determinism.** "Preview == export" and golden-image CI assume
  frame-exact reproducibility; `f64` seconds + per-platform float rounding does
  not give it.

**Every professional NLE uses rational/integer time**, not float seconds
(FCPXML rational `1001/30000s`; OpenTimelineIO `RationalTime{value, rate}`; Avid
edit units). 

**Target design** — rational time + an explicit frame/tick base:

```rust
/// Exact time as a rational number of seconds. `value / rate` seconds.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct RationalTime { value: i64, rate: u32 } // e.g. 1001/30000

impl RationalTime {
    pub fn from_frame(frame: i64, fps: Fps) -> Self { /* exact */ }
    pub fn to_frame_floor(self, fps: Fps) -> i64 { /* exact */ }
    pub fn rescale_to(self, rate: u32) -> Self { /* exact */ }
}
```

Keep a cheap `f64` "seconds" accessor for shader uniforms / UI display, but the
**source of truth is exact**. This is the highest-leverage change and it touches
everything, so do it first.

### C2 — Scene graph is `Vec<Layer>` with recursive lookup

`Scene { layers: Vec<Layer> }`, children nested inline, and `find_mut` does a
recursive O(n) tree walk. Every command targets a `SceneId`, so **every command
is an O(n) traversal**, and an agent emitting thousands of commands is O(n·m).

Worse, the inline-tree shape makes routine editor operations hard or impossible
without fighting the borrow checker:

- No parent pointer (can't ask "what's my parent / siblings / z-index?").
- Reparenting / reordering a node = remove-from-here + insert-there across two
  mutable borrows of the same tree.
- No stable handle independent of position.

**This is the standard scene-graph data-structure decision**, and the standard
answer is an **arena / generational slotmap** with id-based parent/child links:

```rust
slotmap::new_key_type! { pub struct LayerKey; }

pub struct SceneGraph {
    nodes: SlotMap<LayerKey, LayerData>,
    roots: Vec<LayerKey>,
}
pub struct LayerData {
    parent: Option<LayerKey>,
    children: Vec<LayerKey>,
    persistent_id: SceneId, // stable UUID for persistence (see M1)
    // ...content, transform, timing...
}
```

O(1) lookup, trivial reparent/reorder, no borrow fights, cache-friendlier.
Retrofitting this after 300k lines assume `Vec<Layer>` + recursion is brutal;
it's cheap now.

### C3 — Undo clones the entire document per command

```rust
let snapshot = self.document.clone(); // whole Scene, every layer, every track
// ...apply...
self.undo_stack.push(snapshot);       // unbounded
```

- **O(document size) time and memory per edit.** A drag emitting `SetTiming`
  ~60×/s creates 60 full-document deep copies per second.
- **Unbounded stack** (no cap, no memory budget).
- **No coalescing** — a continuous gesture should be *one* undo step, not 60.

The comment claims this can be swapped "without changing the public API." That's
optimistic: an invertible-command or persistent-data-structure model changes the
*semantics* commands must satisfy (invertibility, merge/coalesce rules), so it
must be decided now, while the command vocabulary is small.

**Two viable foundations** (pick one early):
1. **Invertible commands**: each command produces its inverse; undo stack stores
   `(Command, Inverse)` deltas. Add a `Transaction` that groups + coalesces.
2. **Persistent/immutable document** with structural sharing (`im`/`rpds`):
   snapshots become O(changed) via shared structure; undo = keep old root.

Either way, add **edit transactions** (begin/commit) and **gesture coalescing**
to the foundation.

### C4 — The persisted format *is* the in-memory layout ✅

`Document::to_json` / `from_json` serde-serialized the **runtime structs
directly**, welding the on-disk format to the in-memory representation: renaming
a field or restructuring `Layer` would break every saved document and published
template. `SCHEMA_VERSION = 1` was a constant with no machinery.

**Resolved.** Wire format and runtime types are now decoupled at both ends:

- **Wire ≠ runtime arena.** `Scene` already serializes through a `SceneDto`
  (nested `Layer`s), so the on-disk tree is independent of the slotmap arena.
  The `cached_frame` runtime leak was removed back in **M3**.
- **A forward-migration pipeline** (`cutlass-core::migrate`). `from_json` =
  parse → `serde_json::Value` → run ordered migration `Step`s v(n)→v(n+1) →
  `serde_json::from_value` into current types. `to_json` always writes the
  current `SCHEMA_VERSION`.
- **A typed `DocumentError`** (also progress on **M4**): `Json`, `FromFuture`
  (a newer-than-supported file is rejected, never mis-parsed), `NoMigration`
  (a gap in the chain), and `Migration` (a step failed). A document missing
  `schema_version` loads as v1.
- **Stated forward-compat policy**: *additive within a version* — no
  `deny_unknown_fields`, new fields are `#[serde(default)]` — so a slightly
  newer same-version file still opens on an older build; only breaking changes
  bump the version.
- The **migration engine is generic over the step set** so it is fully tested
  with synthetic v1→v2→v3 chains even though the production chain is empty at v1.
  Each future breaking change appends one `Step` and bumps `SCHEMA_VERSION`.
- The **Lottie importer** (C5) already plugs in the same way: external format →
  `Value`/builder → runtime, identical to native load.

Validated by the `migrate` engine tests (chain/missing-version/future/gap/no-op),
the `document` load tests (legacy/ future / unknown-field), and the CLI harness
(save → load round-trip prints the reloaded schema version + node count).
Deferred: explicit per-version DTO *type* snapshots (`schema::v1::DocumentDto`)
— today migrations operate on the JSON `Value`, which is lighter and sufficient
until the first real breaking change makes a frozen DTO worthwhile.

For a project whose headline feature is a *template marketplace*, a stable,
migratable format is non-negotiable and must exist before templates proliferate.

### C5 — The animation model cannot represent Lottie/AE  ✅ RESOLVED

> **Resolved.** Implemented per-keyframe temporal handles (`in`/`out` + `Interp`)
> in `cutlass-anim`, spatial motion-path handles (`to`/`ti`, sampled via a
> generic De Casteljau `cubic_bezier`), and animatable geometry
> `cutlass_shapes::Shape::Path { path: Track<BezierPath> }` with vertex-wise
> morphing. `cutlass-lottie` imports `Welcome.json` as the conformance bar
> (60fps, morphing 32-vertex path over 4 keyframes, tangents verified). The one
> deferred sub-item is **per-dimension** temporal easing (handles are currently
> broadcast across components); rare in practice and tracked for later.

This is the one the `Welcome.json` Lottie file exposes directly. Three concrete
gaps versus the data Cutlass must import:

1. **Per-keyframe in/out tangents.** `Keyframe { easing }` stores a single curve
   "out of this key." Real temporal easing of the segment A→B uses the **out**
   tangent of A **and the in** tangent of B (Lottie `o` and `i`, AE Bezier
   handles). The current model can't store B's in-tangent, so imported eases are
   wrong.
2. **Spatial bezier (motion paths).** Lottie position keyframes carry spatial
   tangents (`ti`/`to`) so an object follows a **curved path**. `Track<Vec2>`
   does straight componentwise lerp — no curved motion paths.
3. **Animated paths (shape morphing).** `cutlass_shapes::Shape::Path { commands:
   Vec<PathCmd> }` is **static** — `commands` is not a `Track`. Yet animated
   vertex/tangent morphing is the most common Lottie sticker effect, and
   `Welcome.json` does exactly this. So the claim "one engine animates shape
   control points" is **not true in the data model** today.

Also: easing is applied scalar-on-`t` to the whole value, but AE/Lottie allow
**per-dimension** temporal easing.

**Target design**:

```rust
pub struct Keyframe<T> {
    pub time: RationalTime,
    pub value: T,
    pub interp: Interp,           // Hold | Linear | Bezier{ out: P2, in_next: P2 }
    pub spatial: Option<Spatial>, // ti/to tangents for path-following (position)
}
// Per-dimension easing for vector values, and:
pub enum Shape {
    Path { contour: Track<BezierPath> },   // <-- animatable geometry
    Rect { /* animatable */ }, // ...
}
```

This doesn't break "one animation engine" — it *strengthens* it by making the
engine actually able to drive geometry. Do it before shapes/text-animation get
built on the current (insufficient) model. Build a **Lottie import conformance
test** against `Welcome.json` as the acceptance bar.

---

## High-severity issues

### H1 — No concurrency / threading model  ◐ STARTED
> **Foundation laid.** The edit→render ownership boundary now exists and is the
> hard part to get right: the edit thread owns the mutable `Document`/`Editor`;
> render/export threads only ever see an immutable `SceneSnapshot` published
> through a lock-free `SharedScene` (`arc-swap`). `Editor::snapshot()` freezes the
> scene; `ffi::Session` republishes after every dispatch/undo/redo and exposes
> `shared_scene()` for a UI render thread. A real second thread in the CLI harness
> exercises the path (render thread renders many frames per published edit).
>
> *Still open:* a full job/scheduler runtime (background decode/export workers),
> a hard-realtime audio thread, and making `cutlass_ml::Models`'
> `Box<dyn Trait>` objects `Send + Sync` so inference can move off-thread.

A real-time 6-platform editor needs, minimum: a UI thread, a render thread, an
audio thread (hard-realtime), background decode/export workers, and off-thread ML
inference. Today everything is single-threaded by value: `Editor` owns all state;
`cutlass_ml::Models` holds `Box<dyn Trait>` that isn't `Send + Sync` (can't move
to a worker); the FFI `Session` is a plain struct that native UIs *will* call
from multiple threads. **Decide the ownership/threading model now** (e.g.,
document owned by a core thread; commands via channel; render reads an immutable
snapshot via triple-buffer / `arc-swap`; audio never blocks). Retrofitting
threading into 300k single-threaded lines is one of the most expensive refactors
there is.

### H2 — Renderer consumes the editable `Scene`  ✅ RESOLVED
> **Resolved.** `Renderer::render` now takes `&SceneSnapshot` (in
> `cutlass-core::snapshot`), never the live `Scene`. A snapshot owns its scene
> and hands out only shared references, so the render thread physically cannot
> mutate or race the document, and `SceneSnapshot: Send + Sync` is asserted at
> compile time. Tracks stay intact in the snapshot, so one snapshot renders any
> number of frames at any `Time` — snapshot cost is paid per *edit*, not per
> *frame*. `active_layer_ids` and the golden-image path both run off the snapshot,
> so preview/export/tests share one editing-decoupled input.
>
> *Deferred (optimization):* baking a fully *flattened* RenderScene (effects
> resolved, z-order precomputed) and structural sharing (`Arc` per node) so
> snapshots don't deep-clone unchanged subtrees. The immutability/threading
> contract is in place; these are perf refinements behind the same API.

`Renderer::render(&Scene, ...)` couples the GPU path to the live, mutable
document. You want the editor to produce an **immutable, flattened RenderScene**
(tracks sampled at `t`, effects resolved, z-order computed, ids stable) that the
render thread consumes without locking the document. This pairs with H1 and lets
preview/export/golden-tests share one input that is decoupled from editing.

### H3 — Audio is absent from the model  ✅ RESOLVED
> **Resolved.** Audio is first-class in the document:
> `LayerContent::Audio(Box<AudioClip>)` places clips on the timeline with
> sample-accurate `cutlass-time::Time` (the owning node's `range`). `AudioParams`
> carry **gain (dB) and pan automation as `cutlass-anim` `Track`s** — keyframed by
> the same engine as transforms — plus mute and fades (`FadeCurve` =
> linear/equal-power/exponential/logarithmic). The mix math is pure and unit
> tested (`db_to_linear`, equal-power pan, `fade_gain_at`,
> `stereo_gain_at`), so the realtime mixer and the exporter compute identical
> gains ("preview == export" for audio). `Scene::active_audio_at(t)` (exposed on
> `SceneSnapshot`) collects active clips at any depth for the future audio thread.
> Waveform **peaks** are a derived cache in `cutlass-media` (`WaveformPeaks`,
> min/max envelope), deliberately *outside* the document — same discipline as
> keeping decoded frames out of the model.
>
> *Deferred:* per-clip audio on **video** `Media` (detachable A/V link),
> multi-channel beyond mono/stereo, and beat markers — all additive, and the
> dedicated audio *track* lane arrives with H5.

`LayerContent` has `Media | Text | Shape | Group` — **no audio**. For a video
editor audio is half the product: audio clips, gain automation, pan, waveforms,
multi-channel, fades, beat markers. There is no home for any of it. This needs a
first-class place in the document and timeline model (and interacts with C1 time
and H4 speed).

### H4 — No time-remap / speed model  ✅ RESOLVED
> **Resolved.** A clip's mapping from *timeline time → source time* now has a
> home: `cutlass_core::timewarp::TimeWarp`, an optional field on `Media` and
> `AudioClip`. `TimeWarp::Constant(f64)` covers fixed speed / reverse / freeze;
> `TimeWarp::Ramp(Track<f32>)` is a **keyframed speed curve** (the same
> `cutlass-anim` engine as everything else). Source position is the *integral* of
> speed, computed **analytically** (trapezoid area over piecewise-linear segments)
> in exact integer ticks, so it's O(keys) and drift-free. `LayerContent::
> source_time_at(local)` and `speed_at(local)` are the single entry points the
> decoder and mixer call. Verified end-to-end (a 0.5×→2× ramp reads source
> `2.19s` at local `2.5s`, `6.25s` at local `5s` — exactly `0.5·t + 0.15·t²`).
>
> *Deferred:* optical-flow / frame-interpolated slow-mo (a *rendering* technique
> that consumes this mapping — it belongs in `media`/`render`, not the model), and
> bezier-eased speed segments (currently integrated as linear between keys).

Speed adjustment, **speed curves/ramping**, and **optical-flow** slow-mo are
core CapCut features (see the feature research doc), but there is no
time-remapping concept. A clip needs a mapping from *timeline time → source
time* (itself a `Track`). Without it, the entire media/speed pipeline has nowhere
to live. Define it alongside the media-clip model.

### H5 — No track/lane concept; `Group` redundant; paint order unstated ✅
Editors have **tracks/lanes** (video/audio/text) with their own state (mute,
lock, solo, blend). Cutlass had a flat `Vec<Layer>` + arbitrary nesting, mixing
two trees pros keep separate: the **timeline** (tracks of clips in time) and the
**composition** (scene graph of visual layers). Every `Layer` could have
`children`, so `Group` carried no meaning, and "index 0 is backmost" was an
**unwritten invariant**.

**Resolved.** The split is now explicit:

- **`cutlass-core::track`** defines `TrackKind` (Video / Audio / Subtitle /
  Overlay), `BlendMode`, and `TrackMeta { kind, enabled, locked, solo, blend }`
  with `is_live(any_solo)` (soloing any lane silences the rest; `locked` is
  edit-only and never affects playback).
- A **track is a top-level node** — `LayerContent::Track(Box<TrackMeta>)` — whose
  children are the clips on that lane. This reuses the whole arena/command/undo
  spine: adding a lane is `AddLayer`, reordering is `Reparent`, and lane state is
  the new invertible **`Command::SetTrackMeta`**.
- **`Group` now has one meaning**: a composition/precomp container *inside* a
  clip, never a lane.
- **Paint order is typed and documented**: `roots` is the compositing order
  (index 0 backmost → last frontmost). `Scene::compositing_clips_at(t)` is the
  single renderer entry point — it expands live, non-audio lanes into their
  active visual children (and includes bare top-level clips for back-compat),
  honoring enable/solo. `active_audio_at(t)` is likewise track-aware. The
  renderer consumes `compositing_clip_ids` off the immutable snapshot.

Validated by `compositing_respects_track_order_enable_and_solo` and
`set_track_meta_is_invertible_and_typed`, and demonstrated by the CLI
`tracks_demo` (compositing order, hide+undo, solo). Deferred: per-clip media
audio (a video lane's clip audio), and structurally *forbidding* clips outside a
lane — today a bare top-level clip is tolerated on an implicit lane.

### H6 — Change notifications are coarse (whole-document) ✅
`Session::document_json()` **re-serialized the entire document** for the UI, and
`Event::DocumentChanged` carried no information about *what* changed — every edit
was O(document) to push, with no path to incremental updates.

**Resolved.** Edits now describe their footprint:

- **`cutlass-core::change::ChangeSet`** = `{ layers: Vec<SceneId>, time:
  Option<TimeRange>, structural: bool }`: the dirty layer ids, the *union* of
  affected timeline spans (new `TimeRange::union` hull), and whether the tree
  shape changed.
- **`Command::apply_tracked`** returns `(inverse, ChangeSet)`. The footprint is
  computed against the **pre-apply** scene (so removes/timing see the old state)
  and a `Batch` totals its children. `Editor::apply` returns the `ChangeSet`;
  `undo`/`redo` return `Option<ChangeSet>` so reversal is reported too.
- **FFI**: `Event::Changed { change }` carries it; `Session::layer_json(id)`
  reads a *single* subtree for incremental refresh. `document_json` is now the
  one-time load path, not a per-keystroke cost. Cost is O(change), not
  O(document).

The renderer consumes `time` to invalidate only the dirty frame span; the UI
rebuilds its layer tree only when `structural`. Validated by
`add_reports_structural_change_with_subtree_and_span`,
`set_timing_dirties_union_of_old_and_new_span`,
`batch_change_set_merges_children_and_undo_reports_change`, the FFI tests
(`dispatch_emits_granular_change_and_subtree_is_readable`), and the CLI agent
demo (which prints each edit's footprint). Deferred: a per-frame *dirty-rect*
within a layer (sub-layer invalidation) — today invalidation is per layer +
time span.

### H7 — No color management ✅
`color` was sold as a core concern ("float color grading", "HDR-ready"), but
there was no color module; colors were bare `Vec4` with a "linear" comment and
nothing enforcing it.

**Resolved.** A new **`cutlass-color`** leaf crate makes the decisions explicit:

- **One working space, written down**: linear light, Rec.709 primaries, D65
  (`WORKING_PRIMARIES`). *All* compositing/effects math happens here; encode and
  decode happen **only at the I/O boundary**.
- **Two orthogonal axes** (this is what keeps it small): `Primaries` (Bt709 /
  Bt2020 / DisplayP3, with linear RGB↔XYZ matrices and `gamut_convert`) and
  `TransferFunction` (Linear / sRGB / Gamma / Bt709 / **PQ** / **HLG**), each an
  exact inverse pair (`decode`/`encode`). A `ColorSpace` is one of each, with
  presets `SRGB`, `REC709`, `DISPLAY_P3`, `REC2020_PQ`, `REC2020_HLG`.
- **A typed working color** `LinearRgba` with the boundary built in
  (`from_srgb8`/`from_srgb`/`to_srgb`, and `ColorSpace::decode/encode`), so
  "linear" is enforced by a type rather than a comment.
- **Recorded on the document**: `cutlass-core::ColorConfig { working_primaries,
  output }` (serde-defaulted to SDR sRGB) with `decode_input`/`encode_output`/
  `is_hdr`. HDR (Rec.2020 + PQ/HLG) is therefore a real, serialized capability,
  and these CPU functions are the reference the future WGSL shaders mirror
  ("preview == export" for color).

Validated by ~15 tests (transfer round-trips + sRGB/PQ anchor values, every gamut
mapping (1,1,1)→D65 white, out-of-gamut detection, a full sRGB→working→Rec.2020/PQ
pipeline round-trip, and the `ColorConfig` defaults/HDR flag), and demonstrated by
the CLI `color_demo` (one picked color delivered to both SDR and HDR). Deferred:
chromatic adaptation for non-D65 spaces (DCI-P3 theatrical), an OOG gamut-mapping
operator, and migrating existing `Vec4` fill fields to `LinearRgba` at their call
sites (the boundary + contract exist now; the field-by-field move is mechanical).

---

## Medium-severity issues

- **M1 — `SceneId(Uuid)` as hot-path key.** 16-byte random ids, walked every
  frame and compared every command, bloat the document and hurt cache behavior.
  Use a generational arena key (`u32/u64`) in memory (C2) and keep the UUID only
  as a *persistent* id for save/template identity.
- **M2 — Determinism vs reality.** Bit-exact cross-platform rendering is not
  achievable with `f32` + transcendentals (`sin/cos` in rotation, SIMD/FMA differ
  across the 6 targets). The doc promises "pixel-identical." Reframe to
  **tolerance-based** golden diffing, and define explicitly where determinism is
  guaranteed (timeline math: exact; pixels: within tolerance).
- **M3 — `core → media` layering leak.** `LayerContent::Media` stores
  `cached_frame: FrameHandle`, so the *document model* depends on a *decoder/GPU
  runtime handle*. Caches belong in a render-side store keyed by layer id, not on
  the serialized node. Removing this also drops a crate dependency.
- **M4 — Error strategy.** Mix of typed (`thiserror`) and stringly-typed
  (`Decode(String)`, `Skill(String)`) errors; `document_json()` uses
  `unwrap_or_default()`, silently returning `""` to the UI on failure. Define one
  error convention and stop swallowing.
- **M5 — No transactions/batching.** "Cut silences" currently applies
  `RemoveLayer` + N×`AddLayer` as **N+1 undo steps**; it should be one atomic
  transaction. This is a foundational command-model feature (pairs with C3).
- **M6 — Project/package I/O unfactored.** Loading a `.cutlass` pack (manifest,
  fonts, media resolution, relative paths) has no home; it will accrete into
  `core`, which should stay filesystem-free. Add a `cutlass-project` (or
  `-io`) crate.
- **M7 — Precision impedance.** `Time` is `f64` but transforms are `f32`
  (`glam`). Decide document-space precision (often f64) vs render-space (f32)
  deliberately rather than by accident.

---

## Foundational gaps (process & safety, not just code)

- **G1 — Zero tests.** No unit tests, no property tests. The math
  (time/easing/interpolation/rational) is *ideal* for `proptest` (e.g.,
  round-trip frame↔time, monotonicity, clamp behavior). Land tests with the
  refactors, not after.
- **G2 — No CI, no golden harness.** The golden-image strategy is described but
  doesn't exist. Stand up CI (build + clippy + test + headless golden diff) early
  so consistency can't regress silently.
- **G3 — Security of untrusted input.** A template marketplace + downloadable
  models = untrusted input. Today: template paths are relative strings (path
  traversal: `../../...`), no resource limits on deserialization (a template can
  declare absurd layer/keyframe counts → OOM), and effect "shaders by id" could
  become shader injection if templates carry WGSL. Define a project-pack sandbox,
  deserialization limits, and a shader allow-list before shipping templates.
- **G4 — Identity.** Workspace is `cutlass-playground2`; crates are `cutlass-*`;
  repo is `github.com/cutlass/cutlass`. Settle the real name/repo before it's
  load-bearing in CI, bindings, and package ids.
- **G5 — No benchmarks.** You're committing to hot paths (sampling tracks,
  walking the scene, building render scenes). Add `criterion` benches for them so
  regressions are visible as the codebase grows.

---

## What's genuinely good (keep)

- The **engine-core / native-UI** split and the **command bus** are the right
  spine; most fixes above are *inside* the core and don't disturb this.
- **FOSS-first + agent-over-commands** is coherent and a real differentiator;
  routing AI through the same commands is exactly right.
- The **crate boundaries** are mostly well chosen (fix M3's leak; add audio,
  project-io, and a render-snapshot type).
- **Schema versioning instinct** is right (C4 is about making it *real*).
- **"Attack text first" / golden-image** instincts are right (M2 just makes the
  determinism claim honest).

---

## Recommended remediation order

Sequenced by *dependency* (each step unblocks the next) and *blast radius*
(do high-blast-radius primitives before code piles on them):

1. **C1 `RationalTime`** — replace `Time` everywhere. Foundational to all timing.
2. **C2 arena scene graph + M1 keys** — id-based slotmap; UUID becomes persistence-only.
3. **C3 + M5 undo/transactions** — invertible commands *or* persistent doc, with coalescing + atomic batches.
4. **C4 schema/DTO split + migrations** — decouple wire format; add Lottie import path.
5. **C5 animation redesign** — tangents, spatial paths, animatable geometry; conformance-test against `Welcome.json`.
6. **H2 render snapshot + H1 threading** — immutable RenderScene; pick the thread/ownership model.
7. **H3 audio + H4 time-remap + H5 tracks** — complete the timeline/composition model.
8. **H7 color + M2 determinism** — define color space + tolerance-based golden CI.
9. **G1/G2/G5 tests+CI+benches** — land continuously alongside 1–8, not after.
10. **H6, M3, M4, M6, G3, G4** — clean-ups and hardening, scheduled into the above.

Steps **1–5 are the "before we write more" set.** They're the primitives the
other 295k lines will assume.
