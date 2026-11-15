# Vulkan FSR4 SR submission leases, 15 November 2026

## Question and source evidence

Can a Vulkan renderer use real FSR4 without disguising FSR3 as FSR4, while
preserving its own recording, submission, resource and resize ownership?
This is a source review and CPU contract check, not a device or game result.
The reviewed RSF HEAD was caa5201bef5685dbb7a02cf350410a14317adf39;
other workers had concurrent changes in the shared checkout.
TotK integration and shader coverage evidence is in its `docs/rsf-integration.md`
and `docs/rsf-shader-audit.md`. That audit's object, deformation, translucency,
particle and auxiliary-pass gaps remain open and are not repaired by this service.

The existing `native_sr_vulkan.cpp`, `rsf_srvk_create` deliberately excludes FSR4:
a single borrowed recording buffer cannot express the Vulkan/DX12 dependency.
The real route already exists in `native_sr_fsr4_vulkan.cpp`,
`rsf_fsr4vk_evaluate`, exposed by `native_sr_split_vulkan.h`. It copies RGBA16F
colour, R32F depth and RG16F motion to private D3D12-shared images, releases them
from the actual Vulkan queue family, and records a graph split. A permanently
imported D3D12 fence timeline orders producer ready, DX12 evaluation and Vulkan
continuation. Shared output is RGBA16F at the output extent. Inputs are full
level-zero images; motion scale and jitter are forwarded from the frame contract,
with no game-specific conversion hidden in the service.

`runtime/vulkan/fsr4/bridge.cpp`, `CreateContext` matches the Vulkan adapter LUID,
consumes the existing pinned FidelityFX headers and signed DX12 provider, forces
the newest available 4.x provider with ffxOverrideVersion and checks the effective
provider identity. NVIDIA/Intel use the existing guarded INT8 compatibility lease.
A provider refusal remains a failure. This work does not introduce an FSR3 fallback
or change provider selection. `FinishHandoff` orders ready/done even for SDK failure,
so the returned continuation remains the host's fallback location.

## Corrections

The service previously relied entirely on the caller's promise that both recorded
sections were submitted. Destruction could wait ten seconds on a DX12 queue whose
Vulkan producer had never been submitted, and a subsequent evaluation could reuse
shared inputs without any admission guard. The same-layout tracked handoff ABI 2
adds explicit `rsf_srvk_fsr4_submitted` notifications. Its small SplitLease owner is
used by the real service and CPU-tested directly. It retains the producer before
calling split_record and the continuation after a successful split. Both must be
reported in order after actual successful queue submission; unrelated, duplicate,
null and out-of-order notifications are rejected. Failed submission retains the
lease for retry. A pending lease immediately refuses evaluation and destruction
with native VK_NOT_READY. Submission is not GPU completion: checked destruction
still waits for the shared timeline, Vulkan device and DX12 owner before releasing
images, memory, module and compatibility leases. Terminal split failures retain
the owner as before; notification does not make a broken graph recoverable.

The output was also sharpened before the EXTERNAL-to-host acquisition barrier.
The service now acquires GENERAL ownership before calling the sharpening helper,
then transitions to transfer source, copies to the host output and releases back
to EXTERNAL. First evaluation and the first success after SDK failure reset history.

Native SR ABI 2 and every public struct layout are unchanged. Split ABI 1 remains
accepted with its original caller-managed submission behavior. New integrations
must explicitly request RSF_SRVK_SPLIT_TRACKED_ABI_VERSION (2). This is an additive
export and opt-in contract, not an incompatible reinterpretation of ABI 1.

## TotK adapter work for the coordinator

No TotK runtime file was edited by this worker. Its `runtime/rsf_sr.c`,
`rsf_sr_open` currently always binds the ordinary single-buffer service, so selecting
backend 4 there still refuses honestly until the adapter is wired as follows:

1. Bind the six rsf_srvk_fsr4_* exports, including submitted, from the same runtime
   module lease. Keep a distinct opaque FSR4 owner, never cast it to rsf_srvk.
   Use the split header directly from the sibling SDK checkout. Create with the
   actual graphics command-pool queue family and enabled timeline capability.
2. Enable VK_KHR_external_memory_win32 and VK_KHR_external_semaphore_win32 at device
   creation after checking support; preserve the existing feature chain and timeline
   feature. The service checks importable D3D12 image/fence support and adapter LUID.
3. Extend recording to return the continuation to `vk_dlaa.c`, `rsf_scene`, and every
   enclosing recorder that holds the current command buffer. split_record must
   allocate/begin a different host-owned buffer, end the producer and attach submit
   metadata without submitting from inside evaluation. Preserve render/debug state
   across the split and invalidate cached state that belongs to the previous buffer.
