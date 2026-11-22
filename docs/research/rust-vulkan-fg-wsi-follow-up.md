# Shared Vulkan FG dispatch and CPU callback follow-up

Base revision `fb27ab2f`. Rust selects the registered provider and converts
its operational status codes in `vulkan_fg.rs`. Registration still names the
four available provider boundary tables, and a missing table still reports
`NOT_COMPILED`. The C++ boundary supplies SDK-specific factory options and
status records and catches native exceptions. Factory exceptions retain the
original initialization failure. Acquire, image enumeration and Present still
return Vulkan results directly, and the submitted command array reaches the
same callback without reinterpretation or reordering.

`vulkan_fg_wsi.rs` now owns FidelityFX's promoted device-proc name retry,
graphics submit forwarding and drain ordering. C++ guards each native resolver
invocation so an exception remains distinct from a missing function. The
caller still supplies the queue user and the exact submit count, pointer and
fence. Drain acquires the caller's queue lock, runs the SDK wait callback,
waits for device idle only after the SDK wait succeeds and unlocks even on
refusal. The SDK still owns its descriptor records, WSI registration ABI and
callbacks. The route borrow still spans the forwarded call.
`vulkan_fg_resources_native.h` remains the SDK leaf's lifetime wrapper for
the Rust resource dispatch table.

| Fixture source | Rust ownership and retained boundary |
| --- | --- |
| `tests/vulkan-fg/dispatch_fixture.cpp` | `dispatch_drivers.rs` supplies the fake provider entry points and the blocked XeSS sleep. C++ selects the enabled fixture variant and includes the production provider boundary. |
| `tests/vulkan-fg/ffx_fixture.cpp` and `tests/native_ffx_vulkan_harness.h` | `ffx_gpu.rs` supplies Vulkan memory, image, command, fence, queue and WSI fakes, including reentrant route retirement. C++ still supplies SDK callback records and the SDK registration adapter. |
| `tests/ffx_vulkan_wsi_dispatch.cpp` | `wsi.rs` supplies the native and host Vulkan fakes and counters. C++ still compiles the corrected SDK header and forwards its typed registration and calls. |

The neutral public entry names, V05 and V06 leaf paths, vendor patch inputs and
CTest names are unchanged. The existing Rust scenarios still check disabled
providers, failed fence submission, reused command receipts, the latency
borrow, foreign-device routing and deferred retirement.

Source verification used targeted bindgen 0.72.1 regeneration for
`vulkan_fg_abi.rs`, `dispatch_abi.rs`, `ffx_abi.rs` and `wsi_abi.rs`.
Pinned Rust 1.94.1 formatting and
`cargo metadata --locked --offline --all-features --filter-platform x86_64-pc-windows-msvc --format-version 1`
passed. MSVC `/Zs /W4 /WX /std:c++20 /EHsc` accepted the three dispatch
fixture configurations, standalone corrected SDK WSI fixture, shared WSI
header and the SDK-disabled FidelityFX leaf. The SDK-enabled FidelityFX leaf
and callback harness need the optional Vulkan FidelityFX headers for their
syntax check. Runtime behavior remains unverified.
