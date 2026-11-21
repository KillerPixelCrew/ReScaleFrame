# AC7 native renderer sites in Rust

Date: 2026-11-21. Base: `NightHammer1000/rust-ac7-native-renderer` at fdd0a2e, after the
merged `rsf-ac7` game entry ([rust-ac7-entry.md](rust-ac7-entry.md)).

## Question

Which owner inside `games/ac7/src/native_renderer.cpp` can move into the existing `rsf-ac7` crate
without changing a patch byte, an RVA, decryption timing, an engine or TrueSky decision, a
refusal, the public renderer C ABI or either combined carrier?

## Inventory

### native_renderer.cpp at fdd0a2e (3076 lines)

| Lines | Owner | Kind |
| --- | --- | --- |
| 31-133 | `Site` table: 65 hooked entries (RVA, role, 16 expected bytes, MinHook target/original/detour), `Hook` enum, evidence-hook rule (38-43) | exact-build data, hook state |
| 134-177 | `installed`, 64 cache-line guard slots, `guards_held`, `EntryGuard`/`OuterGuard` | hook body accounting |
| 178-212 | `game_module`, `engine<>` (test redirection), `bytes_match` (SEH read), GEngine `+3CBBC28` | native leaves, engine relay |
| 215-289 | `rsf_ac7_native_renderer` state | native |
| 290-760 | frame identity, render screen, CPU events, input hook, engine tick, simulation, redraw, viewport draw, controller facts, poll/pump/sync/pacing, pixel shader creation, frame tasks, RHI frame begin/end, renderer retirement, render family | detours, engine relays |
| 760-1000 | pass leases, native graph refs, drain, translucency size, uniform retention | detours, RHI leases |
| 935-1240 | SR graph node, plan, post-process uniform rebuild, `hooked_context` | engine graph |
| 1239-1680 | UI producer, scene colour pool, translucency and base pass scopes, particle bindings, beam/trail tagging, translucent mesh | detours |
| 1677-1800 | pixel uniform evidence hooks | diagnostics |
| 1796-2080 | post-process, owned views, construct, temporal sample index, visibility, descriptor and Process detours | detours |
| 2080-2410 | widget queue/targets/draw, Slate private/allocate/task/window/texture | detours, D3D leases |
| 2410-2470 | `write_render_code` (VirtualProtect, FlushInstructionCache), scene-format bytes, velocity table and transaction | patches |
| 2470-2763 | TrueSky depth dispatch relay, relay slot allocation, effect fingerprint, cloud motion relay, cloud divisor | TrueSky plugin patches, D3D11 |
| 2720-2735 | scene precision `+1095620` | patch |
| 2764-2890 | sky projection/render, scene colour format, `bind_detours`, environment flag | detours |
| 2892-3076 | `rsf_ac7_native_renderer_prepare/start/is_active/quiesce/stop`: admission of entry sites, 11 depth gates and 26 helpers, MinHook install, pinned partial activation, patch apply, retirement order | C ABI, lifecycle leaves |

### Renderer ABI and consumers

`games/ac7/include/rescaleframe/ac7_native_renderer.h` declares the 72-byte options and the five
entry points. Their callers:

- `plugin.cpp` lends them to the `rsf-ac7` lifecycle through `ac7_platform.h` (unchanged).
- `tests/ac7_native_graph.cpp` includes `native_renderer.cpp` with `RSF_AC7_GRAPH_TEST`.

`rsf_ac7_render_scope` holds `native_renderer.cpp` and is linked by `rsf_game_ac7`,
`rsf_ac7_velocity_pass` (and through it `rsf_proxy_dinput8` and `rsf_proxy_ac7`) and the
`ac7_velocity_shader`, `d3d11_present_bridge` and `ac7_render_scope` fixtures. No carrier or
other fixture references a symbol defined in `native_renderer.cpp` (only the five
`rsf_ac7_native_renderer_*` functions are external), so only the game DLL and the native graph
fixture pull its object from the archive. That is what lets the Rust dependency stay off the
carrier link lines.

## Moved

The crate gains three owners, reached from C++ through the internal header
`games/ac7/src/ac7_sites.h` (bindings `sites_abi.rs`):

- `sites.rs`: the 65 hooked entry sites in `Hook` order with their 16 expected bytes, the six
  Process pass roles, the 26 helper sites and the 11 enlarged UI depth/view gates.
  `rsf_ac7_build_admit` compares them in the former order (entries, gates, helpers) and logs the
  former texts at the first difference. `rsf_ac7_hook_rva`, `rsf_ac7_hook_role` and
  `rsf_ac7_hook_wanted` (evidence hooks 38-43 only with diagnostics) replace the table fields.
