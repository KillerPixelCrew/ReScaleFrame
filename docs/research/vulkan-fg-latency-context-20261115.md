# Generic Vulkan FG latency context attachment and retirement

15 November 2026. TotK's XeSS FG source review found that native_latency_vulkan's presentation pointer had no production attachment caller. The core therefore selected Reflex instead of XeLL and could not produce the XeSS provider's required CPU sleep/marker facts. This correction is generic infrastructure; no game identity or adapter vendor choice is introduced.

## Ownership

`native_fg_vulkan_dispatch.cpp`, `rsf_fgvk_create_with_host` attaches the fully created/configured opaque wrapper to the shared internal presentation registry before returning it. Every FG provider follows that path. A binding conflict rolls the newly created provider back; failed cleanup returns its retained owner/chain under the existing ABI. Cleanup of an unattached partial wrapper cannot clear another active binding. No TotK host changes, exported entry points or ABI changes are needed.

New `native_latency_vulkan_presentation.h` implements a process-runtime binding and scoped operation leases. Acquiring a lease briefly locks the registry, snapshots the context and increments its borrow count. All core backend/status/options/sleep/marker accesses use that lease. Its lifetime holds no registry, SDK, presentation or queue mutex. In particular, CPU XeLL sleep and its immediate marker remain protected together while another thread can query SDK status or complete Present.

Exact-owner retirement marks the binding closing and blocks new borrows before checking existing borrowers. `rsf_fgvk_destroy` returns NOT_READY without calling provider destruction while a borrower remains; it never waits for a CPU sleep that could need the caller's host Present lock. After the borrower exits, a caller retries checked destruction; failure retains the closing binding and provider, success releases only that exact context. A late predecessor retirement/release cannot detach a successor. The wildcard legacy `rsf_latencyvk_presentation(nullptr)` is also nonblocking and must be retried after NOT_READY. Nonnull legacy attach refuses replacing a different active owner.

Generic `rsf_fgvk_drain` has a temporary borrow gate for the same reason: it refuses while a CPU user exists, blocks new borrows while invoking provider drain, and resumes borrowing after the drain unless explicit destroy/unbind retirement was requested. A refused drain leaves the gate paused until retry. Native WSI operations remain available for already-started CPU work to finish. Existing host serialization of WSI/lifecycle calls still applies; the registry protects the core's borrowed presentation accesses, not arbitrary external use of freed C ABI pointers.

Checked runtime shutdown takes a control lease only when no operation borrower exists. It blocks attachment/new operations and provider destruction during SDK shutdown, restores the live binding on checked SDK shutdown failure, and clears it on success. It does not drop another active owner's context before knowing shutdown succeeded.

## Policy and facts

The integration continues reporting engine source/input/render/Present facts. The core selects required XeLL from the leased provider identity. At each accepted input pacing boundary it applies required XeLL On and the current interval before sleeping, including when the same requested setting was previously applied while Reflex owned pacing. This avoids relying on a settings change or a forgotten external attach call. Existing marker/token deduplication remains intact and no Reflex sleep is stacked with XeLL. While retirement blocks acquisition, the core does not select an unrelated fallback pacer for that closing owner.

No CPU/Present source is borrowed from a latest-frame global. No SDK/provider API is called with an unprotected context from the old atomic load pattern.

## Changed paths and selective publication

Only these sibling paths belong to this task:

- runtime/orchestrator/src/native_latency_vulkan.cpp
- runtime/orchestrator/src/native_latency_vulkan_presentation.h (new)
- runtime/orchestrator/src/native_fg_vulkan_dispatch.cpp
- tests/native_latency_vulkan.cpp
- tests/native_fg_vulkan_dispatch.cpp
- docs/research/vulkan-fg-latency-context-20261115.md (new)

Scoped diffs against current main contain no preexisting camera ABI3 hunks in any of these four modified cpp files. The new helper uses only opaque provider pointers and existing ABI types; it does not depend on camera ABI3. The working tree's unrelated ABI3/header/provider edits were neither changed nor staged. Pinned external/ReScaleFrame and TotK runtime files were not edited. The core's previously published PCL epoch cookie/tag now carry maybe_unused for strict builds without the Streamline compile flag; behavior is unchanged.

## CPU validation

All outputs are isolated under TotK build/checks/rsf-presentation-worker. `commands.txt` records the full successful commands with absolute sources/includes. UCRT64 Clang++ C++20 builds use RSF_RUNTIME_BUILD and orchestrator/contract/game SDK includes from the sibling. No SDK/game/device/GPU/UI execution, shared build writes, staging or commits occurred.

- latency.exe compiled tests/native_latency_vulkan.cpp with native_latency_vulkan.cpp, latency_amd_vulkan.cpp and native_cpu.cpp, RSF_HAVE_STREAMLINE_VK=1 and -luser32; returns0. Existing PCL, epoch and source tests remain passing. New real native core calls overlap a deliberately blocked XeLL sleep with status, unbind, rebind and checked shutdown. They prove SDK mutex availability during sleep, NOT_READY without waiting, no new retirement borrows, context retention on failed shutdown, old-owner release isolation, and unchanged-mode Reflex-to-XeLL selection/options without Reflex sleep stacking.
- dispatch-base.exe, dispatch-xess.exe and dispatch-fsr4.exe compile the actual dispatch source through the existing provider fixture with no provider flag, RSF_HAVE_XESS_FG_VK=1 and RSF_HAVE_FSR4_FG_VK=1 respectively; all return0. XeSS adds a blocked actual dispatch sleep under the same scoped lease used by the core and calls actual drain/destroy concurrently. Both refuse before provider teardown, new borrows fail, and cleanup succeeds on retry. Attachment rollback and retained partial cleanup cannot clear an independently bound owner.
- Complete native_latency_vulkan.cpp passes -Wall -Wextra -Werror -Wno-missing-field-initializers -fsyntax-only both with and without Streamline. Complete native_fg_vulkan_dispatch.cpp passes the same strict check with both XeSS and FSR4 enabled.
- Scoped git diff --check passes. Initial new-fixture compilation lacked frame_generation.h for the pacing enum and the first strict no-Streamline check exposed unused epoch symbols; both were corrected before the complete successful rerun.

Remaining acceptance is coordinator full native verification, selective main publication/pin, TotK full build and fullscreen XeSS FG/latency capture proving actual provider sleep/CPU markers and generated pixels. CPU fixture success does not establish XeSS hardware admission, FPS improvement or measured latency.
