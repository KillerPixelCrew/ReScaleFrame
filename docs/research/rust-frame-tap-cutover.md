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
| Classification and capture routing | The shader resource shadow, the edge-triggered reconstruction set, near-miss logging, watch budgets, input and candidate reports, research before and after phases, geometry reports, divert verdicts, constant override and constant watch. |
| Resource and in-flight ownership | `ComRef` owns slot textures, the bound target, the view constants (under the lock), saved divert state, patched blends and pinned samplers with their clones. A thread-local depth keeps callback work out of the shadow. |
| Native leaves | `frame_tap.cpp` keeps the typed detours, `rsf::patch_slot`, every D3D11 query and call, the blend alpha patch, the sampler clone filter and `rsf_classify_texture`. Callback invokers keep C++ exceptions on the native side. |

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
  candidate entries, the input watch, the bound target) is atomic, and the plan uses release and
  acquire publication rather than relaxed.
- 64-bit diverted, candidate, refused and patched-blend counters are now 32-bit. The status always
  reported their low 32 bits.

## Open

- The user owns building and every test. The frame tap, constant override, scene promotion and AC7
  capture fixtures need a run, as does a Windows game run for the vtable refresh.
- Uninstall still reaches the render thread's shadow from its own thread, as the native owner did.
- Classification stays in `resource_roles.cpp` behind a leaf; a later port should call that owner
  directly.
