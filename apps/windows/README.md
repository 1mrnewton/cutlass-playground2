# Windows shell

WinUI 3 app (C#/C++) over the shared core.

- **Binding:** `cutlass-ffi` built as a `cdylib`; `cbindgen` emits the C header
  consumed via P/Invoke (C#) or directly (C++).
- **Surface:** an HWND / DXGI swapchain handed to `cutlass-render`
  (wgpu D3D12 backend).
- **Codecs:** Media Foundation / D3D11VA via the `cutlass-media` Windows
  backend; FFmpeg permissible here if desired.

## Planned layout

```
windows/
├─ Cutlass.sln
├─ Cutlass.App/        # WinUI 3 UI
└─ Cutlass.Interop/    # generated C header + P/Invoke wrappers
```