4. Submit the producer with previous_done wait (when nonzero), ready signal, then
   submit the continuation with done wait, all at ALL_COMMANDS on the same host queue.
   Keep ordinary acquire/transfer/binary waits and the final host completion fence;
   binary semaphore values are zero in VkTimelineSemaphoreSubmitInfo arrays.
   Producer and continuation must be distinct VkSubmitInfo sections. Submit frame N
   before evaluating N+1. Do not batch sections into one wait-before-producer submit.
5. Notify each section through submitted only after its real submit succeeds and
   outside the queue lock. Keep image/command/session leases until the final GPU
   fence completes. Retire command resources only after GPU completion and reset.
6. Even on SDK refusal, use the returned continuation for spatial fallback, submit
   both sections, preserve the original error, and report the effective provider
   honestly. NULL continuation is terminal: do not reuse either graph section.
   Native VK_NOT_READY is opaque positive 1, not RSF_BACKEND_ERROR_NOT_READY.
7. Resize/quality/backend changes must submit pending sections, drain all GPU uses
   and use checked destroy before replacing the owner. Preserve failed cleanup
   and device/module leases for retry. Query the new plan before allocating guides.

Until these recording and submit changes exist, keeping FSR4 unavailable is correct;
changing only the selected backend number does not implement the route.

## CPU verification

All outputs are isolated in `build/worker-fsr4-sr-20261115`; no shared build output,
game install, device test, GPU or UI was used. Configure succeeded:

```powershell
cmake -S . -B build/worker-fsr4-sr-20261115 -G 'Visual Studio 18 2026' -A x64 -DBUILD_TESTING=ON
cmake --build build/worker-fsr4-sr-20261115 --config Debug --target rsf_native_split_stub_test rsf_native_split_disabled_test rsf_native_split_c_header rsf_native_sr_fsr4_vulkan_test -j 4
ctest --test-dir build/worker-fsr4-sr-20261115 -C Debug -R '^(native_sr_split_vulkan_(stub|disabled)|native_sr_fsr4_vulkan_contract)$' --output-on-failure
```

The build succeeded with MSVC 19.52, including the real native Vulkan archive,
FSR4 interop source and bridge. All three focused CPU tests passed. Tests cover the
actual lease helper, C header/layout, legacy ABI, tracked version admission, callback
forwarding, refusal, errors and exception containment. The no-argument FSR4 fixture
performs CPU argument checks only. Its device branch now requests tracked ABI 2 and
checks pre-submit destruction refusal and ordered notification, but was not run.

An initial production-DLL fixture build failed because concurrent `native_sr.cpp`
used RSF_ERROR_* constants without including game_api.h. The coordinator explicitly
authorized adding only `#include <rescaleframe/game_api.h>` near native_sr.h; this
one-line dependency repair is separate from this worker's owned changes and must
be staged as its own hunk without absorbing the concurrent SR implementation.

The coordinator owns eng/verify.ps1 and device acceptance; this worker did not run
that script because it uses the occupied shared build and executes device tests.
The two recorded Anomaly gate failures remain baseline failures, not waived new
failures. Required device checks: true FSR4 effective provider, synchronization
validation, every sharpening mode, SDK-failure fallback, failed submit retention,
resize/quality/backend transitions, partial-owner cleanup and repeated teardown.
TotK moving-scene SR quality and its shader audit remain separate acceptance work.

After the authorized include repair, the production DLL and its split/import and
export fixtures also compiled successfully (exit 0):

```powershell
cmake --build build/worker-fsr4-sr-20261115 --config Debug --target rsf_native_split_test rsf_native_vulkan_exports_test -j 4
ctest --test-dir build/worker-fsr4-sr-20261115 -C Debug -R '^(native_vulkan_dll_exports|native_sr_split_vulkan_contract|native_sr_split_vulkan_(stub|disabled)|native_sr_fsr4_vulkan_contract)$' --output-on-failure
```

All five CPU tests passed, including the actual DLL's six FSR4 exports and
production ABI admission. No fixture received a --device argument. A final
scoped git diff --check passed; no staging, commit or checkout was performed.

Owned edited files: runtime/orchestrator/include/rescaleframe/native_sr_split_vulkan.h;
runtime/orchestrator/src/native_sr_fsr4_vulkan.cpp;
runtime/orchestrator/src/native_sr_fsr4_vulkan.h;
runtime/orchestrator/src/native_sr_split_vulkan.cpp;
runtime/orchestrator/src/native_sr_split_lease.h (new);
tests/native_sr_fsr4_vulkan.cpp; tests/native_sr_split_vulkan.c;
tests/native_sr_split_vulkan.cpp; tests/native_vulkan_exports.cpp; this note (new).
The coordinator-authorized one-line game_api.h include in native_sr.cpp is the
only additional edit. All other dirty work belongs to other workers.
