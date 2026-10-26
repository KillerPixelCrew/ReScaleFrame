# Streamline requirements for the selected Vulkan device

The question was whether initializing Streamline before Godot creates its Vulkan
device can force NVIDIA requirements onto an AMD adapter, especially when both
vendors are installed. Source inspection found two independent causes. The
correction below is built and CPU-tested. No AMD device execution is claimed.

## Source evidence and decision

The pinned Streamline 2.14.1 checkout is commit
`2122257e0fce486f91b385aa63b9a09b0a34b363`. Relevant paths in that checkout:

- `source/core/sl.plugin-manager/pluginManager.cpp`,
  `PluginManager::getLoadedFeatureConfigs`, returns every loaded configuration,
  including configurations whose `supportedAdapters` mask is zero.
- `source/core/sl.interposer/vulkan/wrapper.cpp`, `vkCreateDevice`, merges every
  returned configuration's mandatory and optional device extensions, Vulkan 1.2
  and 1.3 features, optical-flow features and extra queues. It does not check the
  physical device selected by the caller.
- `source/plugins/sl.common/commonEntry.cpp`,
  `updateCommonEmbeddedJSONConfig`, computes `supportedAdapters` using the
  common plugin's adapter numbering. Common and PCL accept all eligible adapters.
  DLSS requirements can be present because a different installed GPU supports
  DLSS.
- `source/core/sl.api/sl.cpp`, `slIsFeatureSupported`, returns success without
  the plugin's adapter callback when `deviceLUID` is null, even if
  `vkPhysicalDevice` is provided. It cannot independently establish applicability
  for this pre-device filter.
- `source/plugins/sl.common/commonEntry.cpp`, `findAdapter`, permits either a
  device-ID match or a LUID match when Vulkan physical-device information is
  supplied. Device ID alone cannot distinguish two adapters of the same model.

The wrapper now queries the selected `VkPhysicalDevice` through the existing
instance dispatch and requires `VkPhysicalDeviceIDProperties::deviceLUIDValid`.
It passes only that eight-byte LUID to the common plugin's published
`sl.param.common.findAdapter` lookup. Testing individual mask bits recovers the
common plugin's exact selected-adapter bit without copying its private layout or
assuming DXGI and Vulkan enumeration orders match. A missing lookup, invalid
LUID, absent match or multiple matching bits refuses device creation explicitly.

Before merging any plugin device requirement, the wrapper requires that plugin's
mask to contain the selected bit. It also clears the singleton optical-flow
support flag before recomputing the selected device's requirements. Host-supplied
requirements and common SDK baseline requirements are preserved. This change
applies at device creation; instance requirements are collected earlier, when
there is no selected physical device.

## Reflex correction and runtime use

A mask-only implementation was insufficient. In
`source/plugins/sl.reflex/reflexEntry.cpp`, `updateEmbeddedJSON` deliberately
reports all adapters as supported because statistics are GPU-agnostic. Its
`lowLatencyAvailable` and mandatory `VK_NV_low_latency` / `VK_NV_low_latency2`
requirements can nevertheless be derived from any installed NVIDIA adapter.
The focused helper therefore excludes Reflex's device requirements when the
selected physical device's `vendorID` is not `0x10de`. It keeps the Reflex plugin
loaded and preserves common/PCL requirements.

The runtime audit found a second required guard:
`source/platforms/sl.chi/vulkan.cpp`, `Vulkan::init`, previously tried both
`CreateVkNvLowLatency2` and the optional `CreateNvLowLatencyVk` fallback
unconditionally. The fallback in `nvllvk.cpp` loads `NvLowLatencyVk.dll` and
initializes its low-latency device API. Both constructors are now inside a check
of the actual selected physical device's vendor. Non-NVIDIA devices retain a
null `m_reflex` and continue common compute initialization successfully.

