# Typed Windows overlay renderer and fixture port

Source range `05133165..045fbbb6` on
`NightHammer1000/overlay-native-leaves`. The production port is
`7014704b` and the D3D11 DLL device fixture port is `045fbbb6`.

| Requested path | Disposition |
| --- | --- |
| `runtime/orchestrator/src/overlay_native12.cpp` | Removed. The typed Windows implementation in `overlay_native12/native.rs` owns root signature and pipeline creation, frame objects, upload mapping, atlas copies, draw recording and fence operations. The HLSL is in `overlay_native12/overlay.hlsl`. |
| `runtime/orchestrator/src/overlay_d3d12.cpp` | Removed. `overlay_wrapped12/native.rs` resolves the typed D3D11On12CreateDevice export from the retained System32 module, wraps resources and submits through the same owner. The linked entries now use C linkage. |
| `runtime/graphics/src/overlay_renderer.cpp` | Removed. Renderer, host and input logging share the existing `rsf_call_native_log` exception guard in `runtime/orchestrator/src/plugin_callbacks.c`. The retained rule is that a foreign C++ exception stops before it can unwind through Rust. |
| `tests/overlay_dll_api_device.cpp` | Replaced by a C import launcher. The Rust graphics body owns hidden windows, hardware/WARP selection, COM leases, clear and Present, foreign-chain setup and resize. The fixture still calls the imported Runtime table. |
| `tests/overlay_native12.cpp` | Replaced by a C launcher for the Rust WARP scenario. It invokes the same production native D3D12 entry through the passed callback. |

## Ownership

The native D3D12 port keeps the allocator, list and heaps in the existing
slot owner. A partially created slot passes its earlier successful
references back for that owner's normal cleanup. The Rust GPU object owns
the root signature, queue and device and retains the compiled shader bytes
for format-specific pipeline creation. The same borrowed frame drives
mapping, descriptor setup, command close and submission. Fence completion
still governs slot reuse. An executed list with a failed signal stays with
its original owner so retirement can retry the signal without executing the
list again.

The D08 path retains the supplied resource instead of re-reading the
swapchain index. It wraps PRESENT as the entry and exit state, and validates
the separate input HWND before the owner installs input. The legacy route
uses the current chain buffer with RENDER_TARGET as both states. Admission
precedes acquisition. The owner releases its wrapped and view references,
then flushes the same immediate context, with the original backbuffer
lease alive through that sequence.

The C02 render host methods, shared Unicode routing, saved rollback, panel
wire records and D08 GL owner were compared with the base and are unchanged.
The two private table headers still describe the generated callback
records used by Rust and its fixtures. The obsolete C renderer adoption
record and C++ wrappers were removed.

## Source evidence and open checks

- Extracted the previous raw HLSL and compared it with the new file byte
  for byte after newline normalization.
- Compared the pinned Windows SDK projection fields and signatures against
  the prior calls, including root layouts, scissor and vertex offsets,
  atlas row pitch, barriers, wrap states and COM ownership transfers.
- Compared the old and new device fixture steps, hardware/WARP fallback
  and exit 77 only for unavailable devices.
- Reproduced the private overlay bindings twice using bindgen 0.72.1.
  The checked-in table was identical. Pinned Rust 1.94.1 formatting, locked
  offline Cargo metadata and changed-source whitespace checks passed.

The shared native exception guard and the two C entry launchers remain.
Build, test, Clippy and device execution are user-owned. Native Windows
linking, WARP queue pressure and overlay composition on an actual D08
session remain unverified by this source port. No change was made to
`PORT.md`.
