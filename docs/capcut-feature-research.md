# CapCut Editing Feature Research

A catalog of CapCut's **creative/editing** features (the things that actually
manipulate pixels, audio, text, and motion), compiled to scope the Cutlass
build. Deliberately **excludes** account/login/auth, billing, cloud storage,
real-time collaboration, publishing/social integrations, and team admin — those
were explicitly out of scope.

> Researched June 2026 from CapCut's own tool pages, help docs, and third-party
> reviews. Feature availability differs by platform (Desktop > Web > Mobile);
> the desktop app has the deepest feature set. Sources listed at the end.

Legend for the **Cutlass owner** column (which engine crate would implement it):
`anim` · `text` · `shapes` · `effects` · `render` · `media` · `core` · `ml`
(a *new* crate for the AI/ML features) · `app` (native UI surface).

---

## 1. Core timeline & clip editing

| Feature | Notes | Cutlass owner |
|---|---|---|
| Multi-track timeline | Unlimited video/audio tracks; separate video, audio, text, sticker layers | `core` |
| Trim / split / cut / merge | Core clip operations on the timeline | `core` |
| Crop | Reframe a clip/image to a target frame | `core` + `render` |
| Rotate / flip | Turn or mirror H/V | `core` |
| Freeze frame | Snapshot a still from a moment | `core` + `media` |
| Reverse | Play a clip backward | `media` |
| Replace clip | Swap source while keeping edits (template slots) | `core` |
| Compound / nested clips | Group layers into one timeline element (precomp) | `core` |
| Multi-timeline | Up to ~50 timelines per project (desktop) | `core` |
| Multi-camera editing | Sync & switch 4/9 angles (desktop) | `core` + `media` |
| Markers | Timeline markers, incl. beat markers (see Audio) | `core` |

## 2. Transform, layout & compositing

| Feature | Notes | Cutlass owner |
|---|---|---|
| Transform | Scale, rotate, position of any layer | `core` (`Transform2D`) |
| Opacity | Per-layer alpha | `core` |
| Blend modes | Layer interaction modes (multiply, screen, etc.) | `render` |
| Masks | Linear, circular/mirror, rectangle, **text**, and freehand **pen** masks; feather/invert | `shapes` + `render` |
| Adjustment layer | Apply color/effects to all layers beneath | `core` + `effects` |
| Aspect ratio / canvas | Project ratio, background fill/blur | `core` + `render` |

## 3. Keyframe animation & motion

| Feature | Notes | Cutlass owner |
|---|---|---|
| Keyframes on any property | Position, scale, rotation, opacity, color, effect params, audio volume | `anim` |
| Keyframe **graph editor** | Bézier easing curves; ease-in/out/hold/bounce presets (desktop) | `anim` |
| Motion tracking | Lock a layer (text/sticker) to a moving subject | `effects`/`ml` + `core` |
| Camera/object tracking | Analyze & follow movement in the frame | `effects`/`ml` |
| AI movement | Simulate pan/zoom/shake camera moves | `anim` + `ml` |

## 4. Speed & time

| Feature | Notes | Cutlass owner |
|---|---|---|
| Speed adjustment | 0.1x–100x | `media` |
| Speed curves (ramping) | Non-linear speed with a velocity curve | `anim` + `media` |
| **Optical flow** | Frame interpolation for smooth slow-mo (desktop); vs. simple frame-blending | `media`/`ml` |
| Custom frame rate | 30/50/60 fps targets | `media` |

## 5. Text & captions

| Feature | Notes | Cutlass owner |
|---|---|---|
| Text layers & typography | Fonts, size, color, spacing, stroke, shadow, background | `text` |
| Text **effects** | Preset styled looks (glow, neon, 3D, etc.) | `text` + `effects` |
| Text **templates** | Pre-designed animated title/caption presets | `text` + `core` |
| Text **animations** | In / Out / Loop animation presets, plus keyframes | `text` + `anim` |
| **Auto captions** | AI speech-to-text, word-level sync, punctuation, ~95–97% accuracy | `text` + `ml` |
| Caption styling | Restyle generated captions (font, color, motion, effects) | `text` |
| Auto-translate subtitles | Generate subtitles in a different language than spoken | `text` + `ml` |
| Speaker detection | Differentiate speakers (region/desktop) | `ml` |
| SRT import/export | Subtitle file in/out (desktop/web) | `core` |

## 6. Templates

| Feature | Notes | Cutlass owner |
|---|---|---|
| Template library | 500k+ templates (trend/platform/category); ~6k+ "TikTok trending" | `core` + `app` |
| Apply template | Fill slots with your clips/text, auto-timed to the beat | `core` |
| Template creation | Build from scratch with scripts/guided filming, then publish | `core` + `app` |