Every compute sleep, sleep-mode, marker, report and swapchain notification path
checks `m_reflex` before dereferencing it. Reflex's swapchain before-hook checks
for `vkLatencySleepNV` before adding its latency creation structure; it does not
call that entry point. `slReflexGetState` changes its availability flags to false
when the compute query reports no backend. The existing first-party
`native_latency_vulkan.cpp::bind` queries that state before using Reflex, so the
reported low-latency and report availability reflect the selected device. PCL
binding and marker storage remain separate and available.

There is a remaining vendor-owned refusal path on mixed-adapter systems. Common
sets `needNGX` when any loaded plugin supports any adapter, then initializes NGX
using the actual selected Vulkan device. A failed NGX initialization logs that
NGX features will be disabled and continues common initialization. DLSS SR
startup explicitly refuses a missing NGX context, missing parameters or an
unavailable super-sampling capability. The plugin manager unloads plugins whose
startup returns false and continues initializing the other plugins. The signed
DLSS-G plugin's complete AMD startup behavior was not independently executed or
proved here. This patch does not claim positive AMD startup or rendering
acceptance from those source paths.

## Patch, checks and artifacts

The checked patch is
[`selected_device.py`](../../runtime/vulkan/streamline/sdk-fixes/selected_device.py),
with the first-party
[`selected-device-config.h`](../../runtime/vulkan/streamline/sdk-fixes/selected-device-config.h).
`prepare.py` invokes it after the existing corrections. All three exact source
anchors are validated before either source file is changed. The patch emits
`rsf-selected-device-fixes.json` with the exact edits and before/after SHA-256
hashes of both SDK source files. The SDK build receipt, staging validation and
private bridge build also check the helper and patch hashes.

CPU checks performed on Windows:

- `tests/streamline_selected_device.cpp`, compiled and executed with installed
  LLVM-MinGW, C++17, `-Wall -Wextra -Werror`: AMD/NVIDIA selection, distinct LUIDs
  for identical-model adapters, zero masks, unknown and ambiguous identities,
  missing lookup, Reflex's real all-adapter mask, and common/PCL retention pass.
  CMake registers this as `streamline_selected_device_contract` when SDK headers
  are available.
- `tests/test_streamline_selected_device_patch.py`: four tests pass against the
  pinned local source. They check guard placement before every feature,
  extension, queue and optical-flow branch plus both vendor-gated latency
  constructors; changed-anchor refusal without source mutation; duplicate
  application refusal without source mutation; and composition through the
  complete current `prepare.py` recipe with both source receipts present.
- All edited PowerShell files parse and `git diff --check` passes. The reference
  checkout remains pristine. Full native verification belongs to the coordinated
  parent gate and is not inferred from these focused checks.

The final isolated Production SDK build completed successfully using MSVC on
2026-10-25. Both `sl.compute` and the changed common/interposer projects built.
No GPU test, package staging or user application change was performed.

Final runtime directory:
`.local/godot/vulkan/host/artifacts/streamline-production-selected-device-reflex/runtime`.
Build log: `.local/godot/streamline-selected-device/sdk-build-reflex.log`.
The directory contains `streamline-sdk-build.json`, `vulkanstory-fixes.json`
and `rsf-selected-device-fixes.json`.

| Input or output | SHA-256 |
| --- | --- |
| `sl.common.dll` | `17D1D02041F5BF8EAE51D2682D1F91755E5E4EA3371702F1DF7B9432CF92A80C` |
| `sl.interposer.dll` | `B115E2B70EC3A58D831396764ECF747792D9B814596C66453F483FDB98D6E9F6` |
| `prepare.py` | `4CD5BC5E0E0C6671D2C80D11F15CA5E758BDBBED7E984CDDC04F9F82360536BD` |
| `selected_device.py` | `A71D05DC13BFB0587E4C7F5ACC73848489955E6DAF67938F57751D3057E3B8A6` |
| `selected-device-config.h` | `8C6C7DAEF6FA42F16EC5C0C6E6684AD9D2002D73C43AB6949754C0AE82037761` |
| `build-sdk.ps1` | `D503DCCF7E6AC1F21CBFE27520F412FE07FA0FC15799AFCBAD2B6A30E3D4FD97` |

