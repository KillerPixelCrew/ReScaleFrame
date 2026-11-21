# AC7 native renderer transaction in Rust

Date: 2026-11-21. Base: `rust/orchestrator-anomaly` at 8fae7cd, after the merged site, patch and
guard owners ([rust-ac7-native-sites.md](rust-ac7-native-sites.md)).

## Question

Can the prepare/start/quiesce/stop transaction behind the five calls of
`games/ac7/include/rescaleframe/ac7_native_renderer.h` move into `rsf-ac7` while the detours,
engine fields, code writes, pinning and MinHook stay native, without changing a hook site, a
byte, the exported ABI, TrueSky handling or either carrier link? And is the former partial
activation and retry path safe?

## Former transaction (native_renderer.cpp at 8fae7cd, lines 2744-2867)

- `prepare`: refuse a null, short or sessionless options record, a null output or an installed
  renderer without touching the output; clear the output; admit the build (`sites.rs`);
  allocate; create pass scopes (capacity 256 when zero); `MH_Initialize` (already initialised is
  accepted); diagnostics from the option bit or `RSF_AC7_PIXEL_DIAGNOSTICS`; bind detours and
  targets (`rsf_ac7_hook_wanted`); publish `installed`; `rsf::ac7::install_hooks`. On failure the
  helper had already disabled and removed what it made; then, with any guard held, pin the
  module, mark quiescing, quiesce the scopes and return the inactive renderer; otherwise clear
  `installed`, quiesce and destroy the scopes and delete.
- `start`: the installed, unquiesced renderer only; scene precision then velocity
  (`patches.rs`); active.
- `is_active`: installed and active. `quiesce`: quiescing, inactive, controller facts disabled,
  scopes quiesced.
- `stop`: null succeeds; active refuses; drain; refuse while entry guards, live identities,
  window or frame tasks, pending uniforms or particle bindings, retired leases or busy scopes
  remain; unhook the input hook; disable every bound target; refuse while any guard is held or the
  controller facts cannot be removed; velocity then scene precision; the TrueSky restores;
  destroy the scopes; drain and refuse a retired lease; remove the hooks; clear `installed`;
  delete.

## Moved

`games/ac7/src/renderer.rs` owns that order and every refusal. It reuses the existing owners and
their state rather than holding its own: `sites::admit`, `sites::wanted` and `HOOK_SITES` for
admission and targets, `patches::apply`/`restore` on the renderer's `rsf_ac7_code_patches`
record, `rsf_ac7_guards_held` for the hook body counts, and the lifecycle's `PENDING_CAPACITY`
(256) for the scope default. The installed pointer, the active and quiescing flags, the MinHook
records (`sites[]`) and the scopes stay native; Rust reads and changes them through leaves.

The boundary is `rsf_ac7_renderer_leaves` in the internal header `games/ac7/src/ac7_sites.h`
(272 bytes; `sites_abi.rs` mirrors it with layout asserts). `native_renderer.cpp` lends: the code
leaves and game module; `installed`/`install`; renderer `create`/`release`, `options`, `patches`,
`active`, `quiescing`; scope create/quiesce/idle/destroy; `hooks_initialize` (MH_Initialize, then
`bind_detours`); per-hook `hook_bind`/`create`/`enable`/`created`/`disable`/`remove`; `pin`;
`drain`; `outstanding`; `retired`; `input_unhook`; controller facts disable/remove; and
`truesky_restore`, the former TrueSky restore block unchanged as `restore_truesky`. Every leaf is
`noexcept`. The five exported calls keep their signatures and results and call
`rsf_ac7_renderer_prepare/start/is_active/quiesce/stop` with the leaf table. `plugin.cpp`, its
exception guards and `lifecycle.rs` are unchanged. The pixel diagnostics environment variable is
read in Rust with the former eight-byte rule.

`rsf::ac7::install_hooks`/`remove_hooks` in `ac7_hooks.h` are unchanged and still serve
`motion_capture.cpp`; the renderer no longer includes them. The C++ `guards_held` wrapper is gone;
the native graph fixture calls `rsf_ac7_guards_held` directly.

## Preserved

Hook sites, RVAs, expected bytes, detours and their binding, admission and its texts, the
evidence-hook rule, the prepare and pinned-failure log texts, start and stop patch order and texts,
the stop admission checks and order, the TrueSky restore order and texts, the public renderer
ABI, `plugin.cpp`, the lifecycle, and the link graph: the Rust archive was already on the game DLL
and native graph fixture link lines, and the carriers still link `rsf_ac7_render_scope` without
pulling `native_renderer.cpp`. No hook site is added and nothing visual changes.

