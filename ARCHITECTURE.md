# Cutlass — Architecture

A **free and open-source** (MIT OR Apache-2.0), **AI-native** cross-platform
video editor (CapCut alternative) for **Windows, macOS, Linux, Android, iOS, and
iPadOS**, built as a single Rust + wgpu render/animation core with **native UI
per platform** on top.

Two things make Cutlass distinct from the proprietary incumbents:

1. **Open source.** The entire deterministic engine is FOSS and runs with no
  account, no cloud, and no proprietary blobs. AI features prefer local/open
   models; any hosted model is strictly opt-in.
2. **AI-native via the command bus.** An AI agent sits *on top of* the same
  command vocabulary the UI uses. The user expresses intent ("cut all the
   silences", "add captions") and the agent **emits ordinary** `Command`**s that the
   engine executes** — through the same undo stack as manual edits.



## Guiding principles

1. **One engine, many faces.** All visual logic — layout, text, effects,
  animation, compositing, export — lives in the Rust core. Native UIs are
   views/controllers only; they never render canvas content themselves.
2. **Everything is data.** Templates, effects, and animations are serialized
  documents the core interprets. No effect/animation is ever expressed in
   Swift/Kotlin/C++.
3. **Everything is a command.** Every edit — manual *or* AI — is a coarse,
  serializable `Command`. The AI agent is just another command source, which
   makes AI edits auditable, undoable, and safe by construction.
4. **Preview == export.** One render graph drives both, differing only by
  timestep/quality. This keeps templates faithful.
5. **The core owns the truth.** Document state, undo/redo, and the playback
  clock live in Rust; native UI and the agent send commands and observe events.
6. **FOSS-first AI.** No capability is mandatory or locked behind a service; the
  core editor is fully functional with AI disabled.



## Layers

```
Native UI per platform  (apps/)        AI agent  (cutlass-agent)
   SwiftUI · Compose · WinUI · GTK        NL intent → skills → commands
        │                                       │
        │  commands in / events out            │ emits the SAME commands
Binding layer  (cutlass-ffi) ◀────────────────┘
   UniFFI (Swift/Kotlin) + cbindgen (C ABI)
        │
Rust core                        — document · scene · anim · text · shapes ·
                                   effects · render · media · color
        │                                       │
Media I/O  (cutlass-media)              AI/CV/DSP models  (cutlass-ml)
   per-platform, license-aware codecs     local/open first, hosted opt-in
```



## Crates & dependency DAG

The workspace is a clean acyclic graph (leaves at the top):

```
cutlass-time (leaf: exact tick time)     cutlass-color (leaf: color science)
   ▲                                            ▲
cutlass-anim ──────────────┐            cutlass-media          │
   ▲      ▲      ▲          │              ▲       ▲            │
cutlass-text cutlass-shapes cutlass-effects│   cutlass-ml      │
   └──────┴───┬──┴───────────┬─────────────┘       │           │
              │        cutlass-core ◀──────────────┴───────────┘
        cutlass-lottie ▲  ▲     ▲
                        ▲     ▲     ▲
            cutlass-render  cutlass-ffi  cutlass-agent ◀─┘
                        ▲     ▲     ▲
                     cutlass-cli (dev harness)
```


| Crate             | Role                                                                                                          |
| ----------------- | ------------------------------------------------------------------------------------------------------------- |
| `cutlass-time`    | Exact, perf-first time: fixed-base integer ticks (flicks), `Fps`, `TimeRange`                                 |
| `cutlass-color`   | Color science: linear working space, transfer functions (sRGB/Rec.709/PQ/HLG), primaries/gamut matrices, the encode/decode boundary |
| `cutlass-anim`    | One keyframe engine for **every** animatable property: AE/Lottie tangents, spatial motion paths               |
| `cutlass-text`    | Self-owned text model (own shaping/layout/raster — never native text)                                         |
| `cutlass-shapes`  | Animatable vector shapes + **morphing** bezier paths (`Track<BezierPath>`)                                    |
| `cutlass-effects` | Effect graph: DAG of WGSL shader passes with animatable uniforms                                              |
| `cutlass-lottie`  | Import Lottie/Bodymovin animations into the document model                                                    |
| `cutlass-media`   | Decode/encode/demux traits (video + audio) + per-platform backends; waveform peaks cache                       |
| `cutlass-ml`      | Pluggable AI/CV/DSP model providers (STT, TTS, silence detect, segment, enhance, generate) — local/open first |
| `cutlass-core`    | Document/template model + **versioned load/migration**, **arena** scene graph, **tracks/lanes** (timeline) + composition split, **invertible** command/undo + **granular change sets**, immutable render snapshot, first-class audio, time-remap |
| `cutlass-render`  | wgpu render graph + compositor (preview == export), consumes an immutable `SceneSnapshot`                     |
| `cutlass-agent`   | AI agent: plans natural-language intents into engine `Command`s                                               |
| `cutlass-ffi`     | Stable command-in/event-out boundary for native UIs                                                           |
| `cutlass-cli`     | Headless dev harness + (future) golden-image test runner                                                      |




### Foundation primitives (post-review)

- **Time is exact, not float.** `cutlass-time::Time` is a signed integer count of
ticks at a fixed base (`705_600_000`/s, "flicks") that divides every common
video/audio rate, so frame math is lossless and `Time` is `Copy`/`Eq`/`Ord`/
`Hash` with branch-free arithmetic. `Fps` (rational) lives on sequences/clips.
- **Scene graph is an arena.** `Scene` stores nodes in a generational
`slotmap` keyed by a fast internal `NodeId`, with a `SceneId(Uuid)` → `NodeId`
index for O(1) lookup. `SceneId` is the *stable* identity used by commands and
persistence; `NodeId` is the *fast* in-memory handle. Reparent/reorder are O(1)
and cycle-checked.
- **Undo is inverse-based.** `Command::apply` returns the command that reverses
it, so the history stacks hold small deltas (not document clones).
`Command::Batch` makes a multi-step edit one atomic undo step.
- **Persisted format is decoupled from runtime.** `Scene` serializes via a
`SceneDto` (friendly nested-tree JSON), so the arena can evolve without
breaking saved documents — the first step of the schema/runtime split.
- **Animation matches After Effects / Lottie.** Each `Keyframe` carries its own
temporal handles (`in`/`out`) and an `Interp` mode; a segment's easing is the
cubic Bézier formed by the start key's `out` and the end key's `in`. Position
keyframes additionally carry **spatial** handles (`to`/`ti`) for curved motion
paths, and geometry itself animates via `Track<BezierPath>` (path morphing).
This is what lets `cutlass-lottie` round-trip real motion design rather than
approximate it.
- **Render reads an immutable snapshot, never the live scene.** The edit thread
owns the mutable `Document`; the renderer takes a `SceneSnapshot` (owns its
`Scene`, hands out only `&` access, `Send + Sync`). The edit thread `publish`es a
snapshot per *edit* through a lock-free `SharedScene` (`arc-swap`); render/export
threads `latest()` per *frame* without locking. Tracks stay intact in the
snapshot, so one snapshot serves any number of frames at any `Time` — snapshot
cost is amortized over the whole preview between edits. This is the spine of the
threading model: preview, export, and golden-image tests all consume the same
editing-decoupled input.
- **Audio is first-class, not an afterthought.** `LayerContent::Audio` puts
clips on the same timeline with sample-accurate `Time`; `AudioParams` keyframe
gain (dB) and pan through the *same* `cutlass-anim` tracks as transforms, plus
mute and fades (`FadeCurve`). The mix math is pure and tested
(`stereo_gain_at`, equal-power pan, dB→linear), so the mixer and exporter agree
to the sample. `Scene::active_audio_at` feeds the future audio thread from a
snapshot; waveform peaks are a derived cache in `cutlass-media`, kept out of the
document like decoded frames.
- **Speed lives in one primitive.** A clip's `TimeWarp` (optional on `Media`
and `AudioClip`) maps timeline time → source time: `Constant` for fixed
speed/reverse/freeze, or a keyframed `Ramp` (a `cutlass-anim` `Track<f32>` of
speed multipliers). Source position is the *integral* of speed, computed
analytically in exact ticks (no drift). The decoder/mixer ask one question —
`LayerContent::source_time_at(local)` — and get the exact source frame to read.
- **Timeline and composition are different trees.** The *timeline* is an ordered
stack of **tracks** (`LayerContent::Track` + `TrackMeta`: kind, enable, lock,
solo, blend); each lane's children are its clips. The *composition* is the scene
graph *inside* a clip, where `Group` is the precomp container. Compositing order
is the typed `roots` order (back→front), surfaced by
`Scene::compositing_clips_at` and consumed by the renderer; mute/solo are
honored there and in `active_audio_at`. Lane state is edited by the invertible
`Command::SetTrackMeta`, so mute/solo/blend ride the same undo stack as every
other edit.
- **Edits report what they touched.** Applying a command returns a `ChangeSet`
(dirty layer ids + the union of dirty timeline spans + a `structural` flag).
`Editor::apply/undo/redo` surface it; the FFI re-emits it as `Event::Changed` and
lets the UI re-read a single subtree via `Session::layer_json(id)`. The cost of
notifying the UI/renderer is proportional to the *edit*, not the document — no
whole-document re-serialize per keystroke.
- **One working space; encode/decode only at the boundary.** `cutlass-color`
fixes the working space as **linear light, Rec.709 primaries (D65)** — the only
space effects/blends do math in. The two color axes are split: `Primaries`
(gamut, with D65 XYZ/gamut matrices) and `TransferFunction` (sRGB / Rec.709 / PQ
/ HLG, exact inverse pairs). A `ColorSpace` is one of each; inputs decode to the
working space and outputs encode from it (`ColorSpace::decode/encode`). The
document carries a `ColorConfig` (working primaries + output space), so SDR
(sRGB) and HDR (Rec.2020+PQ/HLG) are explicit, serialized, and the GPU shaders
have one CPU reference to mirror — "preview == export" for color too.
- **The wire format is not the runtime layout.** Saving writes the current
`SCHEMA_VERSION`; loading runs a forward-migration pipeline (`cutlass-core::
migrate`): parse → `serde_json::Value` → apply ordered `v(n)→v(n+1)` steps →
deserialize into today's types. A typed `DocumentError` rejects future-version
files instead of mis-parsing them, a missing version loads as v1, and the policy
is *additive within a version* (no `deny_unknown_fields`). Each future breaking
change appends one migration step and bumps the version, so every saved project
and published template keeps opening.



