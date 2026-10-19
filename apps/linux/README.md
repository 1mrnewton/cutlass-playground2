# Linux shell

GTK4 app over the shared core (Qt is an alternative if you want to share one
C++ UI with Windows).

- **Binding:** `cutlass-ffi` built as a `cdylib`; `cbindgen` emits the C header.
- **Surface:** a Wayland/X11 surface handed to `cutlass-render`
  (wgpu Vulkan backend).
- **Codecs:** FFmpeg (VAAPI) via the `cutlass-media` Linux backend.

Lowest priority of the six targets, but wgpu + the shared core make it nearly
free; keep it in the golden-image CI matrix to catch driver-level drift.

## Planned layout

```
linux/
├─ meson.build / Cargo  # build wiring
├─ src/                 # GTK4 UI
└─ generated/           # cbindgen C header
```
