# Vulkan Streamline call leaves in Rust (V04)

The last first-party C++ of the Vulkan Streamline path moved to Rust. This note records what moved,
what stays native and why, and what was checked.

## What moved

| Former C++ | Now |
| --- | --- |
| `runtime/vulkan/streamline/bridge.cpp` | `lifecycle/src/win32/bridge.rs` (all `rsf_vk_sl_*` exports and typed SDK calls), `bridge.c` (interposer digest) |
| `runtime/orchestrator/src/native_dlss_vulkan.cpp` | `vulkan_dlss_sr/leaves.rs`, table in `native_dlss_vulkan.c` |
| `runtime/orchestrator/src/native_fg_dlss_vulkan.cpp` | `vulkan_dlss_fg/leaves.rs`, table and `rsf_dlssgvk_*` exports in `native_fg_dlss_vulkan.c` |
| `runtime/orchestrator/src/native_fg_dlss_vulkan_capture.cpp` and `.h` | `dlss_vulkan_capture/leaves.rs` |
| `tests/native_fg_dlss_vulkan.cpp` | `tests/vulkan-fg/src/dlss/sdk.rs` with the shared C main |

The interposer exports its API with C linkage, and the references in the C++ declarations are
pointers in that ABI. `lifecycle/src/sdk_api.rs` names the entry types and builds the viewport,
adapter, resource and tag headers. The orchestrator includes it with `#[path]`, as it already did
for the record bindings. The records are C mirrors: `sdk_records.h` gains `rsf_slvk_sdk_viewport`,
`rsf_slvk_sdk_adapter`, `rsf_slvk_sdk_reflex_state` (leading facts and an opaque 12808-byte tail)
and `rsf_slvk_sdk_pcl_state`; `sdk_fg_options.h` gains `rsf_slvk_sdk_fg_state`; the DLSS SR
options and optimal settings live in `sdk_sr_options.h`. MSVC measured every size and offset
against the 2.14.1 headers before the mirrors were written. The D3D12 Streamline host's
`streamline_sdk.rs` landed at the same time with hand-written mirrors of several of the same
records; `sdk_api.rs` stays separate because the standalone bridge crate compiles it too, and
merging the two is a follow-up.

The C++ leaves copied each wire record into a constructed SDK object with `memcpy`, so passing the
wire record directly is byte-identical. The determinant gate on `clip_to_previous` keeps the
DirectXMath rule (`XMMatrixInverse`, nonfinite or below 1e-12 refused) through the bit-exact
transcription in `dlss_native12/xm.rs`, now shared with the Vulkan leaves instead of a third copy.

The production tables stay in C (`native_dlss_vulkan.c`, `native_fg_dlss_vulkan.c`) so that the
Rust leaves reference no symbol outside the orchestrator crate. The tables add the session invoke
(`rsf_slvk_invoke`), the worker timeout epoch, the FrameDump parameter call and the shared DLSS
preset policy, which is a header-inline C function. Builds without the corrected Streamline runtime
pass a null table as before.

The DLSS-G native owner starts with `rsf_dlgvk_sdk`, its five SDK entries. The fixture's
initialize leaf writes synthetic entries there, replacing the former `#include` of the C++ sources.
The capture fixture copies the exported Rust capture table and builds its native with
`rsf_core_dlss_capture_native`, which takes the registration entry and loads the real command and
fence operations.

## What stays native

- `sdk_guard.c`: SEH containment. The Rust thunks and SDK entry types use the C-unwind ABI, so a
  foreign C++ exception (0xE06D7363) unwinds to the C frame and becomes eErrorExceptionHandler,
  the result the C++ catch-alls returned. Each crate compiles its own copy; the orchestrator's is
  `rsf_core_slvk_guarded` so both archives link into one Runtime. The interposer also guards its own
  API with SEH (`SL_EXCEPTION_HANDLE_*`, defined with `SL_ENABLE_EXCEPTION_HANDLING` in its
  premake project). The same file holds the `sl::FrameToken` virtual call (vtable slot 0). The
  MinGW standalone bridge has no SEH and relies on the interposer's own guard.
- `bridge.c`: the corrected or pinned retail interposer digest, which CMake and `build.ps1`
  inject at compile time.
- `sdk_records_checks.h` and `sdk_fg_options_checks.h`: layout, GUID and enum assertions against
  `sl.h`, which is C++ only. `tests/dlss_sr_layout_facts.cpp` compiles the first and
  `tests/streamline_selected_device.cpp` the second. They now also cover the new records, the
  result and log codes the Rust leaves name, and the FrameToken vtable assumption.
- The SDK corrections in `sdk-fixes/` and the shims that test them. They are C++ inserted into
  Streamline's own C++ translation units, and their hashes pin the corrected SDK build receipt.

## Behaviour notes

- Exceptions: a C++ exception from an SDK call now becomes eErrorExceptionHandler at the call and
  then the leaf's usual failure result. The SR initialize leaf returned INIT_FAILED for any
  exception; an exception in its support or plan query now reports NOT_SUPPORTED.
- The DLSS-G configure callback is `rsf_core_dlgvk_report_api_error`, and the bridge's is the
  lifecycle diagnostics store, as before.
- The standalone bridge builds with gcc in C11; it links no C++ runtime.

## Checks

- MSVC `/Zs /W4 /WX` of both check headers against the vendor 2.14.1 headers, the C leaves with
  and without `RSF_HAVE_STREAMLINE_VK`, `native_sr_vulkan.c` with Streamline and FidelityFX, and
  `tests/dlss_sr_layout_facts.cpp` and `tests/streamline_selected_device.cpp` with the extended
  checks.
- bindgen 0.72.1 regeneration; only the four V04 bindings changed.
- Rust 1.94.1: `cargo fmt --check`; `cargo test -p rsf-streamline-vk` (18 tests); clippy for the
  lifecycle crate and the Vulkan FG fixture crate (`dlss`, `streamline-selected`,
  `streamline-dispatch`). The orchestrator crate does not resolve on the current tip, so its
  modules were type-checked and linted over a temporary merge of the tip-build-fix branch, which
  was then aborted.
- Not run: the CMake build, CTest and the standalone gcc bridge, blocked by the tip build.
