# Vulkan FidelityFX 3 FG SDK leaf in Rust (V02)

This closes the V02 C++ files. `native_fg_vulkan.cpp`, the last first-party
C++ of the FidelityFX 3 Vulkan frame-generation provider, moves to
`runtime/orchestrator/src/vulkan_ffx_fg/sdk.rs`. The provider table in
`native_fg_vulkan_dispatch.cpp` becomes the C11 leaf
`native_fg_vulkan_dispatch.c`. The FidelityFX fixture and the corrected SDK WSI
fixture lose their C++ as well. The owner in `vulkan_ffx_fg.rs` keeps every
admission, slot, fence, presentation and retirement rule described in
[rust-vulkan-ffx-fg-cutover.md](rust-vulkan-ffx-fg-cutover.md) and
[rust-vulkan-ffx-wsi-owner.md](rust-vulkan-ffx-wsi-owner.md).

## What moved

| Former native step | Rust or C11 owner |
| --- | --- |
| SDK descriptor records for the swapchain context, effect, prepare, configure, provider query, replacement-function query, generation dispatch and present drain | `sdk.rs`, built from `runtime/vulkan/fsr3/sdk_abi.rs` (bindgen 0.72.1 over the pinned SDK `c6efa6bf`). The generator now also emits `ffxQueryGetProviderVersion` and `FfxApiCreateContextFramegenerationFlags`. |
| The five corrected-SDK WSI hooks, the backend `vkDeviceProcAddr`, the game-queue submit and the generation callback | `sdk.rs`. Each hook still borrows the leaf from the Rust route for the whole callback, including the forwarded Vulkan call. |
| `ffxApiGetSurfaceFormatVK` | `surface_format` maps the four formats the owner admits (RGBA8, BGRA8, RG16 float and R32 float) to the same SDK values. No other format reaches the leaf. |
| Vulkan and SDK constant `static_assert`s | `const` assertions against `ash` and the SDK bindings. |
| `rsf_ffxgvk_native` lifetime, capture and resource tables | `sdk::Native`, dropped by the owner's close step. |

The owner's step table (`Ops`) is now Rust only. Its module loader moved into
the creation-time host table, so `Ops` no longer has a `repr(C)` layout.
`startup::open` remains the ordered binder and its unit test is unchanged
apart from the host argument.

## What stays native, and why

`native_fg_vulkan.c` keeps three things:

- The `RSF_HAVE_FFX_VK` gate. CMake sets it from header detection and the
  crate has no feature for it, so the C leaf passes either the production host
  table or null to `rsf_core_ffxgvk_create_sdk`. A null host validates the
  arguments and reports `NOT_COMPILED`, as the former disabled stubs did. With
  no owner ever created in that build, every other entry gives the same result
  through the Rust owner for null.
- The `rsf_ffxgvk_*` provider symbols. The dispatch fixtures link synthetic
  providers under the same names, so the production entries must live in a
  native object the fixtures can leave out. Each entry forwards to its
  `rsf_core_ffxgvk_*` owner.
- Exception containment for the host's Vulkan resolvers and
  `vkGetPhysicalDeviceMemoryProperties`, the only calls the former leaf wrapped
  in `catch (...)`. The guards use an SEH filter on the MSVC C++ exception
  code `0xE06D7363`, as in `plugin_callbacks.c`. The former catches around
  Rust capture and resource calls were unreachable, because Rust aborts rather
  than unwinding through `extern "C"`.

The host table (`rsf_ffxgvk_host`) also carries the latency scopes
(`rsf_slvk_without_dlssg`, `rsf_latencyvk_wsi_call`,
`rsf_latencyvk_queue_present`) from `native_latency_vulkan.c`. The Rust crate
therefore has no link-time reference to native-only symbols, and a fixture can
supply its own scopes. Both sides assert the eight-pointer layout.

`native_fg_vulkan_dispatch.c` keeps the provider table because the compile
gates are CMake defines and the fixtures substitute providers at link time.
Every adapter only restores the provider's owner type. The former
`catch (...)` wrappers had nothing to stop: each provider entry is a thin
forwarder to a Rust owner. MSVC with `/EHsc` also assumes `extern "C"`
functions do not throw.