## 7. Effects (visual)

| Feature | Notes | Cutlass owner |
|---|---|---|
| Video effects library | Glows, blurs, glitch, retro, light leaks, particles, etc. | `effects` |
| Body/face effects | Cosmetic + creative effects bound to detected subjects | `effects` + `ml` |
| **AI style transfer** | Anime / watercolour / 3D-cartoon restyling of footage | `ml` |
| AI portrait enhancer | Skin smoothing + lighting (generative) | `ml` |
| AI video upscaler | Rescue low-res footage | `ml` |
| AI "Scenes" / effects generator | Generate effects from prompts | `ml` |

## 8. Filters & color grading

| Feature | Notes | Cutlass owner |
|---|---|---|
| Filters / presets | One-tap looks, organized by tone | `effects` |
| Basic adjustment | Brightness, contrast, saturation, exposure, temperature, highlights/shadows | `effects` |
| Color **wheels** | Independent shadow/midtone/highlight (lift/gamma/gain) | `effects` |
| **HSL** | Target individual colors by hue/sat/lightness | `effects` |
| Curves | RGB + Luma + Hue/Sat curves | `effects` |
| LUTs | Custom LUT import + preset saving (brand kits) | `effects` |
| Float/high-precision grading | Higher bit-depth pipeline | `render` + `effects` |
| AI color grading | One-tap grade matched to a reference image/video | `ml` |
| Auto color correction | Automatic balance | `effects` + `ml` |

## 9. Stickers, overlays & shapes

| Feature | Notes | Cutlass owner |
|---|---|---|
| Sticker library | Static + animated stickers, GIFs | `shapes`/`render` + `core` |
| Shapes | Basic vector shapes as overlays/mask sources | `shapes` |
| Overlays | Picture-in-picture, frames, borders | `core` + `render` |
| **AI-generated stickers** | Stickers created from a text prompt | `ml` |

## 10. Transitions

| Feature | Notes | Cutlass owner |
|---|---|---|
| Transition library | Hundreds (dissolve, zoom, glitch, 3D, trending styles) | `effects` + `render` |
| Adjustable duration | Per-transition timing | `core` |

## 11. Audio

| Feature | Notes | Cutlass owner |
|---|---|---|
| Music library | Royalty-free tracks | `media` + `app` |
| Sound effects library | Royalty-free SFX | `media` + `app` |
| Volume / fade / loudness normalization | Per-clip + mix-level | `media` |
| **Noise reduction** | One-click AI denoise w/ sensitivity | `media`/`ml` |
| Voice changer | Robot/chipmunk/electronic + character voices | `media` |
| Voice enhancement | Clarity/clean-up (mobile/desktop) | `media`/`ml` |
| **Vocal isolation** | Split vocals from instrumental | `media`/`ml` |
| **Beat detection** | Auto-mark beats for sync | `media` |
| Pitch control | Independent pitch / with speed | `media` |
| Audio split/trim/remix | Timeline audio ops | `core` + `media` |
| Pro audio (desktop) | Compression, de-essing, EQ | `media` |

## 12. AI generation & smart automation

| Feature | Notes | Cutlass owner |
|---|---|---|
| **Text-to-video** | Prompt → clip, via Veo 3.1 / Sora 2; up to ~8s | `ml` |
| **Image-to-video** | Animate a still (first/last frame) into motion | `ml` |
| Keyframe-to-video | Generate motion between key images | `ml` |
| **AI image generation** | Generate/enhance images (Dreamina) | `ml` |
| AI inpainting / object & people removal | Remove/replace regions | `ml` |
| AI text remover | Clean on-screen text | `ml` |
| **Text-to-speech** | 200+ voices, multi-language, speed/pitch | `media` + `ml` |
| Voice cloning | Custom cloned voice (Pro) | `ml` |
| AI avatars / talking head | Lip-synced presenter | `ml` |
| Lip sync | Sync mouth to generated/added voice | `ml` |
| AI script generator | Topic+tone → hook/body/CTA script | `ml` + `app` |
| "Video Studio" / auto-edit | Describe a video → assembled timeline | `ml` + `core` |
| Smart cut / filler-word removal | Auto-trim silences/filler words | `ml` + `core` |
| **Smart/auto reframe** | Auto-crop to target aspect ratios | `ml` + `render` |

## 13. Retouch & beauty

| Feature | Notes | Cutlass owner |
|---|---|---|
| Face retouch | Smooth skin, blemish/dark-circle removal, teeth whiten, brighten | `effects` + `ml` |
| Auto reshape | Reshape eyes/nose/eyebrows/mouth/face | `ml` |
| Makeup | Virtual highlights, lashes, lipstick | `effects` + `ml` |
| Body editor | Slim/widen body, waist, shoulders, arms, legs, hips, height | `ml` |

