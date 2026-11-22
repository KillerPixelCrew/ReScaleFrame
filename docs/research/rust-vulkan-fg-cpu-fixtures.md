# Vulkan FG CPU fixture port

Base revision `031aa9ac`. The fixture drivers now call the same Vulkan FG
exports from Rust. They link the integrated runtime owner and use the
production C++ entry points where the export or SDK callback still crosses
that boundary.

| Previous source | Rust scenario and retained coverage |
| --- | --- |
| `tests/native_fg_vulkan_dispatch.cpp` | `tests/vulkan-fg/src/dispatch.rs` retains ABI validation precedence, independent SR and FG selection, exclusive owner admission, retained partial owners, explicit source identity and unchanged WSI results. CTest still selects default, FSR4-enabled and XeSS-enabled configurations. Each absent provider must return `NOT_COMPILED`. The XeSS path holds a real runtime latency borrow through blocked fake SDK sleep and checks that drain and destroy return `NOT_READY` and close new borrows. |
| `tests/native_ffx_vulkan_reset.cpp` | `reset` in `tests/vulkan-fg/src/ffx.rs` retains the SDK reset flag checks, stable activation, rejected guides, completion timeout, exact older-source selection and failed generation accounting. |
| `tests/native_ffx_vulkan_submission.cpp` | `submission` in `ffx.rs` retains image replacement rollback, enumeration capacities, duplicate and reused command receipts, a partial prepare that requires a receipt before retirement, split prepare and HUD-less fence ordering, timeout, exact source provenance, stored and late FG vetoes, and source and consumer waits after a Present error. The fake can now also fail the zero-command fence submission with `VK_ERROR_DEVICE_LOST`. That failure must leave the source ineligible even though the host command receipt was recorded. |
| `tests/native_ffx_vulkan_wsi_route.cpp` | `wsi_route` in `ffx.rs` retains foreign-device refusal, core alias lookup, caller queue forwarding, withholding of new callbacks during retirement, deferred close under a live borrow, failed unregister retry and successor admission. |

`dispatch_fixture.cpp` still supplies fake providers to the production
dispatch table, including the XeSS sleep callback and its synchronization.
`ffx_fixture.cpp` and `native_ffx_vulkan_harness.h` supply FidelityFX SDK
records, Vulkan callback fakes and ABI adapters for the native WSI hooks.
Assertions inside those callbacks check the SDK-facing arguments. The
original four C++ scenario files are removed and their six CTest names
remain. The corrected SDK dispatch fixture from the prior port keeps its
own existing target.

Verification is source-only. Bindgen 0.72.1 regenerated the fixture bindings.
The affected crate passed pinned Rust 1.94.1 formatting, locked offline
workspace metadata and diff checks. The test executables and their runtime
behavior remain unverified for this revision.
