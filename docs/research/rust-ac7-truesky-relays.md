# AC7 TrueSky patches and relays in Rust

Date: 2026-11-21. Base: `rust/orchestrator-anomaly` at 97c3f65, after the renderer transaction
([rust-ac7-renderer-transaction.md](rust-ac7-renderer-transaction.md)).

## Question

Can the TrueSky plugin code patches and call relays in `games/ac7/src/native_renderer.cpp` move
into `rsf-ac7` while the relay handlers, their D3D11 checks and shaders, the module lookup, relay
memory and protected code writes stay native, without changing a site, a byte, the effect
fingerprint, a producer or consumer, shader behaviour or the public C ABI? Is the former install,
rollback and retirement safe?

The rendering decisions are unchanged and come from
[the TrueSky skill](../../.agents/skills/truesky-render-integration/SKILL.md),
[its AC7 depth reference](../../.agents/skills/truesky-render-integration/references/ac7-depth.md)
and the [hook map](ue418-hook-map.md) sections for 2 and 4 October. This is a source move; no new
experiment was run.

## Former code (native_renderer.cpp at 97c3f65)

- `apply_cloud_resolution_patch`, per sky render while active, under `cloud_resolution_guard`:
  return once full grid is on or refused; find `TrueSkyPluginRender_MT.dll`; install the
  composite_tile relay at `+ABC6B` (16 bytes before and 16 from it; refusal logged and retried
  next frame); install the depth route (`install_depth_dispatch`), else refuse the full grid; wait
  for `cloud_depth_ready`; patch the divisor at `+87F6D`, else refuse.
- `install_depth_dispatch`: once owned, answer whether the format is owned; compare `+C003F`,
  `+BFF07` and `+BFC05` over 16 bytes and the `mixed_resolution.fxo` fingerprint (45,263 bytes,
  CRC-32 `78881ef3`); prepare the 7-byte relay; selector `75` to `77`; patch the relay, else
  restore the selector; format immediate `+BFC09` `16` to `05`, else log and refuse with relay and
  selector owned.
- Relays: a page within the allocation search range of the call holds the handler; the call
  becomes `ff 15 <rel32>` (plus `90` for 7 bytes); a refused rel32 distance or write frees it.
- Dispatch relay with unverified bindings after full-grid activation: under the lock, readiness
  off, refused, divisor restored for the next allocation, dispatch suppressed.
- `restore_truesky` (stop leaf): divisor, motion relay with its cloud depth resources, depth relay,
  format, selector, then the depth shader, device and readiness; each forgotten as restored, the
  first refusal logged and returned.

## Moved

`games/ac7/src/truesky.rs` owns the module name, the five sites with their RVAs, expected,
native and replacement bytes, the effect path, size and CRC, the relay byte construction and
rel32 check, the install order and refusals, full-grid refusal, the restore order and every log
text. The record `rsf_ac7_truesky_patches` (112 bytes, with two 40-byte `rsf_ac7_call_relay`)
replaces the renderer's former site pointers, `CallRelay`s and refusal flag; only Rust changes it,
always under `cloud_resolution_guard`. The effect fingerprint is read in Rust from the process
executable's directory (`std::env::current_exe`, which is `GetModuleFileNameW` of the process).

`rsf_ac7_truesky_leaves` in `ac7_sites.h` (104 bytes) lends: the existing code leaves (SEH read,
protected write) with the renderer's host log; `module` (`GetModuleHandleW`); `relay_allocate`
(the unchanged nearby-page search, then the handler stored in the slot) and `relay_release`; the
two handler addresses; `depth_ready`; `motion_release` (drops the `CloudDepth` resources);
`depth_release` (shader and device reset, readiness off). Entry points:
`rsf_ac7_truesky_apply`, `rsf_ac7_truesky_refuse_full_grid` and `rsf_ac7_truesky_restore`.
`apply_cloud_resolution_patch` and `restore_truesky` are now lock-and-call wrappers; the
renderer leaf table and `renderer.rs` stop order are unchanged.

## Preserved

Every RVA, expected, native and replacement byte, the module name, effect path, size and CRC
(mechanically compared with the removed C++), admission order (bytes before fingerprint), the
relay encoding and allocation search, install order (motion relay, selector, depth relay, format,
divisor after readiness), the per-frame motion retry, refusal semantics, log texts, restore order,
both relay handlers and their D3D11 checks, the one-to-one depth shader, the device-recreation
rebuild, cloud motion depth, the public renderer C ABI, `plugin.cpp`, the lifecycle and both
carrier links. No patch, site or rendering behaviour is added.

The separate cloud-depth texture used for motion reprojection now has a Rust owner in
`cloud_depth.rs`. It consumes the same composite tile bindings and preserves the padded-grid
calculation, opacity and distance constants. Native code keeps the original shader, D3D11
descriptors, dispatch and compute-state restoration. The Rust owner refuses nonfinite viewport
extents and stages a resized texture and view before dropping the old pair. A failed allocation
still refuses the draw. The source shader and binding order matched the previous implementation.
The synthetic viewport case and device and game checks remain unrun.

## Fixes

1. Restore ran without `cloud_resolution_guard`. The depth relay is not a MinHook hook, so the
   stop's hook disable does not stop it, and its handler does not check activity: with unverified
   bindings it restores the divisor and clears the site under the lock, while restore wrote the
   same site and pointer and reset the shader and device unlocked. Restore now holds the lock, and
   the handler reads the full-grid site under it instead of before taking it.
2. Relay memory and cloud resources were released while a relay call could still run. A handler
   that took its relay before its call was restored keeps reading the slot, the depth shader (used
   outside the lock between its check and `CSSetShader`) or the motion resources. Restore now puts
   every call back first, as the hook map's teardown asks, then refuses while any hook or relay
   guard is held, keeping the slots and resources for the next stop, and releases them last.
   Both handlers already hold entry and outer guards. A new log line reports the retained state.
3. A refused depth relay write rolled the selector back without checking the result, leaving it
   patched and unowned. A selector that cannot be restored now stays owned, and stop restores it.

Ownership checks use the relay slot, so a relay whose call is back but whose slot is retained is
never installed over. The motion and depth release now follow all code restores instead of
interleaving them; both only touch first-party resources.

## Checks

Source review against the removed C++; a script compared every moved byte array, RVA, the
fingerprint and all moved log texts with `git show 97c3f65:games/ac7/src/native_renderer.cpp`;
`rustfmt --edition 2024 --check` (rustfmt 1.11.0; the pinned 1.94.1 toolchain is not used);
`git diff --check`; `engine.json` parse; generator `py_compile`. bindgen-cli is not installed, so
the new `sites_abi.rs` items are hand-mirrored with size and offset assertions. New Rust unit
tests (`truesky/tests.rs`) cover the fingerprint and CRC, byte consistency, install order and
encoding, readiness, each admission refusal, relay reach and allocation refusal, both rollbacks,
format and divisor refusal, full-grid refusal, restore order, retry from every refused step and
retained relay memory while a relay runs. The native graph fixture now drives the real leaves and
Rust owner on a writable plugin image (motion relay in, depth route refused, restore). They were
written but not run. Not compiled, linked, Cargo-tested, Clippy-checked, CTest-run, deployed or
game-tested.

## Remaining

Native: both relay handlers with their D3D11 binding checks, `truesky_depth.cpp`,
`truesky_motion.cpp`, the sky render and projection detours, relay memory, and the other owners
listed in [the transaction note](rust-ac7-renderer-transaction.md). The guard read after a code
restore cannot see a thread between the relay `call` and its guard increment; MinHook removal has
the same window.