## 14. Background & cutout

| Feature | Notes | Cutlass owner |
|---|---|---|
| **Chroma key** (green screen) | Sample key color; adjust intensity/shadow | `effects`/`render` |
| Auto cutout / BG removal | AI subject isolation w/o green screen; replace background | `ml` + `render` |

## 15. Stabilization & enhancement

| Feature | Notes | Cutlass owner |
|---|---|---|
| Stabilizer | Reduce handheld shake | `effects`/`ml` |
| Image/video enhancement | Sharpen/denoise/upscale | `ml` |

## 16. Export / output (editing-adjacent)

| Feature | Notes | Cutlass owner |
|---|---|---|
| Resolution | up to 1080p free / 4K (Pro) | `media` + `render` |
| Frame rate / bitrate | Configurable | `media` |
| Format | MP4/MOV; GIF; audio-only | `media` |

---

## Implications for Cutlass

> **Positioning:** Cutlass is **free and open source** and **AI-native**. Unlike
> CapCut, the whole deterministic editor works with no account/cloud, and AI
> features prefer **local/open models** with hosted models strictly opt-in. The
> AI is delivered through an **agent on the command bus** (`cutlass-agent`): the
> user states intent ("cut all silences", "add captions") and the agent emits
> ordinary, undoable `Command`s the engine runs — so every CapCut "AI tool"
> below becomes an agent **skill** backed by a `cutlass-ml` provider.

**Tiering by build difficulty for the engine we've scaffolded:**

- **Tier 1 — deterministic engine work (already mapped to crates).**
  Timeline editing, transform/compositing, masks, keyframe animation + graph
  editor, speed curves, text/typography + text animation, shapes, transitions,
  filters & full color grading, blend modes, chroma key, stickers/overlays.
  These are exactly what `core`/`anim`/`text`/`shapes`/`effects`/`render`/`media`
  are designed for. **This is the achievable, differentiated core of a CapCut
  alternative.**

- **Tier 2 — heavy but classic CV/DSP (no large generative models).**
  Optical-flow interpolation, motion/camera tracking, noise reduction, vocal
  isolation, beat detection, stabilization, auto reframe, auto captions (ASR).
  Buildable with established libraries/models; some are on-device feasible.

- **Tier 3 — large generative AI (`cutlass-ml` `Generator` provider, opt-in).**
  Text/image-to-video (Veo/Sora-class), AI image gen, style transfer, avatars,
  voice cloning, inpainting, upscaling, AI script/scene generation, body
  reshaping. CapCut *buys/hosts* these models (Veo 3.1, Sora 2, Dreamina). As a
  FOSS project we **integrate third-party model APIs** behind the `Generator`
  provider rather than train/bundle them — kept opt-in so a pure-FOSS build can
  omit them entirely. This is a product/cost decision more than an engine one.

**Architectural takeaways (now reflected in the workspace):**

1. ✅ `cutlass-ml` exists with **provider traits** (`SpeechToText`,
   `SilenceDetector`, `TextToSpeech`, `Segmenter`, `FrameEnhancer`, `Generator`)
   — local/open first, hosted opt-in — so AI plugs in without polluting the core.
2. ✅ `cutlass-agent` exists: every CapCut "AI tool" maps to a `Skill` that emits
   `Command`s (e.g. `add_captions`, `cut_silences`), keeping AI edits undoable.
3. The **template system** (`core::Document` + slots) underpins CapCut's biggest
   draw (the template library) — keep it first-class and versioned.
4. **Auto-captions + TTS** are the highest-value AI features for the least cost
   (ASR + TTS are commoditized, run locally); good early wins after the core.
5. Generative video/image is where CapCut leans on ByteDance scale — treat it as
   an opt-in **integration**, not something to reimplement.

---

## Sources

- CapCut official tool/resource/help pages (capcut.com): AI Video, Text-to-Speech,
  Text Tools, Auto Caption Generator, Keyframe Animation, Color Grading, Optical
  Flow, Face Retouching, Audio Editor/Mixer, Noise Removal, "How to Use CapCut".
- TikTok/ByteDance "About CapCut" feature listing (ads.us.tiktok.com).
- Third-party reviews/guides (2026): marcandrews.com, cybernested.com,
  capcutguide.com, freeacademy.ai, aitoolsatlas.ai, flowith.io,
  videowizardtools.com, filmora.wondershare.com, vediting.home.blog,
  photography.tutsplus.com, moviemaker.minitool.com.