The earlier mask-only SDK build in
`.local/godot/vulkan/host/artifacts/streamline-production-selected-device` is a
superseded source control. It built successfully but omitted the Reflex vendor
guards discovered above and was never staged. Preserve its compact receipt as
the failed-hypothesis record; it is not the runtime to select.

Positive AMD, mixed-adapter and identical-model multi-adapter runtime validation
remain required. NVIDIA image/lifecycle regressions also require a fresh paired
runtime capture. No readiness flag or image gate was changed.

## Staged NVIDIA regression

The coordinated native gate subsequently passed: 75 tests discovered, 64
executed and 11 skipped, with formatting and Clippy checks passing. The parent
staged Native SHA-256
`3EBCE3E011211E195A573CFE786FBCA465E3A85967D5A68A3B1F91ABDFD3F9B9`
with the final common/interposer hashes above. Three actual Godot Vulkan
regressions then passed on the NVIDIA GeForce RTX 5080:

| Fixture | Manifest | Result |
| --- | --- | --- |
| Standard, camera-free PCL, native WSI, no C# | `.local/godot/validation-20261025-210352-469.json` | PCL, low-latency and report availability; input events; 120 sleep calls and 717 accepted marker calls |
| Mono, same camera-free fixture, no C# | Same manifest | Same bounded functional result |
| Standard, DLSS SR1, four raw input/output captures, no C# | `.local/godot/validation-20261025-210439-338.json` | SR1 to SR3 switching, resize/reset, actual player PNGs and four complete guide/output frames |

All three used the portable Khronos layer, synchronization validation,
`VK_LOADER_LAYERS_DISABLE=~implicit~`, per-process legacy-extension-point
mitigation and explicit `-MaintainForeground`. Their process receipts report
effective mitigation policy 1, no RTSS module, one successful owned-window
activation and zero runtime foreground-loss samples. API and synchronization
validation reported no errors. Every fixture held the shared GPU lease; no GPU
run or process remained active afterward.

The loaded-module receipts independently identify the staged Native, common and
interposer DLL paths. Hashing those immutable fixture paths confirmed the exact
three hashes above. Godot's retained logs identify device 0 as NVIDIA GeForce RTX
5080 and announce Streamline Vulkan dispatch/PCL startup. The standard engine is
`DE9BA015EDC32412380BE6A57B59118BAC604517538C957371ADF6D8946145CB`;
the Mono engine is
`4C6461FC557BE416EAF35207A92B7963ECC3487B8FE224C36F9AA33729A594D0`.
All three used runtime manifest SHA-256
`DE674B4FECE1D76F472CBED0F6185DE426EE206D0D601E4D9B8462817FCD59FD`.
The loaded validation DLL remains
`E86FA4F70676A120F00A32BEAC6979CEB3CAFCE70DE42FA9107C89F587AB206E`.

The compact module/foreground/selected-device receipt is
`.local/godot/streamline-selected-device/nvidia-regression.json`. The retained
fixture directories are:

- `.local/godot/validation/standard/pcl-20261025-210352-469`
- `.local/godot/validation/mono/pcl-20261025-210352-469`
- `.local/godot/validation/standard/sr-20261025-210439-338`

The SR analyzer's report is `frames/decoded/analysis.json` in the last directory.
Maximum object/floor vector p95 error is 0.000108/0.000341 render pixels, and
maximum raster-jitter metadata error is 0.000806 render pixels. The actual
`frame-30-sr-guides.png` was visually inspected: scene color, SDK output, depth
and motion belong to the expected cube/floor scene. This is input/output and
functional regression evidence, not an independent SR image-quality comparison,
DLSS-G lifecycle acceptance or a measured latency reduction. AMD and mixed-GPU
execution remain untested.
