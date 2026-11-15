# Vulkan PCL registered-thread message delivery, 15 November 2026

The TotK input-delivery audit found that VulkanStorySlBindPcl registers its calling thread, which need not own a window or run a message pump. The generic native_latency_vulkan path now records successful registration identity, safely retries registration when accepted input boundaries change threads, and removes only its SDK-owned registered thread message ID. No game-specific switch or adapter-side vendor policy was added. Full TotK producer evidence is in docs/rsf-input-delivery-20261115.md in the TotK integration repository.

The pump runs only after accepted version1 keyed FRAME_BEGIN/INPUT_SAMPLE dispatch, under the SDK mutex while inspecting/removing at most16 messages, then releases the mutex before native_cpu_dispatch. It uses HWND(-1), exact registered ID bounds, actual QPC/frequency and source0 WINDOW_MESSAGE|INPUT_UNFRAMED facts. Keeping INPUT_UNFRAMED preserves the real removal timestamp rather than the historical WINDOW_MESSAGE-only normalization to FRAME_BEGIN. Dispatch after the boundary leaves those later facts for following-boundary association under the existing bounded core rules. Marker deduplication and token ownership remain unchanged.

PM_NOREMOVE inspection prevents consuming WM_QUIT, which bypasses PeekMessage filters; a quit arriving between peek/removal is restored with its exit code. Unrelated thread IDs and same-ID window messages remain queued. Registration failure is reported, retains the prior successful registration, and cannot pump a new thread queue; invalid registered IDs refuse binding. Successful shutdown/reinitialization clears thread identity. Old messages on another thread remain that thread's responsibility; their teardown/reinitialization generation has not been device/game validated.

Validation: isolated UCRT64 Clang++ C++20 executable compiled tests/native_latency_vulkan.cpp, runtime/orchestrator/src/native_latency_vulkan.cpp, latency_amd_vulkan.cpp and native_cpu.cpp with -DRSF_RUNTIME_BUILD -DRSF_HAVE_STREAMLINE_VK=1 and the orchestrator/contract/game include directories, linked -luser32, output to TotK build/checks/rsf-input-worker/core-latency.exe. It returns0 for existing ownership tests and new real-thread-queue cases: first bind, next-boundary ping, failed/successful thread rebind, exact ID and window filtering, flood bound, duplicate markers, WM_QUIT and shutdown/reinit. One invisible message-only STATIC window verifies window-message preservation; no game/GPU/visible UI was used. Strict changed-source syntax compilation also passes. Coordinator owns full native verification, review, commit and pin; no unrelated sibling changes were touched or staged.


## Review correction: registration epochs and pacing order

The first version pumped only after a boundary, so a changed input thread could
emit that boundary's markers before successful registration. The accepted
native_cpu sink now binds the actual keyed FRAME_BEGIN/INPUT_SAMPLE thread before
pacing, tokens or SDK markers. Malformed facts, input-only delivery and rendering
do not redirect registration. A failed rebind refuses that boundary's SDK work.

A successful registration posts a private exact-ID FIFO queue fence containing
a process-monotonic cookie and private tag. The bounded pump discards only its
own queued pings ahead of that epoch and consumes the fence without reporting it
as a tool ping. This addresses the earlier same-thread teardown/reinitialization
and rebind-away/back queue caveat without backdating dequeue timestamps or
removing unrelated IDs, window messages or WM_QUIT.

The extended real-queue CPU fixture passes as
`build/checks/rsf-input-worker/core-latency-epoch.exe` in the TotK workspace.
It proves stale pre-shutdown and pre-rebind pings do not produce new-source
markers, fresh pings still do, and SDK marker/sleep stubs observe the correct
registration thread already active. Earlier filter, flood, deduplication and
shutdown-refusal cases remain passing; strict changed-source compilation passes.
These are CPU and stub-SDK checks. Full native rebuild and actual gameplay
latency counters remain coordinator acceptance work for this revision.
