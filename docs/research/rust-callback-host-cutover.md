# Shared callback render host Rust cutover

The baseline is `9da65ff` on `rust/orchestrator-anomaly`. The question was whether the shared callback render host behind `callback_render_host.h` could move to Rust without changing its callers, the two exported entry points, or the SR12, FG12 and plugin session owners it drives. This was a source port with synthetic contracts and CPU fixtures. No device, window, plugin DLL or game ran.

## Callers and surface

- `games/unity-mono/src/runtime_host.cpp` builds `rsf_callback_host_setup` from its own INI and probe, then calls `rsf_callback_host_start` and `rsf_callback_host_stop`. Engine identity, the backend choice mask and the `set_engine_spatial` policy callback come from that adapter.
- `tests/orchestrator_domains.cpp` requires `rsf_callback_host_start` in the Runtime DLL. `tests/unity_sr_d3d12.cpp` drives the host on a D3D12 device.
- The runtime core object library carries the host, so the Runtime DLL, both `dinput8.dll` carriers and `ReScaleFrame.Minecraft.dll` export the two entries.

## Ownership after the move

| Concern | Owner and evidence |
| --- | --- |
| Admission and refusal codes | `callback_host.rs` reads the setup prefix before any field and keeps the order and codes: 1 for an existing host or an incomplete or mismatched setup, 2 for an invalid preset, backend or quality, 4 when the log cannot be opened, 8 when the adapter policy refuses, 9 when a refused plugin cannot be stopped (the host stays published to retry), 10 when the plugin refuses preparation or activation, 11 when the host cannot be allocated. `callback_render_host_platform.cpp` asserts the setup and plugin session option layouts the Rust mirrors name. |
| Plugin lifecycle | Rust fills `rsf_game_host_services` (log, render pass, render configuration, CPU event, session id from the tick plus one), prepares and starts the session, quiesces and stops a refused one, and on stop quiesces and stops it before the overlay, both SR12 contexts and the input dump are retired. The session entries are the public `rsf_plugin_session_*` ones, reached through the leaf table so contracts can substitute them. |
| Host state | One `Mutex<State>` holds the SR and input-normalizing SR12 contexts, the plan, the three recording slots, requested and prepared backend and quality, preset, sharpening, panel flags, counters and the pending FG provider save. The log file has its own lock. The plugin handle is an atomic written only under the lifecycle lock. |
| Renderer configuration | Auto resolution against the adapter that owns the command list's device through `rsf_gpu_select_sr_backend`, the adapter policy query (spatial or not, never the vendor), the engine spatial plan by quality ratio, vendor SR12 preparation with the DLSS registration fields, and the reconfiguration rule (only on a pass flagged `RSF_GAME_RENDER_RECONFIGURE`, or on an output resize) are unchanged in order. |
| Event, scene and submission matching | The end of SR, FG input, final scene and submission passes reports `session_id` and `source_frame_id` to FG12. Final scenes go to FG12 as HUD-less input, window passes to FG12 before the panel. Depth-domain requests, short passes and missing lists are ignored. The leased resources stay the pass's own: SR12 receives the pass and its recording slot, and the first refusal logs the actual input descriptions. |
| Settings and overlay control | Sharpening and the panel layout go through `settings_store.rs`. The DLSS preset is saved through the new `write_dlss_preset`, which writes the store's `DLSSPreset` key; profile keys match case-insensitively, so the adapter's existing `DlssPreset` entry is replaced and still read. The overlay statistics, refusal reasons, the provider switch save after the switch lands, FG options, the performance HUD and SR changes subject to the adapter policy are in Rust. `rsf_overlay_fill_fg_stats` and the panel draw remain native leaves. |
| Platform leaves | Each leaf in `callback_render_host_platform.cpp` performs one D3D12, DXGI, SR12, FG12, presentation bridge, FG choice, overlay, input dump, plugin session, tick or header helper call. The two public entries stay native forwarders so the Runtime DLL and embedded consumers keep their exported names and ordinals. |

No plugin or vendor policy entered the generic host. The adapter remains the only source of engine identity and of the spatial policy. The one engine name in the old host, a log line naming URP, is now engine-neutral.

## Changed behavior

- The spatial plan logs "FSR1 selected through the engine's own spatial upscaling path." instead of naming URP (Rule 1).
- The DLSS preset is written under the store-owned `DLSSPreset` key name through `settings_store.rs` instead of a direct `WritePrivateProfileStringW` of `DlssPreset`. Reads are case-insensitive, so the Unity adapter's read is unaffected; `preset_write_replaces_only_the_preset_key` checks the existing key is replaced, not duplicated.
- Log lines are no longer truncated to fixed buffers.
- The present-count leaf returns no count for a null swapchain instead of dereferencing it.
- The log file is opened through `std::fs` with the same `FILE_APPEND_DATA` access, read and write sharing and open-always semantics.
- `rsf_sr12_destroy` is no longer called with a null context; SR12 ignored null, so nothing observable changes.
- The host allocation is still fallible and returns 11. Other allocation failures and panics abort instead of returning 11 from start or 2 from stop.

## Verification

Commands ran from the worktree with Visual Studio 2026. The build directory was configured with `cmake -S . -B build/callback-host -G "Visual Studio 18 2026" -A x64 -DBUILD_TESTING=ON` and the vendor and reference paths of the coordinator's `build/rust-migration` cache; the pinned yyjson files were copied from the coordinator checkout after a fresh download failed its hash check.

- `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets --locked -- -D warnings` and `cargo test --workspace --locked` passed. `rsf-orchestrator` ran 100 tests, 14 of them new callback host contracts and one new settings store test, with a recording platform, a fake plugin session, reference-counted fake COM objects and temporary log and preferences files. They cover admission order and every refusal code, the services handed to the plugin, start and stop order and the retry paths, Auto resolution per vendor, adapter and policy refusal, the spatial plan, recording slots and counters, the input-normalizing context, pass-end submission reports and scene routing, reconfiguration on the flagged pass, overlay statistics and intent persistence, the FG provider save, the sharpening poll and log appends.
- Debug CTest `callback_host_abi` (Runtime DLL) and the new `callback_host_abi_embedded` (the same fixture linked against `ReScaleFrame::OrchestratorCore`) check the refusal codes 1, 2, 4, 8 and 10, the policy queries and stop without a host, without loading a plugin. With `orchestrator_domain_contract`, `render_frontend_contract`, `render_frontend_embedded`, `sr12_abi`, `sr12_abi_embedded`, `fg12_abi`, `fg12_abi_embedded` and `fg_choice` they passed 10 of 10.
- `dumpbin /exports` on Release `ReScaleFrame.Runtime.dll` (381), the AC7 `dinput8.dll` (256), the shared `dinput8.dll` loader (273) and `ReScaleFrame.Minecraft.dll` (267) matched the coordinator's `build/rust-migration/baseline-exports` in every ordinal and name.
- Built only, not run: `rsf_game_unity_mono` and `rsf_unity_sr_d3d12_test`, which creates a D3D12 device and drives this host.

Unverified: real D3D12 recording through this host, vendor evaluation, the shared overlay on a device, the Unity adapter in a game and the full `eng/verify.ps1` suite.
