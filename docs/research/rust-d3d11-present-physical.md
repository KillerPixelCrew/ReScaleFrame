# Physical swapchain and queue ownership in Rust

This source audit starts at `5038f7e` and follows the [bridge cutover](rust-d3d11-present-cutover.md)
and [buffer cutover](rust-d3d11-present-buffers.md). It traces the creation queue and swapchain
from publication through replacement and retirement in the
[Rust owner](../../runtime/orchestrator/src/d3d11_present.rs) and
[native facade](../../runtime/presentation/src/d3d11_present_bridge.cpp).

| Path | Source evidence and ownership |
| --- | --- |
| Creation and fallback | `create_plain` returns its queried `IDXGISwapChain4` reference and the queue used for creation. For FG, `generation_setup` supplies Streamline's proxy queue for DLSS, the engine queue for native D3D12 FidelityFX/XeSS, and the native graphics queue for their D3D11 path. The provider creation calls use that supplied queue. Rust adopts the chain and retains the queue with `ComRef` before publishing the pair. Failed QI, a missing queue, and failed fallback release staged references. Lazy creation publishes the usable pair before requesting fullscreen. |
| Facade queries | `GetBuffer` still uses the Rust buffer batch in virtual-buffer modes. Strict native D3D12 forwards it on a leased chain. The facade uses a private lease for forwarded descriptors, outputs, statistics, private data, transforms, colour and HDR state, frame latency, backbuffer index, and window operations such as `ResizeTarget`. The three lookups that previously skipped initialization still only look up the current chain. |
| Present and submission | Present pins one pair across fullscreen and tearing checks, backbuffer acquisition, queue joins, list unwrapping, execution, fence signalling, Streamline common begin/Present/end, and SDK callbacks. The native leaf uses the same queue to choose proxy or native lists. The public queue and list lookups and the retire callback use the existing provider guard. The public queue is borrowed until the next drained provider transition. The public list lookup can still use the native-list path while creation is pending. |
| Resize, fullscreen and switch | Resize and fullscreen keep their pair through drain and the typed DXGI call. The switch drains the interop and graphics queues and joins vendor retirement before replacement. The adopted graphics queue's native interface comes from that same queue, while native D3D12 drains the engine queue through its interop slot. A live physical lease defers replacement to a later Present. Otherwise Rust withdraws both references before leaving outgoing fullscreen and destroying the provider. Reentrant physical lookups during withdrawal return a transient refusal. Fullscreen restoration can see the new pair. |
| Teardown | Owner unpublication and drain use the existing transition guard. Successful retirement releases the pair after the shared presentation fence and before provider and host teardown. Failed drain deliberately retains both references with the provider and GPU-visible inputs. |

At the baseline, a copied DXGI chain reference could outlive `clear_physical` while a switch tried
to create another chain for the same HWND. Queue reads were independent borrowed pointers, and
submission could look up the queue again after unwrapping the list. The Rust lease keeps both
references together and defers HWND replacement while either is in use. `DXGI_PRESENT_TEST` and
forwarded DXGI calls previously held `physical_guard` across the native call. The new mutex only
copies or detaches a lease, so native calls and final COM releases can reenter. A failed Streamline
list unwrap now faults the frame before prepare or submission and still completes its notification.

## Verification

Synthetic leaves verify chain and queue identity through Present and retirement callbacks, reentrant
lookups during test Present and fullscreen, switch deferral and retry, partial creation rollback,
strict native `GetBuffer`, null list refusal, and retained references after failed drain.

- Rust 1.94.1 passed 379 locked workspace tests (236 in the orchestrator), formatting and Clippy
  with warnings denied.
- The baseline and candidate used independent Visual Studio 2026 Release build and Cargo target
  directories with Streamline enabled. Both copies of yyjson matched the three locked hashes.
  The PE comparison matched case-sensitive export names, ordinals, forwarders and imports in
  Runtime (381 exports), AC7 (256), Loader and version alias (273 each), thin carrier (6),
  Minecraft (267), Unity Native (275), and Godot Native (398). All 33 public linked presentation
  symbols matched in the native static archive. The public presentation, Runtime and game SDK
  headers matched byte for byte.
- All 23 focused MSVC CPU and ABI contracts passed, including the C++ and C11 presentation and
  FG DLL contracts and the embedded renderer and FG12 contracts. The D3D11 and shared FG bridge,
  shared surface, provider, and provider-switching fixtures linked successfully.

Real DXGI fullscreen and resize, device queue ordering, vendor SDK execution, and gameplay remain
unverified. The device fixtures and full release suite were outside this run.

## Merged Standard game retest

The isolated Standard clone ran at `35f99ca` with the rebuilt Rust adapter
and Runtime. The known executable hash matched the Standard profile.
The actor callback requested a windowed resize from 1280×720 to 1600×900
and then queued an engine quit. The Runtime reported one successful resize
and 689 DLSS SR evaluations with zero refusals at the last full status
sample. The engine destroyed Direct3D and returned exit code 0. The staged
DLLs and settings were restored and checked against their backup hashes.

Windows kept the window out of the foreground. The activation attempt
reported that the foreground window had no process ID. DLSS-G stayed
inactive and the run supplies no foreground input, generated-frame or
fullscreen evidence. Streamline printed the same native swapchain
reference warning on release as the preceding presentation-buffer run.
Logs, script, sizes and payload hashes are under
`.local/anomaly/staging/live-58fe95a-stage5/evidence/merged-physical-standard-35f99ca/`.
