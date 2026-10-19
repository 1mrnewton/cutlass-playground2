# Android shell

Jetpack Compose app over the shared core.

- **Binding:** `cutlass-ffi` built for Android ABIs (arm64-v8a, x86_64) via the
  NDK; `uniffi` generates the Kotlin bindings.
- **Surface:** a `SurfaceView`/`ANativeWindow` handed to `cutlass-render`
  (wgpu Vulkan/GLES backend).
- **Codecs:** `MediaCodec` through the NDK via the `cutlass-media` Android
  backend. Mind memory ceilings and thermal throttling — design for phones.

## Planned layout

```
android/
├─ app/                 # Compose UI: Timeline, Inspector, Preview
├─ rust/                # cargo-ndk build of cutlass-ffi
└─ app/src/main/java/.../generated/   # UniFFI Kotlin bindings
```
