# Generation ownership and lease port

C04 moves the D3D11 FG table, dispatch, scene COM lookup, provider lookup, and
lease COM operations to Rust. The AC7 carrier still calls the same embedded
dispatch symbols. The Anomaly adapter still obtains the DLL's version-4 FG
table. The table's argument order, 120-byte x64 layout, refusal order, and
callback targets are preserved. The public capture table validates normalized version-1 depth and motion and derives the frame extent from the pass. The new lease teardown is an additive export.
The GameSDK records and C05 CPU dispatch contract keep their existing layouts.

## Findings

1. Scoped capture enters the scene owner's restoration guard. It uses the
   same binding and explicit refusal path as the ordinary scene setter.
2. Session switching and lease sealing use one retirement descriptor parser.
   It refuses the reserved fence target before polling. Both use the same
   fence-completion classification, including the device-removal sentinel.
3. Provider marker failure invalidates the ledger entry at the accepted
   marker. Prepare and finalization cannot reuse the failed entry. Abort
   releases the sequence through the existing provider path.
4. Shared SR evaluates in a separate transfer slot when an explicit FG lease
   already exists for the source. Its depth, motion, commands and completion
   fence are separate from the explicit FG slot. An SR result does not update
   that slot's provenance, readiness or HUD-less state.
5. Input-derived timing uses consecutive verified source receipts on the same
   presentation thread and session, chain, provider generation, viewport and
   view. It rejects frequency changes, unsigned wrap, backward time, large
   gaps and missing receipts. Repeated Prepare for the same receipt reuses
   the measured delta. When neither the engine nor an admitted measurement
   supplies a positive delta, configuration declines interpolation.
6. A separate lease-owner operation consumes retained references after the
   caller verifies device loss and tears down GPU work. It does not turn the
   failed retirement into success. The fault fixture keeps the failed fence
   at the removal sentinel through teardown.
