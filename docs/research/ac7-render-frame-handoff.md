# AC7 CPU-to-render frame handoff

30 September 2026. Static binary/source inspection and synthetic runtime tests. No new AC7
hook is installed by this work, and association through the RHI thread, final HUD-less/UI
export and Present remains unvalidated.

## Question and method

How can a source-frame identity survive queued rendering while the game thread advances?
The renderer object is the useful ownership boundary: Unreal copies view-family state into it
before queueing work, and the queued task keeps its pointer until rendering finishes. A global
latest frame ID or a Present counter cannot preserve that relationship.

Used the retained decrypted dump, original executable SHA-256
`c7da97f5f8a807d4f1264adbb074146fcffe9bdc2ffa98791b822cd28e558f4f`, dump SHA-256
`fafd1db2808d32333caecf4056e8bcd676c2f07e4c1fabc0f270ef29f22d24db`, and UE 4.18.3 source
`0a14a8d537a31ecc77488ced41dbaa0166612ef8`. Ghidra MCP explicitly selected
`Ace7Game.exe.dump`, x86-64 Windows, at base `0x7ff741350000`.

The motion workstream had already identified deferred rendering at RVA `0xef5cc0`. Traced its
vtable back to the deferred-renderer constructor, then inspected the callers and allocation
paths. Decompilation supplied structure/control-flow leads; decoded instructions confirmed
arguments, offsets and queued storage. `RenderViewFamily`, `FDrawSceneCommand`,
`BeginRenderingViewFamily` and `ViewExtensionPreDrawCommand` strings are absent. Their absence
did not stop the ownership trace.

## Matched native functions

| RVA | Researched behavior | Supporting evidence |
| --- | --- | --- |
| `0x111ac10` | FRendererModule::BeginRenderingViewFamily | Scene/world update, deferred updates, canvas flush, scene/family counter, renderer allocation, view-extension command and draw command |
| `0x111ec90` | FSceneRenderer::CreateSceneRenderer | Selects mobile/deferred constructor and allocation sizes `0x220`/`0x228`; same allocation path is inlined in BeginRenderingViewFamily |
| `0x1113080` | FSceneRenderer constructor | Copies input family into object `+0x10`, allocates/copies view-info entries, rewrites their Family pointer |
| `0x1113350` | FSceneViewFamily copy constructor | Copies view pointers and metadata, including the 32-bit frame number at `+0x68` |
| `0x11241b0` | Draw-scene task execution | Retrieves RHICmdList, loads captured renderer at task `+0x10`, calls render helper, flushes, then releases task storage |
| `0x112ff00` | RenderViewFamily_RenderThread | Updates resources, invokes renderer virtual Render, maintains scene caches, calls renderer retirement/deletion |
| `0x113a0c0` | WaitForTasksClearSnapshotsAndDeleteSceneRenderer | Waits for tasks, clears snapshots, handles async RHI dispatch and invokes deleting destructor |

These names describe source-matched behavior. The task name is a semantic label, not recovered
debug-symbol evidence of its exact template specialization. Ghidra's inferred extra arguments
in the render helper/task decompilations are not a calling-convention specification. At
`0x11241cc`, `48 8b 57 10` loads the captured renderer into RDX; `0x11241d0` puts RHICmdList
in RCX; `0x11241d3` calls `0x112ff00`. The helper saves those two pointers, then at
`0x112ffac` sets RDX to RHICmdList and at `0x112ffb2` sets RCX to the renderer before its
virtual call at `0x112ffbe`. This confirms the two relevant Windows x64 arguments.

The deferred renderer's vtable begins at RVA `0x2a892d8`, with Render at slot `+8` pointing
to `0xef5cc0`. Its constructor writes that vtable. This links the queued helper to the known
deferred-renderer/postprocess producer rather than only to a generic task system.

## Identity, layouts and lifetime

