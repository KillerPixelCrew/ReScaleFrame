# Vulkan SR provider admission port

This V01 checkpoint started at `70e01825`, using the G05 sharpening owner,
V05 split FSR4 owner and V04 capture role helper already in that base.

FidelityFX SR uses the common Vulkan image allocator transaction. Its memory
policy first chooses the lowest compatible device-local index, then the
highest compatible index within the reported type count. An incomplete
replacement rolls back immediately. The SDK owner keeps every completed
allocation, the module and the device-dispatch registration until checked
effect destruction succeeds. XeSS follows the same cleanup-only publication
contract and retains its SDK session and completed private images. The shared
dispatcher refuses plan, preset and evaluate calls for cleanup owners. Godot
already records the returned handle independently of the result.

`sr-output-linear` is recorded before RSF sharpening for FidelityFX SR, XeSS
and split FSR4, matching DLSS. XeSS and FSR4 use the V04 input role mapping.
Their recording command, frame identity and reset decisions stay at the
original call sites. FSR4 records output on the Vulkan continuation after
external ownership acquisition. The capture operation restores GENERAL for
sharpening, before the normal transfer to the engine output. Existing
submission and completion gates control publication.

The public split creator gives the native provider the validated fixed records.
The independent FSR4 create entry still validates raw callers, including Godot,
and retains its private result mapping. The split lease is Rust-only.

## Closed Rust findings

- Share Vulkan image allocation while preserving FidelityFX's nonlocal memory fallback.
- Give the SR output capture the same stage meaning across Vulkan providers.
- Return a cleanup-only Vulkan SR owner when failed creation cannot retire the SDK.
- Honor XeSS context destruction failures during constructor rollback.
- Exercise the production Vulkan SR admission path in XeSS owner fixtures.
- Set fake Vulkan blit capabilities on the fake device.
- Store each XeSS private image descriptor once.
- Remove the global raw pointer from the XeSS test fixture.
- Share the Vulkan SR test callback macro between sibling fixtures.
- Use the admitted Vulkan FSR4 setup when constructing the split owner.

## Closed inventory paths

- `runtime/orchestrator/include/rescaleframe/native_sr_split_vulkan.h` and
  `runtime/orchestrator/include/rescaleframe/native_sr_vulkan.h` contain C ABI declarations.
- `runtime/orchestrator/src/native_sr_split_lease.h` was removed. The Rust split-lease
  contract calls the production methods.
- `runtime/orchestrator/src/vulkan_fidelityfx_native.h` contains typed SDK declarations
  and a pointer record. Rust now implements the shared SR and FG symbol callback.
- `runtime/orchestrator/src/vulkan_sdk_native.h` contains C ABI declarations and
  an immutable loader table.
- `tests/native_sr_xess_range_check.py` was removed. It extracted a C++ dispatch
  implementation that had already moved. The Rust and native XeSS fixtures
  now exercise range admission through the shared production dispatcher.
- `runtime/orchestrator/src/native_sr_vulkan.cpp` was replaced by the C11 leaf
  described below.
- `tests/native_vulkan_exports.cpp` was replaced by `tests/native_vulkan_exports.rs`,
  retaining all 31 export names, the separate process and the real DLL loader check.

## Remaining native code

- `runtime/orchestrator/src/native_sr_split_vulkan.cpp` supplies the CMake-selected
  provider and DLL export thunks. The stub fixture still exercises its exception
  boundary and continuation preservation.
- `runtime/orchestrator/src/native_vulkan_timing.h` owns native DLL-scope
  retirement for the Rust timing file. Native FG/WSI, fence waits and latency
  calls still consume this scope.
- `tests/native_sr_split_vulkan.cpp` still checks compiled, stub and disabled
  provider wiring, DLL imports and C++ exception containment.
- `tests/native_sr_vulkan.cpp` still owns the synthetic capture leaf and device
  fixture with direct Vulkan calls and packed-pixel checks.

The remaining files require a coordinated native export/feature-selection
mechanism, migration of native timing call sites, or typed Vulkan/SDK bindings
for the C++ leaves and device fixtures. Preserve those contracts during a full
port.

## Native SR Vulkan C11 leaf

`native_sr_vulkan.cpp` became `native_sr_vulkan.c`. Rust already owned provider
admission, descriptor assembly, images, capture and retirement, so the leaf keeps
three things Rust cannot express:

- the `static_assert` layout checks of the FidelityFX create, plan, dispatch and
  resource records against the configured SDK headers, now `_Static_assert`;
- the six FidelityFX SDK calls and the `vkGetDeviceProcAddr` call that contained
  foreign C++ exceptions. Each `catch (...)` became `__try/__except` filtered on 0xE06D7363,
  which is the only code MSVC `/EHsc` catch-all caught, and returns the same
  result code. Access violations still propagate;
- the CMake-selected platform table and the seven `rsf_srvk_*` export thunks.

The platform record keeps 264 bytes and the FidelityFX ops record 28 pointers,
with the same offsets that `vulkan_sr.rs` mirrors. `ffx_api_vk.h` names
`VkQueueInfoFFXAPI`, `FfxApiResource` and `FfxApiResourceDescription` without
`struct`, so the leaf typedefs them before the include.

Callers changed for C linkage:

- `vulkan_sdk_native.h` and `vulkan_fidelityfx_native.h` hold C declarations only.
  The loader table is the `RSF_VK_SDK_SYSTEM_LOADER` initializer, used by this leaf
  and by the V02 FidelityFX FG table in `native_fg_vulkan.cpp`. The FG leaf now
  value-initializes its `rsf_vkffx_exports` member, because the header lost its
  C++ default member initializers.
- `native_dlss_vulkan_platform.h` (V04) replaces the C++ `rsf::dlss_vk::platform`
  constant with the C symbol `rsf_vkdlss_native_leaves` and the
  `RSF_VKDLSS_NATIVE_PLATFORM` selector; `native_dlss_vulkan.cpp` defines that symbol.
- The XeSS table uses V06's `RSF_XESSVK_NATIVE_PLATFORM`.

## Evidence

Source checks compared the C++ and Rust FidelityFX callback record sizes and
offsets, checked all three pre-sharpen capture sites and compared the 31-name
export inventory. Bindgen 0.72.1 regenerated the Vulkan SR bindings without a
diff. Rustfmt from Rust 1.94.1 and `git diff --check` passed. The changed fixture
expectations and rollback injections are source evidence. Build, test, lint and
device validation remain user-owned.

V02 also edits the shared image resource file and FSR4 native translation unit.
Its fence wait change is independent of V01's image allocator and its FSR4
copy-table edits are independent of the admitted create callback. The moved
FidelityFX symbol callback retains the name and signature used by the V02 leaf.
V03 can keep consuming the existing `valid_image` predicate.