## Differences

1. Partial activation keeps reachable callbacks. The former helper removed every hook it had
   created (`MH_RemoveHook` frees the trampoline) before prepare looked at the guards, so a pinned
   forwarding call that had not yet reached `sites[i].original` would jump into freed MinHook
   memory. Now a failed installation disables what it created, and removes it only if no guard
   is held. When one is, the hooks stay created and disabled, `installed`, the renderer, its
   options callbacks and the quiesced scopes stay alive in the pinned module, and stop removes
   the hooks once the guards drain.
2. Hooks are tracked by what was created. `hook_created` is `target && original`; a successful
   (or not-created) `MH_RemoveHook` clears `original`. Stop disables and removes created hooks
   instead of every bound target, so it removes exactly what a partial activation left, and a
   later prepare never sees a stale trampoline.
3. The engine tick no longer installs the game-thread message hook once the renderer is
   quiescing. Stop clears `input_hook` from another thread while the engine tick hook was still
   enabled, so a tick in between could install a new message hook that nothing removed, and the
   field was written by two threads. Stop's admission requires idle scopes, which only quiesce or
   the pinned failure produce, and both set quiescing first, so the window is closed. No input
   event is lost: events are reported only while active, which a quiescing renderer never is
   again.
4. Leaves load and store the flags and `installed` sequentially consistent; the former code used
   acquire/release at a few sites. Stronger, not weaker.
5. The diagnostics variable is read as UTF-16 rather than ANSI, so the seven-character limit counts
   differently only for non-ASCII values.
6. Each transaction call builds the leaf table and crosses into Rust; `is_active` runs under the
   lifecycle lock for status. Not measured; it is a few calls, not per frame.

Retry safety, checked step by step: every stop step forgets only what it retired (input hook,
created hooks, engine patch records, each TrueSky site and relay, scopes), null scopes are idle
and destroy successfully, and the engine patch owner is idempotent. A stop refused after the
hooks are disabled leaves `quiescing` set, so start refuses a half-retired renderer.

## Remaining AC7 owners

In `native_renderer.cpp`: the TrueSky plugin sites (`+87F6D`, `+C003F`, `+BFF0B`, `+BFC09`,
`+ABC6B`), the `mixed_resolution.fxo` fingerprint, relay allocation and the device-dependent depth
checks (the sites, fingerprint and their transaction later moved to `truesky.rs`, see
[rust-ac7-truesky-relays.md](rust-ac7-truesky-relays.md)); every detour and engine relay with about sixty `engine<>()` RVAs and layouts, GEngine
`+3CBBC28`, frame identity, the SR graph node and plan, UI producer, translucency and particle
scopes, beam/trail tagging, Slate leases, widget targets and view sizing; the renderer object
itself.

Elsewhere: `render_scope.cpp` (its owner later moved to `render_scope.rs`, see
[rust-ac7-render-scopes.md](rust-ac7-render-scopes.md)), `view_uniforms.cpp`, `ui_rules.cpp`, `scene_color.cpp`,
`motion_capture.cpp` (with `ac7_hooks.h`), `velocity_pass.cpp`, `velocity_shader.cpp`,
`truesky_depth.cpp`, `truesky_motion.cpp`, `contact_shadow.cpp`, and in `loader/proxy` both
DirectInput carriers (`dinput8_proxy.c` with the decryption wait and carrier engine patches,
`dlss_bridge.c`, the `shim_loader.cpp` AC7 branch).

## Checks

Source review against the removed C++; `rustfmt --edition 2024 --check` (rustfmt 1.11.0; the
pinned 1.94.1 toolchain is not installed); `cargo metadata --locked --offline`; `engine.json`
parse; `git diff --check`. bindgen-cli and libclang are not installed, so the updated generator
entry was not run and `sites_abi.rs` is hand-mirrored with size and offset assertions. New Rust
unit tests (`renderer/tests.rs`) cover admission first, binding and publication order,
diagnostics, initialisation failure, installation failure with and without a running hook body,
start refusals, quiesce, stop admission, stop order and retries after each refusal; they and the
updated native graph fixture were written but not run. Not compiled, linked, Cargo-tested,
Clippy-checked, CTest-run, deployed or game-tested.
