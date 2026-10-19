# Apple shell (macOS · iOS · iPadOS)

One SwiftUI app targeting all three Apple platforms.

- **Binding:** `cutlass-ffi` compiled as a static lib + `uniffi` generates the
  Swift package. The `Session` object is exposed directly to Swift.
- **Surface:** host a `CAMetalLayer` in a `UIViewRepresentable`/`NSViewRepresentable`
  and hand its `raw-window-handle` to `cutlass-render` (wgpu Metal backend).
- **Codecs:** VideoToolbox (App Store-safe hardware encode/decode) via the
  `cutlass-media` Apple backend. No FFmpeg on these targets.

## Planned layout

```
apple/
├─ Cutlass.xcodeproj
├─ Shared/        # SwiftUI views: Timeline, Inspector, Preview
├─ macOS/  iOS/   # per-target entry points & capabilities
└─ Generated/     # UniFFI Swift bindings (build step)
```