At `0x111adb2`, `89 46 68` writes the input family's native frame number. For a scene-backed
family the value comes from scene virtual slots `+0x270` (increment) and `+0x268` (get);
otherwise it comes from GFrameNumber. The constructor also refreshes the copied frame number
at renderer `+0x78`. Stock PostRenderAllViewports increments GFrameNumber separately. These
are 32-bit engine counters, not the orchestrator's 64-bit source-frame ID.

| Native field | Offset/size | Interpretation and limits |
| --- | --- | --- |
| Renderer.Scene | `+0x8`, pointer | Scene identity, not a source-frame counter |
| Renderer.ViewFamily | `+0x10`, embedded | Copy of the input family, owned until renderer deletion |
| Family.FrameNumber | `+0x68`, uint32 | Per-scene or global native sequence; copied value is renderer `+0x78` |
| Renderer.Views | `+0xb8`, pointer | Array of owned FViewInfo records |
| Renderer.Views count | `+0xc0`, int32 | Must be bounded/validated before traversing |
| FViewInfo stride | `0x27c0` bytes | Confirmed constructor/postprocess iteration; specific to this executable |
| FViewInfo.Family | `+0x0`, pointer | Rewritten to the renderer's embedded family |
| Draw task captured renderer | `+0x10`, pointer | Assigned before enqueue and consumed by task execution |

The original game-thread family and view pointers are copied; neither is a stable render-thread
ownership key. A plugin can bind its CPU source ID to the renderer during construction, while
the submitting frame is still known. Render-thread consumers resolve that key, and copy the
result into their own packet before the renderer retires. Main-view selection remains a
separate requirement: a source frame can create several families and views.

The retirement helper waits for task/snapshot use and deletes the renderer. It also optionally
waits for dispatch to the RHI thread. This does **not** prove that the GPU completed, that FG
inputs were copied before overwrite, or that Present belongs to this family. Any packet that
outlives the renderer needs copied metadata and separately leased GPU inputs. Do not dereference
the renderer from an asynchronous vendor callback or carry a render-thread TLS scope into an
RHI thread by assumption. The precise bind/release instrumentation still needs runtime proof,
including synchronous rendering, captures, multiple views, loading and allocator reuse.

## Reusable runtime implementation

`runtime/orchestrator/.../render_links` implements the bounded association mechanism. The game
plugin chooses the engine ownership key and when to bind/release it; the runtime copies
session ID, 64-bit source-frame ID, resource generation and its own submission ticket. Lookup
does not consult a latest-frame global. Multiple submissions in one source frame are distinct.
It refuses duplicate live keys, full capacity, stale generations and late releases from an
older ticket. A missed retirement refuses further binding instead of silently evicting a live
association. Destruction refuses pending links; callers must stop producers/consumers first.

This registry carries metadata only. It neither retains engine objects nor retires GPU leases,
selects the main view, emits input/simulation markers, tags a vendor SDK or assigns Present IDs.
It is independent of AC7, UE layouts and the selected SR/FG provider. The registry is built and
synthetic-tested; an AC7 adapter and public plugin lifecycle wiring remain to be connected.

`tests/render_links.cpp` checks an earlier queued frame consumed after the CPU advances, IDs
above UINT32_MAX, multiple submissions per frame, capacity/duplicate refusal, reused native keys
with stale release tickets, cross-session refusal, ABI checks and competing producers. Native
Release verification with VS2026 passed: 31 tests passed, five opt-in fixtures skipped. This is
Windows build/synthetic evidence, not an injected game or enabled-FG test.

## Recovery and evidence retention

Auto-analysis completed and the earlier six CPU names were successfully saved. The seven new
handoff names were also preflighted, applied, read back and saved through MCP. Their
[replay manifest](evidence/ac7-render-frame-handoff-20260930.json) records checked byte spans
and source rationale. Replay uses the tracked
[name tool](../../tools/ghidra/replay-research-names.py). Database and raw decompiler/disassembly
output remain untracked; local artifacts are under `.local/fg-cpu-research/ghidra/`.