`vulkan_fg_wsi.h` had no remaining includer and is deleted. Its Rust
functions are crate-internal now.

## Fixtures

`tests/vulkan-fg/ffx_fixture.c` is a C11 launcher. `src/ffx_sdk.rs` fakes the
SDK module through the host table (loader, `ffxConfigure`,
`ffxCreateContext`, `ffxDestroyContext`, `ffxQuery`, `ffxDispatch` and
`rsfFfxSetVulkanWsiDispatch`) and the fake GPU's instance and device
resolvers. The production leaf now runs every step in all three scenarios,
including startup binding, descriptor assembly, the hooks and the generation
callback. Before, C++ fakes replaced its open, creation and query steps. The
fake records context slots and the callback, resolver and submit pointers
that the leaf hands to the SDK, then drives the hooks through the registered
table, as the corrected SDK does. The fixture links `native_fg_vulkan.c`
without `RSF_HAVE_FFX_VK` and uses its production guards in the fixture host.
Two status assertions in `ffx.rs` that never compiled
(`status.enabled != 0 == 0` and `&& status.enabled`) now compare
`status.enabled` with zero. `dispatch.rs` now names `std::mem::offset_of!`.

Once the scenarios compiled, the submission scenario showed a stale
expectation. After a HUD-less submission whose prior source fence timed out,
`rsf_core_vkfg_fence_submission` marks the slot `completion_unknown`. No fence
covers that copy, so the slot is never reused before teardown drains the
device. The scenario still reused that slot as frame 7 of session 99, which
the ported C++ scenario did against an older owner. It now asserts that the
slot refuses `prepare` with `NOT_READY` and records no SDK work, then uses
frame 9 for the late veto steps.

`tests/ffx_vulkan_wsi_dispatch.c` replaces the C++ forwarders. V05 made the
corrected SDK's `wsi-dispatch.h` implementation section C11, so the fixture
compiles it directly and gives its static entries linkage.

## Verification

- `cargo fmt --all -- --check` passed.
- `cargo clippy -p rsf-vulkan-fg-fixtures --locked` passed with the `ffx`,
  `dispatch` and `wsi-dispatch` features, with no warnings in the crate.
- The `rsf-orchestrator` crate does not build at the integration tip. On
  `NightHammer1000/tip-build-fix` plus this change, `cargo build -p
  rsf-orchestrator` reported no error or warning in the touched modules. The
  crate's lib tests need native symbols outside Cargo. In a scratch copy that
  disabled unrelated broken test modules and stubbed those symbols,
  `cargo test -p rsf-orchestrator --lib vulkan_ffx_fg` passed all seven tests.
- bindgen 0.72.1 reproduced `runtime/vulkan/fsr3/sdk_abi.rs` byte for byte
  with the Vulkan SDK 1.4.357 headers before the allowlist change.
  `tests/vulkan-fg/src/dispatch_abi.rs` and `wsi_abi.rs` regenerate unchanged
  from the C headers.
- MSVC `/Zs /W4 /WX /std:c11` accepted `native_fg_vulkan.c` with and without
  `RSF_HAVE_FFX_VK`, all three dispatch configurations, `ffx_fixture.c`,
  `ffx_vulkan_wsi_dispatch.c`, and `native_fg_ffx_vulkan.h` compiled as C++20.
- On the same `tip-build-fix` base, CMake (VS 18, with the FidelityFX Vulkan and
  Vulkan headers) built `rsf_native_vulkan` with `RSF_HAVE_FFX_VK`. The three
  `native_ffx_vulkan_*_contract` CTests passed. They needed scratch-only stubs
  for 79 unrelated native symbols that the fix branch's Rust archive pulls into
  test executables. None of those symbols belong to this chunk.
- The dispatch fixtures compile but do not link on that base. Their synthetic
  `rsf_fsr4gvk_*` providers collide with the Rust FSR4 owner's exports
  (LNK2005) once the orchestrator archive is pulled in. The Runtime DLL stops
  earlier, on `/WX` C4206 in `dll_exports.c` and
  `input_dump_vulkan_exports.c`. Both are outside V02.
- The C11 WSI fixture, compiled by hand against the `wsi-dispatch` fixture
  archive and the Vulkan SDK 1.4.357 headers, built with `/W4 /WX` and exited
  with 0.
