# Shared D3D11 frame tap Rust cutover

The baseline is `44923c7` on `rust/orchestrator-anomaly`. The question was whether the shared
frame tap behind `frame_tap.h` could move to Rust without changing its public header, its exported
`rsf_frame_tap_*` entries or what its callers observe. This was a source port checked by review,
`cargo fmt` and bindgen 0.72.1. Nothing was built, and no fixture, game or device ran.

## Callers and surface

- `loader/proxy/src/dlss_bridge.c`, linked into the AC7 carrier, calls install, uninstall,
  refresh, end frame, plan, watches, candidates, divert, constant watch and override, bound target
  and status.
- `games/ac7/src/motion_capture.cpp` sets the research callbacks, and
  `runtime/orchestrator/src/native_translucency.cpp` sets the sampler bias.
- `scene_promote.h` and `depth_replay.h` use the public plan and geometry types only.
- `tests/frame_tap.cpp`, `tests/constant_override.cpp`, `tests/scene_promote.cpp` and the AC7
  capture tests exercise the public entries through `ReScaleFrame::Graphics`.

The tap is not exported from the Runtime DLL. `rsf_graphics` links `rsf_orchestrator_rust`, so the
embedded and DLL consumers reach the same Rust owner through the unchanged C++ exports.

## Ownership after the move

| Concern | Owner |
| --- | --- |
| Hook admission and retirement | `frame_tap.rs` keeps the vtable, the sentinel and each hook's slot, replacement and original. Install patches in the old order and rolls back as a unit; uninstall restores every slot and keeps the originals for calls in flight. |
| Runtime vtable rewrites | Every entry checks slot 13 against its replacement and re-applies the table, recording the runtime's value as the original before replacing it. |
| Callback and plan state | Options and role rules are copied at install. The plan is published with `plan_active`; gates, divert setup, candidate sets, watches and research callbacks are atomics. |
| Gating and substitution | Gate opening before a single-target binding is forwarded, one gate callback per binding, shader and render view substitution, depth policy, viewport and scissor scaling, clear and copy redirection. |
| Classification and capture routing | Roles come from `resource_roles.rs` (`matching_rule`) over the copied rule table. The shader resource shadow, the edge-triggered reconstruction set, near-miss logging, watch budgets, input and candidate reports, research before and after phases, geometry reports, divert verdicts, constant override and constant watch. |
| Resource and in-flight ownership | `ComRef` owns slot textures, the bound target, the view constants (under the lock), saved divert state, patched blends and pinned samplers with their clones. A thread-local depth keeps callback work out of the shadow. |
| Native leaves | `frame_tap.cpp` keeps the typed detours, `rsf::patch_slot`, every D3D11 query and call, the blend alpha patch and the sampler clone filter. Callback invokers keep C++ exceptions on the native side. |

`frame_tap_platform.h` is private to `rsf_graphics`, and `frame_tap_abi.rs` is generated from it
by the new recipe in `tools/generate-rust-bindings.py`. No public layout or export changed. No game
or engine policy entered the runtime.

## Preserved order

- Each binding hook forwards the game's call first, unless the plan substitutes. Substitution and
  gate opening run under their own reentry guard before the forward, and the shadow is updated
  after it.
- Draw: shadow sync, divert or constant override, the before-phase research report and a refresh,
  the game's draw read from a fresh original, a refresh, divert and override restore, then the
  geometry report, the reconstruction set and the after-phase reports.
- Map records a pending upload after the forward. Unmap hands it to the watch before the forward.
  UpdateSubresource hands it over before the forward, on a copy unless the watch only reads.

## Source-level behaviour changes

- A callback that throws no longer unwinds through the hook. A gate counts as declined, a verdict
  as leave and an override as binding nothing; other callbacks return.
- The vtable original is recorded before the slot is replaced, also when a refresh fails to
  re-patch. Previously a failed refresh kept the older original.
- The instanced draw detours return on a null original instead of calling it. This is unreachable
  after install.
- State written by other threads (gates, the substituted flag, the divert layer and verdict,
  candidate entries, the input watch, the bound target) is atomic.
- 64-bit diverted, candidate, refused and patched-blend counters are now 32-bit. The status always
  reported their low 32 bits.

## Plan publication and retirement

The first cut cleared `plan_active` and overwrote the plan with no wait, so a hook on another
thread that had already read the flag could read a plan being overwritten or forward a substitute
the caller then released, and uninstall cleared the render-thread shadow while admitted hooks could
still borrow it. The follow-up after merging `744be82` uses the counted-call pattern of
`overlay_cursor_hooks.rs` and the observer's Present calls:

- Hooks on the observed context take an admission: a SeqCst count raised before `installed` and
  the writer gate are read. set_plan and uninstall close the gate, which holds new hooks at entry,
  and drain the admitted ones before their first change, so one side always sees the other and the
  drain cannot be outrun by new hooks. Nested calls of an admitted hook pass a closed gate.
- The admission spans the game's forwarded call. Where the detour forwards a Rust decision (clear,
  copy, samplers), the entry hands its admission to the detour's `leave`.
- Same-thread reentry stays possible. Readers copy what they need of the plan instead of holding it
  across a callback; a gate callback that replaces the plan leaves that binding unsubstituted; an
  uninstall from inside a callback retires the shadow when the outermost admission ends, and
  install refuses until it has.
- install, uninstall and set_plan serialise on a lock hooks never take. The view-constant lock is
  never held while waiting.
- A control call from inside a tap callback never waits. It takes the lock only if free and
  proceeds only if no other thread is admitted; otherwise set_plan and uninstall return
  `RSF_FRAME_TAP_ERROR_INVALID_ARGUMENT` and install `RSF_FRAME_TAP_ERROR_ALREADY_INSTALLED` (an
  admitted caller's session is installed or still retiring). Validation, the lock, the installed
  check and the drain all precede the first change, so a refusal keeps the old plan and session.
  `frame_tap.h` states this for set_plan and uninstall.

The bridge (`loader/proxy/src/dlss_bridge.c`) calls set and clear plan (`apply_tail`,
`stop_reinsert`, `watch_for_stalled_plan`) and uninstall (`drain_shutdown`) from its Present
callback, between context calls on the render thread, and its `on_gate` saves and restores state
around `rsf_promote_finish` without touching the plan. That path never waits.

## Limits

- Source review, `cargo fmt` and bindgen 0.72.1 only. The user owns building and every test: the
  frame tap, constant override, scene promotion and AC7 capture fixtures, and a Windows game run
  for the vtable refresh.
- The drain and the closed gate spin with `yield_now`; hooks on other threads stall at entry while
  a writer drains. A hook entering on another thread at the instant an admitted caller closes the
  gate can make that caller refuse.
- set_divert, set_candidates, the watches and the constant callbacks keep their atomic publication
  without a wait. A divert layer cleared mid-draw on another thread is still the caller's to keep
  alive, as `frame_tap.h` states.
- Each observed-context hook pays two SeqCst read-modify-writes.