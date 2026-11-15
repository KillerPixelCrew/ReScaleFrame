# XeSS Vulkan input range and engine-quantized scene, 15 November 2026

## Evidence and question

The coordinator's `build/checks/rsf-final-xess-quality.log` records an optimal
provider input of 1130x636, a requested scene of 1130x636, and actual game images
at 1128x632, the renderer's eight-pixel grid. SR stayed pending with producer
reason 3 while preparations/HUD-less production continued (325 by source 1800).
Latency owner 2 reported 1798 sleeps, 21606 markers and zero refusals; generation
was still zero with result 3. These are coordinator observations, not a worker
GPU run, and they do not imply XeSS admitted the rounded SR input.

The SDK source contract already distinguishes recommendation from acceptance:
`vendor/xess/inc/xess/xess.h`, xessGetOptimalInputResolution documents the optimal
resolution and a possible minimum/maximum range, with all low-resolution input
buffers at the same resolution. `xess_vk.h`, xess_vk_execute_params_t carries
inputWidth/inputHeight separately from image-view allocation dimensions. The
existing service queried all three results at context creation but retained only
the recommendation and enforced equality during evaluate. That discarded actual
SDK evidence; replacing it with an invented alignment rule in RSF would be wrong.

## Corrections

The TotK resolution owner now normalizes provider requests down to the actual
1.4.3 eight-pixel scene grid. Zero/tiny requests still refuse. Existing API
res_rsf_scene returns 1 for a queued transition and 2 when its normalized plan
already matches the scene owner. No new function, vendor ABI or backend policy
was added. The scene adapter also compares the actual current images with
res_sizes before declaring convergence: an owner-side pending update is not
proof that the game already allocated it. Once current image dimensions equal
the normalized plan, the adapter passes those real descriptors/camera dimensions
to the existing provider evaluate API. A strict provider or an incompatible range
then fails explicitly rather than repeating an impossible optimal resize forever.
FSR4 split preparation is cancelled only on genuine pending/refused resolution,
not before an actual converged evaluation.

`native_xess_vulkan.cpp`, rsf_xessvk_create now retains the SDK-returned minimum
and maximum alongside the unchanged recommended plan. It allocates private input
images at that maximum, avoiding resource replacement while earlier Vulkan work
can still reference them. Image-view resource dimensions describe those true
allocations; execute inputWidth/inputHeight describe the actual active scene
rectangle. Copies cover only the actual input region. No engine grid/alignment
policy exists in the RSF service.

`rsf_xessvk_evaluate` validates actual record render dimensions inclusively against
both SDK bounds, requires matching camera render/output dimensions, and checks
colour/depth/low-resolution motion descriptors against the actual record extent.
It passes that actual extent to XeSS execution. Output remains the fixed requested
output size. Changing active input extent resets SDK history even if frame IDs
remain consecutive and the external allocation generation did not change. The
public recommended plan and all public ABI layouts are unchanged.

The optional frame diagnostic callback reports one line containing genuinely
queried recommendation/minimum/maximum and actual input dimensions. TotK supplies
that callback through its existing gfx/present trace filter. The numerical live
minimum/maximum were not collected by this worker and must not be replaced by
synthetic fixture values. The coordinator's next accepted run should preserve
that `XeSS input range ...` line to establish the actual device/SDK range.

## Focused CPU proof

One final focused batch built the complete real native Vulkan archive and XeSS
fixture with MSVC in build/worker-fsr4-sr-20261115. No shared output, staged DLL,
GPU/game/UI/device test, pin, staging, commit or remote action was touched.
The existing no-argument XeSS ABI contract and a new CPU input-range variant both
pass. The latter includes the actual service implementation with inert SDK and
Vulkan dispatch and tests:

- recommended plan stays 1130x636 while synthetic SDK bounds allow 1128x632;
- actual execution receives 1128x632, with truthful maximum private image extents;
- identical consecutive input preserves history and changed input resets it;
- minimum/maximum edges, zero dimensions, inconsistent depth/motion sizes and
  camera render/output disagreement refuse before any copy or SDK execute;
- native requirements ABI behavior remains available through the original fixture.