## The AI agent (command-bus design)

The agent never mutates the document. It reads the project + media (via
`cutlass-ml`) and returns a `Vec<Command>` that the engine applies through the
normal `Editor`. Flow:

```
user intent ──▶ Agent ──▶ Skill.plan(ctx) ──▶ Vec<Command> ──▶ Editor.apply ──▶ engine
  "cut all     (selects   (analyzes via      (ordinary,         (same undo
   silences"    a skill)   cutlass-ml)        serializable)      stack as UI)
```

Why route AI through commands instead of letting it edit state directly:

- **Auditable** — each AI action is an explicit, inspectable command.
- **Undoable** — AI edits use the same undo/redo as manual edits.
- **Safe** — the agent can only do what the command vocabulary permits.
- **Uniform** — the agent and the native UI are interchangeable command sources.

A `Skill` is the unit of capability (`context → commands`). An LLM planner
(added later, behind a provider trait) only chooses *which* skills to run and
with what arguments — it has no access to engine internals. Built-in skills
today (placeholders): `add_captions` (STT → text layers) and `cut_silences`
(silence detection → split/keep segments). The `Models` registry in `cutlass-ml`
reflects whichever providers a build enables; a pure-FOSS build uses local/open
models (whisper.cpp, piper, ONNX), and generative features stay opt-in.