- `patches.rs`: the temporal velocity group (`+118A4E3 75 0E -> 90 90`, `+10FDD0C 76 38 -> 90 90`,
  `+11838C4 77 88 -> EB 88`) and scene precision (`+1095620`, 16 expected bytes, first 13
  replaced by `B8 04 00 00 00 90 90 48 8B CB 90 90 90`). Velocity keeps its preflight,
  rollback, idempotent ownership and reverse restore. `rsf_ac7_engine_patches_apply` keeps start's
  order (scene precision, then velocity; an applied patch stays owned on a later refusal) and
  `rsf_ac7_engine_patches_restore` stop's order (velocity, then scene precision) with the former
  log texts. Ownership stays in the renderer as `rsf_ac7_code_patches`.
- `guards.rs`: 64 cache-line slots, a per-thread slot index chosen by a relaxed counter, relaxed
  enter, release leave, a sequentially consistent fence and acquire loads in the sum, as before.

`native_renderer.cpp` keeps the `Hook` enum with `static_assert`s against the Rust count,
Process count and evidence range, the per-hook MinHook record (target, original, detour),
`bytes_match` and `write_render_code` lent as `rsf_ac7_code_leaves`, `EntryGuard`/`OuterGuard`
as thin RAII calls, and everything listed under Remaining.

`rsf_game_ac7` now links the Rust archive after `rsf_ac7_render_scope`, which the GNU
cross-link needs; `rsf_ac7_native_graph_test` links it too. The carriers' link libraries and
`rsf_ac7_render_scope`'s own link interface are unchanged.

## Preserved and differences

The tables were moved mechanically: a script compared all 91 entry and helper sites, the 11
gates, the 3 velocity patches and the three scene-precision arrays against fdd0a2e in order, and
they match. No patch site is added. Admission still runs before allocation, scope creation or
MinHook. Decryption timing is the carriers' and is untouched, as are TrueSky, the public renderer
ABI, `plugin.cpp` and the lifecycle.

Differences, none intended to change behaviour:

- Scene precision resolves its site as `game_module() + 0x1095620` instead of
  `engine<unsigned char*>(0x1095620)`. These are equal outside `RSF_AC7_GRAPH_TEST`, and the
  fixture never starts a renderer.
- Each guard is now two out-of-line calls into Rust, and each Process call and plan step looks up
  its role through one. Not measured.
- Refusal texts are formatted in Rust instead of fixed 128/144-byte buffers; each text fits
  those buffers, so the logged text is identical.
- A null leaf table or patch record refuses instead of crashing; C++ always passes both.

## Remaining AC7 work

In `native_renderer.cpp`:

- TrueSky plugin (`TrueSkyPluginRender_MT.dll`): divisor `+87F6D`, depth call relay `+C003F`,
  pass selector `+BFF0B`, depth format `+BFC09`, composite relay `+ABC6B`, the
  `mixed_resolution.fxo` fingerprint (45263 bytes, CRC-32 `78881EF3`), relay slot allocation and
  the device-dependent depth shader and binding checks under `cloud_resolution_guard`. Next
  candidate: their sites and the apply/restore ordering as a second Rust patch record, with relay
  allocation, D3D11 and the dispatch detour as leaves.
- The rest of prepare and stop: MinHook initialise/install/removal, pinned partial activation,
  render scope creation and destruction, the input hook, controller facts removal, the drain and
  live-counter checks, and the TrueSky restores. A Rust retirement plan over native leaves would
  own the order. Done on 21 November; see
  [the transaction migration note](rust-ac7-renderer-transaction.md).
- Every detour and engine relay: about sixty `engine<>()` RVAs and layouts used inside hook
  bodies, GEngine `+3CBBC28`, frame identity, the SR graph node and plan, UI producer,
  translucency and particle scopes, beam/trail tagging, Slate window/texture leases, widget
  targets and view sizing.

Elsewhere, unchanged since [rust-ac7-entry.md](rust-ac7-entry.md): the rest of the render-scope
library, `view_uniforms.cpp`, `ui_rules.cpp`, `scene_color.cpp`, `motion_capture.cpp`,
`velocity_pass.cpp`, and in `loader/proxy` both DirectInput carriers (`dinput8_proxy.c` with the
decryption wait and the carrier engine patches, `dlss_bridge.c`, the `shim_loader.cpp` AC7 branch).

## Checks

Source review against the removed C++; the scripted table comparison above;
`rustfmt --edition 2024 --check` (rustfmt 1.11.0, the pinned 1.94.1 toolchain is not installed);
`cargo metadata --locked --offline`; `engine.json` parse; `git diff --check`. bindgen-cli and
libclang are not installed, so the new generator entry was not run and `sites_abi.rs` is
hand-mirrored with layout assertions; the C++ side `static_assert`s the shared counts. New Rust
unit tests and the updated native graph fixture were written but not run. Not compiled, linked,
Cargo-tested, Clippy-checked, CTest-run, deployed or game-tested.
