# Native app shells

Per the chosen architecture, the UI is **native per platform** while 100% of the
rendering/animation/effects/text logic lives in the shared Rust core and is
reached through `cutlass-ffi`.

Each shell is responsible only for:

1. Window/lifecycle + a view that hosts the engine's GPU surface.
2. Editor chrome: timeline, inspector, panels, file pickers.
3. Forwarding user intent as `cutlass_core::Command`s and rendering state from
   document snapshots + `Event`s.
4. Native export hooks (platform encoders).

| Folder     | Targets                | UI            | Binding            | Surface             |
|------------|------------------------|---------------|--------------------|---------------------|
| `apple/`   | macOS, iOS, iPadOS     | SwiftUI/UIKit | UniFFI (Swift)     | `CAMetalLayer`      |
| `android/` | Android                | Jetpack Compose | UniFFI (Kotlin)  | `SurfaceView`       |
| `windows/` | Windows                | WinUI 3       | cbindgen (C ABI)   | HWND / DXGI         |
| `linux/`   | Linux                  | GTK4 (or Qt)  | cbindgen (C ABI)   | Wayland / X11       |

Build the Apple trio first: one SwiftUI codebase ships three of the six targets.
