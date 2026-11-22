# V05: FidelityFX Vulkan bridges without first-party C++

This note records the V05 close-out on `rust/orchestrator-anomaly`. It follows
[rust-vulkan-ffx-production-leaves.md](rust-vulkan-ffx-production-leaves.md) and
[rust-vulkan-ffx-residual-port.md](rust-vulkan-ffx-residual-port.md).

## What moved

| Former file | Now |
| --- | --- |
| `runtime/vulkan/fsr4/bridge.cpp`, `bridge_platform.h` | `runtime/vulkan/fsr4/native.rs`, a child of `sdk.rs` in the `rsf-fsr4-compat` archive. It holds the capsule (module, DXGI and D3D12 objects, SDK context, INT8 lease), builds the owner's 24-slot platform table and defines every export: `rsf_vk_fsr4_*` and `RsfFsr4Bridge*`. Both sides of the private table are Rust, so the header is gone. |
| `runtime/vulkan/fsr4-fg/bridge.cpp`, `bridge_platform.h` | `runtime/vulkan/fsr4-fg/native.rs`, a child of `sdk.rs` in the orchestrator crate. The capsule keeps the create descriptors the SDK retains (override version, DX12 backend, generation version, swapchain version, HWND swapchain description and the DXGI description) in one zeroed, never-moved allocation. It builds the 36-slot table and exports `RsfFsr4FgProbe` and `RsfFsr4FgCreate`. |
| `runtime/orchestrator/src/native_fg_fsr4_vulkan.cpp` | `vulkan_fsr4_fg.rs` builds the 43-slot production table once (`LazyLock`) and exports `rsf_fsr4gvk_probe` and `rsf_fsr4gvk_create`. The importer entries and the V02 graphics table are bound by address exactly as V06 binds them in `vulkan_xess_fg.rs`; the graphics table is copied bitwise, as the native table embedded it. |
| `runtime/orchestrator/src/native_sr_fsr4_vulkan.cpp` | C11 leaf `native_sr_fsr4_vulkan.c`. It keeps containment of C++ exceptions from the host procedure resolvers, the host split callback and the bridge calls that reach the SDK, as SEH filters on code 0xE06D7363 (the pattern of `plugin_callbacks.c`). The interop pointer is a Rust address, so the table is completed once with `InitOnceExecuteOnce` instead of a constant initializer. |
| `runtime/orchestrator/src/native_fg_fsr4_vulkan_platform.h` | C declarations only. The eight bridge slots are spelled with the canonical Windows x64 types that bindgen previously derived through `decltype`, so `tests/vulkan-fg/src/fsr4_state_abi.rs` stays byte-identical. A scratch MSVC C++17 unit asserted `std::is_same` between every slot and the former `decltype(&RsfFsr4Fg*)` type and the 344-byte layout. The header still includes V03's `native_fg_dx12_vulkan_platform.h` for the importer and graphics records. |
| `runtime/vulkan/fsr3/wsi-dispatch.h` | C declarations plus a C11 implementation section behind `RSF_FFX_WSI_IMPLEMENTATION`: an SRWLOCK replaces `std::mutex`, and `rsf_ffx_wsi_create`, `_destroy`, `_present`, `_acquire` and `_images` replace the `rsf_ffx_wsi::` namespace. `swapchain-lifetime.json` and `tests/ffx_vulkan_wsi_dispatch.cpp` call the new names. The registration rules and return codes are unchanged. |

`ffxApiGetResourceDX12` and `ffxApiGetSurfaceFormatDX12` are C++-only inline
helpers in `ffx_api_dx12.h`. Both bridges now use the orchestrator's existing
port, `d3d12_sr/ffx_resource.rs`; the compatibility archive includes that file
and its `fsr_abi.rs` by path rather than adding a third copy.

## SDK values

Bindgen skips the function-like descriptor macros. A scratch MSVC program
compiled against the pinned FidelityFX 2.4.0 headers (`7539ae8b`) printed the
values the Rust constants use: backend DX12 `0x2`, `FFX_UPSCALER_VERSION`
`0x1001002`, `FFX_FRAMEGENERATION_VERSION` `0x1000002`, swapchain DX12 version
`0x1000000`, swapchain version and HWND descriptor types `0x3000b` and
`0x30006`, the generation, configure, PrepareV2, version and WaitForPresents IDs
`0x20001`, `0x20002`, `0x2000c`, `0x2000e`, `0x30007`, the upscaler IDs
`0x10000`, `0x10001`, `0x10003`, `0x1000b`, and surface BGRA8 `14`. It also
printed the layouts asserted in `fsr4-fg/native.rs`: backend and swapchain
version descriptors 24 bytes, HWND descriptor 64 bytes with `hwnd` at 24, `desc`
at 32 and `gameQueue` at 56, and `numGeneratedFrames` at 264. The DXGI, Win32
and Vulkan values the deleted SR leaf asserted are compile-time checks in
`fsr4/native.rs`.

## Standalone bridge

`runtime/vulkan/fsr4/build.ps1` no longer compiles C++. `g++` still drives the
link for its static runtime, and `-Wl,--undefined` pulls every export listed in
`integrations/vulkanstory/native/fsr4.def` out of the Rust archive. The `.def`
now also lists the plain `rsf_vk_fsr4_*` and `RsfFsr4Bridge*` names that the
former `__declspec(dllexport)` attributes exported.

## Still C++

The five SDK correction manifests and `sdk-fixes/swapchain-trace.h` edit or are
compiled into the pinned FidelityFX Vulkan SDK's own C++ sources by
`build-sdk.ps1`. They change code inside `amd_fidelityfx_vk.dll`, so they have no
first-party translation unit to port. The trace header's RAII scopes and
`call()` wrapper close a row on every return path of the instrumented vendor
functions; a C form would need an explicit finish at each of those exits.

## Checks

- `cargo fmt --all -- --check`.
- `rsf-fsr4-compat`: `cargo check`, `cargo clippy --all-targets` and
  `cargo test --locked` (10 passed). Its test build first needed a `move`
  closure in `fsr3/owner.rs` swapchain creation: the failure closure borrowed
  `owner` while the SDK call took `&raw mut (*owner).chain`.
- `rsf-orchestrator`: the integration tip stops at name resolution, so the
  crate was type-checked and linted in a scratch merge with `tip-build-fix`.
  No diagnostics in the V05 files. Its library tests do not compile there for
  other chunks' fixtures.
- MSVC `/std:c11 /W4 /WX` for `native_sr_fsr4_vulkan.c` and for the WSI header
  with its implementation section; `/std:c++17 /W4 /WX` for
  `tests/ffx_vulkan_wsi_dispatch.cpp`.
- `cmake --build --preset windows-debug --target rsf_native_vulkan` with
  `/p:BuildProjectReferences=false` (the Rust core target does not build at the
  tip) compiled every native object of the library, including the new leaf.
- The pinned FidelityFX Vulkan SDK sources (`c6efa6bf`) staged with the
  lifetime and CPU-drain manifests, and again with the trace manifest, passed
  MSVC C++17 `/Zs` with the new `rsf-wsi-dispatch.h`.
- Not run: CTest (the tip's `tests/CMakeLists.txt` does not parse), bindgen
  (no compatible libclang on this machine), the SDK and standalone bridge
  builds, and device fixtures.