The synthetic bounds (1024x576 through 1280x720) are test data, not measured SDK
bounds for the user's machine. The actual creation path's stored range comes
solely from xessGetOptimalInputResolution and is never synthesized from those data.

The CPU variant is behind RSF_TEST_XESS_INPUT_RANGE in tests/native_xess_vulkan.cpp.
For this isolated batch, an ignored CMake top-level include deferred this target
until ReScaleFrame::NativeVulkan existed, avoiding tracked build-system changes:

```cmake
function(rsf_add_xess_range_worker)
    add_executable(rsf_xess_range_worker "${CMAKE_SOURCE_DIR}/tests/native_xess_vulkan.cpp")
    target_compile_definitions(rsf_xess_range_worker PRIVATE RSF_TEST_XESS_INPUT_RANGE RSF_HAVE_XESS_VK RSF_RUNTIME_BUILD)
    target_include_directories(rsf_xess_range_worker SYSTEM PRIVATE "${RSF_VULKAN_INCLUDE}" "${RSF_XESS_VK_INCLUDE}")
    target_link_libraries(rsf_xess_range_worker PRIVATE ReScaleFrame::NativeVulkan rsf_build_options)
    add_test(NAME worker_xess_range_contract COMMAND rsf_xess_range_worker)
endfunction()
cmake_language(DEFER CALL rsf_add_xess_range_worker)
```

It resides at build/worker-fsr4-sr-20261115/xess-range-worker.cmake. Commands from
RSF root (all exit zero):

```powershell
cmake -S . -B build/worker-fsr4-sr-20261115 -DCMAKE_PROJECT_TOP_LEVEL_INCLUDES=D:/Coding/ReScaleFrame/build/worker-fsr4-sr-20261115/xess-range-worker.cmake
cmake --build build/worker-fsr4-sr-20261115 --config Debug --target rsf_xess_range_worker rsf_native_xess_vulkan_test -j 4
ctest --test-dir build/worker-fsr4-sr-20261115 -C Debug -R '^(worker_xess_range_contract|native_xess_vulkan_contract)$' --output-on-failure
```

The same final batch ran TotK tests/rsf_reduced_quality_check.ps1 (three complete
UCRT64 Clang objects with -Wall -Werror, six CPU executables),
tests/rsf_split_record_check.py and tests/rsf_fg_resize_check.py. They pass with
updated actual scene tests for already-converged nearest size and explicit
provider refusal, plus the actual game-thread mailbox proving 1130x636 becomes
1128x632 once and does not continually rebuild. Known camera, split-fence,
accepted FG ownership and previous-jitter regressions remain intact.

## Publication and remaining acceptance

Owned RSF hunks: native_xess_vulkan.cpp adds retained bounds, maximum private input
allocations, actual extent/camera validation and SDK execution, active-size reset
and optional bounds facts; tests/native_xess_vulkan.cpp adds only the guarded CPU
range variant and its closing #endif; this new note. Both tracked C++ files already
had unrelated camera-ABI migration hunks at task start. They were preserved and
must not be attributed to or selectively published with this delta by accident.

Owned TotK hunks: resolution.c normalizes requests and reports already-current;
vk_dlaa.c handles convergence and forwards existing diagnostic callback;
tests/rsf_resolution_check.c, tests/rsf_reduced_quality_check.py and
tests/rsf_split_record_check.py update actual-function regression setup; the
existing reduced SR audit receives the correction below. No shared header or
new public ABI was changed.

The coordinator must publish/select these RSF hunks, update the external pin and
rebuild before its own XeSS-quality run can exercise range admission. Preserve
actual recommended/minimum/maximum/scene facts, accepted temporal dispatch and
FG preparation/tag/submission/present identity, and sustained generated output
without new validation errors. If the nearest engine extent is outside the actual
queried SDK range, explicit native failure remains correct. Other strict SR
providers are not silently relaxed or relabelled by this change. Their nearest
representable extent acceptance remains a separate provider-specific contract.

The coordinator wired the CPU input-range fixture into the existing CTest suite as
`native_xess_input_range_contract` when the Vulkan/XeSS headers are available.
This uses the existing test executable infrastructure and replaces reliance on
the ignored worker-only CMake include for subsequent verification.
