# FidelityFX Vulkan bridge follow-up

The port starts at `031aa9ac`. It moves first-party SDK calls, resource
allocation and queue calls across the existing FSR4 Rust boundaries. Both
bridges still use the SDK's DX12 resource-description helper with live COM
resources, preserving the SDK's format, type and usage mappings.

## Source dispositions

| Source | Disposition |
| --- | --- |
| `runtime/vulkan/fsr4/bridge.cpp` | Rust now creates and exports shared textures, records the reconstruction and its barriers, closes the list, executes it and calls queue Wait and Signal. Native code retains the typed capsule, SDK description helper, LUID discovery, loader, compatibility lease, command/fence setup, host event wait and legacy exports. |
| `runtime/vulkan/fsr4-fg/bridge.cpp` | Rust marshals the retained DXGI swapchain description and invokes the SDK Configure, PrepareV2 and WaitForPresents calls. Rust creates and describes shared textures, transitions guide resources, records the backbuffer copy and calls queue Wait, Execute and Signal as well as DXGI Present and GetLastPresentCount. The native capsule keeps the SDK's retained create descriptors, typed resource helper, generation callback, COM setup and teardown calls. |
| `runtime/vulkan/fsr4/bridge_api.h`, `runtime/vulkan/fsr4/bridge_platform.h`, `runtime/vulkan/fsr4-fg/bridge_api.h`, `runtime/vulkan/fsr4-fg/bridge_platform.h` | The existing public and private ABI contracts and callback table orders still apply. SDK call projections are regenerated in each bridge's `sdk_abi.rs`. |
| `runtime/orchestrator/src/native_sr_fsr4_vulkan.cpp`, `native_sr_fsr4_vulkan.h`, `native_sr_fsr4_vulkan_platform.h` | Vulkan graphics, capture and handoff leaves remain in the existing platform binding. The synthetic fixture uses its generated Rust callback layout. |
| `runtime/orchestrator/src/native_fg_fsr4_vulkan.cpp`, `native_fg_fsr4_vulkan.h`, `native_fg_fsr4_vulkan_platform.h` | The Vulkan provider table and interop wiring remain in the existing platform binding. |
| `runtime/vulkan/fsr3/bridge.cpp`, `bridge_platform.h` | Standalone bridge Rust cutover already present in the base. The FSR3 SDK bindings regenerated without a diff. |
| `runtime/vulkan/fsr3/wsi-dispatch.h` and `sdk-fixes/{descriptor-pool-size,line-width,presenter-cpu-drain,swapchain-lifetime,swapchain-trace}.json`, `sdk-fixes/swapchain-trace.h` | Retained the receipt-hashed source payloads and the SDK build's patch order byte for byte. The WSI dispatch implementation runs inside the corrected SDK. |
| `tests/native_sr_fsr4_state.cpp` | C entry only. Rust owns the synthetic callbacks and scenarios, including admission, frame gaps, split submission receipts, capture resets and ordered checked retirement. |
| `tests/native_sr_fsr4_vulkan.cpp`, `tests/vulkan_interop_fixture.h` | Retired. The C11 launcher still lends the real SDK-gated public bridge create entry to the three Rust ABI checks. The optional device path uses the Ash owner and its real image and command operations. |
| `tests/native_fsr4_fg_state.cpp` | Retired. The C11 fixture launcher calls the Rust WSI cases and the existing Rust DX12 bridge state cases through the same SDK-gated CTest target. |
| `tests/native_fsr4_fg_bridge.cpp`, `tests/native_fsr4_fg_contract.cpp` | The linked public FG fixture and Rust SDK descriptor assertions were already ported in the base. |

## Ordering and checks

The SR record step resets the allocator and list before recording the four
input/output transitions. On SDK refusal it closes the list and returns the
SDK result. It returns the successful dispatch's four resources to COMMON
before closing. The owner retains its existing shared-ready wait, execute and
signal order. The FG sequence still performs the SDK present drain before local
completion and retains the separate post-present HUD-less release submission.
Its local wait uses the shared D3D12 helper with the same 10 second budget and
device-removal sentinel. A stale event wake continues within that budget.

The generated SDK layouts use bindgen 0.72.1 and pinned FidelityFX revisions
`7539ae8b6568ad29ed78a753404351daf3b5cca7` (FSR4) and
`c6efa6bf7f2027b3ec94f28578bb5965eabb9e55` (standalone FSR3, with Vulkan SDK
1.4.357.0). Clang reported the expected pragma-once-in-main-file warning.
Rust 1.94.1 formatting, locked offline Cargo metadata and Git whitespace
checks were completed. Compilation, linking, fixture execution, lint and device
verification remain pending.

The synthetic SR state fixture uses a C11 entry with the same CTest target
and Rust scenario owner. Strict MinGW C11 syntax passed for that entry and
five retained declaration headers. Native Vulkan/SDK glue and real-device
verification remain pending.


## Fixture source verification

The fixture follow-up starts at `9716068b`. Both CTest names and SDK header
gates are preserved. The SR test still runs the three public bridge ABI
checks without arguments and enters the device path only with
`--device <runtime-directory>`. The Ash device path uses Vulkan 1.2, a
graphics queue, Win32 external memory and semaphore extensions, and timeline
semaphores. It allocates and binds four real images, records two command
buffers, and submits the producer's ready signal and continuation's done wait
on the same queue after the split callback returns. It covers six frames,
the sharpening mode sequence, missing copy capability, wrong view, duplicate
evaluation, early destruction, wrong submission order, and fallback clear.
The reused loader resolves Vulkan from System32. Failed bridge destruction
retains the borrowed Vulkan objects and split callback state for process exit.

The FG Rust doubles still send an Ash `VkPresentInfoKHR` through
`rsf_fsr4gvk_present_source` and its shared descriptor reader. The cases cover
the short identity prefix, mismatch, repeated and exact older Present,
capture grouping and discard, guide replacement after fence retirement,
the binary wait plus prior timeline completion, adapter mismatch,
rollback retry, and a terminal marker failure.

Both fixture projections regenerated byte-for-byte with bindgen 0.72.1.
Rust 1.94.1 workspace formatting, all-feature locked offline Cargo metadata,
strict Clang C11 syntax checks for both entry points, and Git whitespace
checks passed. Build, link, fixture execution, and device behavior are
unverified for this follow-up.