7. Required shared callback slots use typed storage at admission or linked
   construction. The owner-by-owner audit records the overlay handoff.
   [Shared callback admission](#shared-callback-admission)
8. FG Prepare and the native window observer classify the scene already
   returned by the registry. The public standalone scene and classification
   queries preserve their existing contracts.

## Shared callback admission

The audit starts at `e2c1a8c2`. It follows each table from its initializer or
incoming pointer through the owner that dispatches the callbacks.

| Named owner | Source evidence |
| --- | --- |
| FG session | Already typed at the baseline. `Session` holds `Host` and `Context` holds `Provider`. Create and switch call the respective checked constructors before retaining or creating state. [Storage](../../runtime/orchestrator/src/fg_session.rs#L24), [validation](../../runtime/orchestrator/src/fg_session.rs#L148), [create](../../runtime/orchestrator/src/fg_session.rs#L314), [switch](../../runtime/orchestrator/src/fg_session.rs#L354) |
| FG12 | The linked Rust initializer now requires every callback by type. The existing parameterless bind publishes that complete table, and `native!` invokes plain pointers in FG12 and its color owner. The private C declaration and generated record keep their layout. [Bind and dispatch](../../runtime/orchestrator/src/fg12.rs#L93), [initializer](../../runtime/orchestrator/src/fg12/native.rs#L90) |
| DX12 Vulkan interop | The interop owner validates all 16 platform leaves before querying or opening the interop route and holds them by value. The legacy constructor also extracts its required image and fence callbacks before use. Its stored image factory is present only on that route. FSR4 SR checks its incoming interop table before opening the interop route and retains the existing graphics cleanup on refusal. [Platform validation](../../runtime/orchestrator/src/dx12_vulkan_interop.rs#L66), [legacy admission](../../runtime/orchestrator/src/dx12_vulkan_interop.rs#L348), [FSR4 SR admission](../../runtime/orchestrator/src/vulkan_fsr4_sr.rs#L525) |
| Auxiliary SR | Both the C create entry and the D3D11 reduced-products factory use the fallible constructor. It checks all 13 leaves as required by the private header. An incomplete table yields a null handle or absent products. The failed-backup fixture supplies explicit unreachable leaves for operations outside its reservation path. [Validation](../../runtime/orchestrator/src/auxiliary_sr.rs#L276), [C create](../../runtime/orchestrator/src/auxiliary_sr.rs#L1003), [host factory](../../runtime/orchestrator/src/d3d11_host_reduced.rs#L55) |
| Frame tap | Rust supplies its linked leaves and checks the nine foreign exception-containment leaves before publishing the typed table. Install rejects an incomplete platform before patching the vtable. The public options still carry optional observer and decision callbacks, and the C trampolines still guard their invocation. [Platform admission](../../runtime/graphics/rust/src/frame_tap/platform.rs#L13), [install](../../runtime/graphics/rust/src/frame_tap/detours.rs#L474), [C trampolines](../../runtime/graphics/src/frame_tap_callbacks.c#L48) |
| Native FG | Graphics already used a linked plain-function table at the baseline. Its CPU reference now points to the typed CPU table as well. [Graphics storage](../../runtime/orchestrator/src/native_fg.rs#L67), [linked initializer](../../runtime/orchestrator/src/native_fg/platform.rs#L75) |
| Native FG CPU | Production calls use the linked table of 22 required leaves, with optional window-thread and wait overrides retaining their Win32 fallbacks. The ctest fixture builds its own Rust table from the linked one with recording leaves and calls the typed handlers directly. [Linked table](../../runtime/orchestrator/src/native_fg_cpu/platform.rs#L40), [fixture](../../runtime/orchestrator/src/native_fg_cpu/fixture.rs#L1) |
| Shader patch | Production and fixture platform tables now require their linked callbacks by type. The public API record, compile and reflection sink records, and fixture callbacks retain their contracts. [Platform type](../../runtime/orchestrator/src/shader_patch/leaf.rs#L57), [production table](../../runtime/orchestrator/src/shader_patch/platform.rs#L333), [fixture table](../../runtime/orchestrator/src/shader_patch/tests.rs#L182) |
| OverlayHost | The D3D11 host uses required typed pointers, and the loaded panel API admits required exports before any draw. See [overlay dispatch coverage](overlay-typed-dispatch.md). |
| Overlay service | Runtime telemetry is called directly. Only optional adapter callbacks and the fixture status override remain nullable. See [overlay dispatch coverage](overlay-typed-dispatch.md). |
| Overlay native12 | The native and wrapped owners use required typed tables. Raw C records are validated at their legacy entry points. See [overlay dispatch coverage](overlay-typed-dispatch.md). |

Rustfmt from Rust 1.94.1 checked the touched Rust files. Bindgen 0.72.1
regenerated all 154 binding files byte-for-byte, and Cargo 1.94.1 locked offline
metadata passed. Build, fixture, and device validation remain pending.

## Port inventory

The declaration-only paths under runtime/orchestrator/include/rescaleframe are
d3d11_fg_api.h, fg_choice.h, fg_leases.h, fg_session.h, frame_assembly.h,
frame_sequencer.h, gpu_policy.h, native_fg.h and native_scene.h. The private
headers runtime/orchestrator/src/fg_auto_choice.h and
runtime/orchestrator/src/native_fg_platform.h are also declarations.

The removed native implementations under runtime/orchestrator/src are
d3d11_fg_api.cpp, fg_leases_com.cpp, fg_providers.cpp, native_fg_dispatch.cpp and
native_scene_platform.cpp. The fixtures tests/fg_choice.cpp,
tests/frame_assembly.cpp and tests/gpu_policy.cpp moved to tests/generation.
The choice fixture still calls the C inline SDK reference for default FG
preference. SR policy retains both DLL and embedded coverage. Assembly still
calls the DLL, and its motion-sign check now exercises the Rust runtime helper.

The follow-up removes runtime/orchestrator/src/native_fg_platform.cpp and
runtime/orchestrator/src/fg_session_platform.cpp. The graphics table now has
required Rust function types at construction. The platform module keeps the
existing shared-surface service, paired command allocation, closed initial
command list, texture layout, full-resource barrier order, 10-second fence wait,
and exact C calls to the presentation bridge and frame dump services. Failed
COM outputs retain an owner until release. The session's C ABI entry and fence
query are Rust. The small runtime/orchestrator/src/fg_frame_policy.c leaf uses
the GameSDK's original inline admission predicates, which native backends also
use. The follow-up also removes runtime/orchestrator/src/native_fg.cpp. Its
Rust platform keeps the native CPU callback table layout for the C05 fixture,
preserves the direct presentation-bridge operations and uses the same typed
DXGI hardware lookup. The Anti-Lag SDK factory remains a native leaf. The
CPU dispatcher, graphics and latency exports retain their existing signatures.
Scene and window matching share the same IUnknown identity query. The query
and scene GetBuffer operations release any non-null failure output and retain
only copied identity values in the registry.

- tests/d3d11_fg_api.cpp moved to tests/generation/src/dll.rs. The C entry
  imports only the Runtime DLL and supplies canonical DLL callback addresses
  for the typed identity checks. The separate C11 import fixture still
  compiles against the public header. The Rust fixture preserves refusal bytes,
  oversized table tails, CPU ordering and reservation coverage, malformed
  capture refusal and separate presentation table ownership.
- tests/fg_session.cpp moved to tests/generation/src/session.rs. The mock
  provider and host still drive the public DLL session and sequencer exports.
  The fixture retains re-entrant quiesce and resume, provider rejection before
  and after Prepare, reserved and failed retirement, plain fallback, rendered
  source marker handling and slot reuse at the u64 boundary.
- tests/fg_leases_faults.cpp and tests/fg_leases.cpp moved to
  tests/generation/src/leases.rs. The synthetic COM objects use slots derived
  from the Windows bindings and cover removal on either fence, successful
  collection of a different slot, and terminal teardown while removal persists.
  The WARP case keeps the device-unavailable skip, real resource retention and
  independent CPU-signaled queue and provider fences.
- tests/fg_provider.cpp moved to tests/generation/src/provider.rs. The fixture
  keeps opt-in argument parsing, physical adapter admission, the early
  Streamline host path, D3D12 debug messages, a hidden DXGI window, real
  backbuffer and disabled Present, queue drain and provider retirement. A
  removed fence or refused retirement keeps resources alive through process
  exit.
- tests/fg_switching.cpp moved to tests/generation/src/switching.rs. The
  public session callbacks still coordinate fence quiesce, borrowed provider
  chains and an owned plain chain. The fixture presents on each FSR3, XeFG
  and second FSR3 chain before restoring plain DXGI presentation. It
  preserves the opt-in arguments, unsupported-provider skip and CTest target.

Source verification covers pinned Rust formatting, regeneration of the changed
bindings with bindgen 0.72.1, locked offline Cargo metadata, and the C entry
point and CMake source lists. The fixture follow-up compared public session
and sequencer call counts with the C++ baseline, matched generated fixture
record sizes against the Runtime bindings, checked the import-only DLL link
and C11 companion, and checked real window, backbuffer, Present and fence
operations alongside the original CTest names, timeouts and skip codes.
Build, test, lint, DLL export inspection and device execution are pending.