## The five concerns this design targets

- **Templates** — a template is a versioned `Document` with fillable slots
(`cutlass-core`). Portable JSON; renders identically everywhere.
- **Text** — the hardest cross-platform problem; we own the entire stack and
bundle fonts so titles are pixel-identical (`cutlass-text`). Attack first.
- **Effects** — WGSL shader DAG compiled by `naga` to every backend
(`cutlass-effects` + `cutlass-render`).
- **Shapes** — GPU path geometry in the same scene graph (`cutlass-shapes`).
- **Animations** — a single timeline evaluator drives transforms, text, shapes,
and shader uniforms alike (`cutlass-anim`).



## Cross-platform consistency strategy (from day 1)

- **Golden-image CI**: render fixed templates headlessly on every platform and
diff against reference PNGs (`templates/golden/`). Drift can't be merged.
- **Deterministic render mode**: fixed timestep + fixed RNG seeds so exports
are reproducible (`RenderMode::Export`).
- **Schema versioning**: `SCHEMA_VERSION` from commit #1, with migrations.



## Codecs & licensing (the minefield)

Decode/encode sit behind `cutlass-media` traits with per-platform backends:


| Platform               | Backend                       |
| ---------------------- | ----------------------------- |
| Apple (mac/iOS/iPadOS) | VideoToolbox (App Store-safe) |
| Android                | MediaCodec (NDK)              |
| Windows                | Media Foundation              |
| Linux                  | FFmpeg (VAAPI)                |


⚠️ FFmpeg's GPL + patent exposure makes it unsuitable for the App Store, which
is exactly why codecs are abstracted — Apple/Android ship native encoders.

## Intended tech stack (added per subsystem, not yet wired)

Kept out of the build until each subsystem is implemented, so the workspace
stays fast and green:


| Subsystem              | Crates to add                                                                    |
| ---------------------- | -------------------------------------------------------------------------------- |
| Rendering              | `wgpu`, `naga`, `raw-window-handle`                                              |
| Text                   | `rustybuzz`, `cosmic-text`, `swash`, `fontdb`                                    |
| Shapes                 | `vello` (or `lyon`), `kurbo`                                                     |
| Audio decode           | `symphonia`                                                                      |
| AI/ML (local, FOSS)    | `whisper-rs` (STT), `ort`/ONNX Runtime (CV models), `piper` (TTS)                |
| AI agent / LLM planner | local LLM via `llama.cpp` bindings, or opt-in hosted API behind a provider trait |
| AI generative (opt-in) | external model APIs (text/image-to-video) — never bundled                        |
| Bindings               | `uniffi`, `cbindgen`                                                             |
| Windowing (dev)        | `winit`                                                                          |




## Repository layout

```
cutlass/
├─ Cargo.toml            # virtual workspace + pinned shared deps
├─ ARCHITECTURE.md
├─ LICENSE-MIT / LICENSE-APACHE   # dual FOSS license
├─ crates/               # the Rust engine + agent + ml (see table above)
├─ shaders/              # WGSL effect library (e.g. blur.wgsl)
├─ templates/            # sample documents + golden references
├─ docs/                 # research & design notes (e.g. capcut-feature-research.md)
└─ apps/                 # native UI shells (apple / android / windows / linux)
```



## Phased roadmap

1. **Render spike** — wgpu compositor draws a static scene into a window
  (mac + one other); prove the surface handoff.
2. **Document + animation** — schema + keyframed transforms. ✅ (data model)
3. **Text engine** — own shaping/layout/raster; golden-test early (highest risk).
4. **Effects + shapes** — shader DAG + vector renderer on the shared anim model.
5. **Media I/O** — decode → timeline → deterministic export with native encoders.
6. **FFI + first native UI** — UniFFI → SwiftUI; ship the Apple trio first.
7. **Remaining shells** — Android (Compose), then Windows/Linux.
8. **AI agent v1** — wire real local models into `cutlass-ml` (whisper STT,
  silence DSP) behind the existing `add_captions` / `cut_silences` skills, then
   add an LLM planner that maps free-form intent to skill calls.
9. **Template ecosystem** — authoring, packaging, marketplace format.

