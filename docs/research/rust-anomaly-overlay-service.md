# Anomaly overlay service in Rust

This note covers the move of `games/anomaly/src/overlay_service.cpp` into the
`overlay` module of the `rsf-anomaly` static library. It is a source port with
CPU contracts. No game, device or visible-window run was made for it. The
baseline it must match is the C++ adapter at `8322179a36c7`
([baseline note](rust-anomaly-cpp-baseline.md)), where Insert opened and closed
the panel, the panel applied and saved options and SR/FG choices, and game input
returned after closing.

## Ownership after the port

| Concern | Before | Now |
| --- | --- | --- |
| Binding `rsf_get_overlay_api` from the carrier-loaded Runtime, size/version/entry validation, refusal reasons | `overlay_service.cpp` `bind_api` | `overlay.rs` `bind`/`validate`, exported privately as `rsf_anomaly_overlay_bind` |
| Setup translation: identity, INI path and `[Anomaly]` section, defaults, SR choices from the Stage 5 link, FG choices from the bridge link, latency and generated-frame limits, DLSS preset availability | `make_setup` | `overlay.rs` `make_setup` |
| Option table, `options_read`, `option_changed`, `option_label` | C++ lambdas over the `std::vector` registry in `adapter_options.h` | `overlay.rs`; the registry itself is a Rust list fed by `rsf_anomaly_overlay_option_add`, and `options::add` forwards to it |
| `settings_changed`, the saved-settings log and the startup SR request | `overlay_service.cpp` | `overlay.rs`, through the `sr_request` leaf |
| Start state (bound table, started flag), cold-start order and every refusal log line | `State` and `start` | `Overlay` in `overlay.rs` |
| Engine-chain identity, DEVICE_RESET on a new chain or device, resize notify, Insert key-poll edge | `on_present`, `on_resize` | `Service::present`, `rsf_anomaly_overlay_resize` |
| Panel run diagnostics (open/close edges, first frames, bounded progress lines, input and settings counter diffs) and the opt-in backbuffer capture schedule | `track_panel`, `say_panel_summary`, `configure_capture` | `Service::track`, `Service::summary`, `configure_capture` |
| Engine window filter (`_XRAY_1.5` class in this process), `GetAsyncKeyState`, INI reads, local time stamp, Runtime exports | Win32 calls inline | `win32` leaf functions in `overlay.rs` (windows-sys) |
| `IDXGISwapChain::GetDesc`/`GetDevice`, `GetImmediateContext`, `GetBuffer` and their releases | inline COM | `describe_chain`, `capture_backbuffer` leaves in `overlay_service.cpp` |
| SR and FG stage links (`sr_overlay.h`, `fg_overlay.h`) | read inline | `sr_choices`, `sr_report`, `sr_request`, `fg_link`, `fg_foreign_chain` leaves |
| Hook installs | called inline | `install_input`/`install_present` leaves calling the unchanged `overlay_hooks.cpp` |

The private layouts live in `overlay_platform.h`. `tools/generate-rust-bindings.py`
generates `overlay_abi.rs` from it together with the Runtime's `overlay_api.h`,
`named_frame_dump.h`, `render_frontend.h`, `frame_generation.h` and
`sr_session.h` constants with bindgen 0.72.1, so the adapter uses the Runtime's
one generic overlay table and its setup, option, capture and diagnostics
structures, not a hand mirror. `rsf_overlay_sr_report`, `rsf_game_log_fn` and
`rsf_result` come from `rsf-game-sys`. `overlay_service.cpp` asserts that the
C++ `StartStatus` values and the input gate's capture bits equal the generated
constants.

`overlay_service.h` keeps its declarations, so `bootstrap.cpp`, `fg_stage.cpp`
and the three overlay fixtures compile unchanged. `overlay_hooks.cpp`,
`input_hooks.cpp` and `overlay_input_gate.h` are untouched. The one
`InputGate` the hooks register devices with is now a never-destroyed object
created by `input_gate()`, whose capture query forwards to
`rsf_anomaly_overlay_capture_bits`. The C++ `State` held the gate in the same
leaked singleton.

Rule 1 and Rule 2 are unchanged in substance: the Runtime receives the same
setup data and callbacks and no game knowledge, and every registered adapter
option is still drawn and applied live. Rule 3 is not touched; the service
reports SR status through the stage link and forwards the shared settings.

## Behaviour kept

The setup, log text and refusal reasons are the same strings and formats.
`StartStatus` values are unchanged. The order inside a start is unchanged: the
configuration path check, binding, configure, `OverlayKeyPoll`, the capture
configuration and its logs, the saved-settings log and SR request, the input
gate, the Present hooks, then the active line. Capture still writes
`<phase><cycle>_f<index>` PNGs through `rsf_input_dump_named_texture` with
view 0 and scale 1, the same frame/stride/cycle bounds (1..16, 1..600, 1..8)
and the same default directory beside the INI.

Three implementation differences do not change results:

- The option table is a snapshot of the first 24 registered options taken at
  start. The C++ callbacks walked the live vector; every option is registered
  during cold start before the overlay starts, and the Runtime only asks for
  indices below the configured count, so the indices and callbacks are the same.
  A later registration can no longer reallocate the vector under a Present-thread
  callback.
- The bound table is published once through `OnceLock` before the hooks are
  installed. The C++ service copied the table into a plain struct that the
  DirectInput thread could already read through the gate installed earlier by
  the input-facts step.
- A log line is truncated at 767 bytes on a UTF-8 boundary, matching the C++
  `char[768]` buffer.

## Defect fixed

After a hook refusal the C++ start had already stored the configured table, so a
second `start` returned `already_started`. The cold entry's overlay leaf treats
`already_started` as success, so a repeated `rsf_anomaly_start` after status 9
reported READY although neither Present nor input hooks were active. The Runtime
accepts one `configure`, and `overlay_hooks.cpp` refuses a second Present install,
so the retry can never complete. The Rust start now keeps that refusal:
a start after a hook refusal returns `hooks_refused` with the reason "the overlay
hooks were refused earlier in this process" and configures nothing.
`overlay::tests::a_hook_refusal_stays_refused_on_retry` covers it.

## Checks

The worktree build used Visual Studio 18 2026 (MSVC 14.52) in `build/overlay`,
configured with the coordinator's `build/rust-migration` vendor include paths, a
private `FETCHCONTENT_BASE_DIR`, the read-only MinHook source override, the
pinned yyjson files copied from the coordinator checkout and NVAPI diagnostics
off.

- Debug and Release built with no warnings or errors: `rsf_game_anomaly`, the
  overlay input-gate, API, hooks and visibility fixtures, the SR hooks, profile
  and Runtime API fixtures, the input-hook, plugin, lifecycle, native-hook,
  bootstrap, FG policy and `rsf_plugin_contract` fixtures, and the
  `rsf_anomaly_fg_device_test` and `rsf_anomaly_sr_temporal_device_test` device
  executables (built, not run as device tests).
- Passed in both configurations (16 each): `anomaly_overlay_input_gate_contract`,
  `anomaly_overlay_api_contract`, `anomaly_sr_hooks_contract`,
  `anomaly_sr_hooks_avx_contract`, `anomaly_input_hooks_standard_contract`,
  `anomaly_input_hooks_avx_contract`, `anomaly_input_events_standard_contract`,
  `anomaly_input_events_avx_contract`, `anomaly_fg_policy_contract`,
  `anomaly_plugin_contract`, `anomaly_lifecycle_contract`,
  `anomaly_native_hooks_contract`, `anomaly_bootstrap_contract`,
  `anomaly_sr_profile_contract`, `anomaly_sr_runtime_api_contract` and
  `game_plugin_contract`. The overlay API contract runs the production C++
  `start`, `bind_api`, `query_capture` and the Rust `accept_window` against fake
  tables and two hidden `WS_POPUP` windows, with hooks off. It checks every
  refusal before configure, the exact setup the Runtime receives and a single
  configure.
- Six new Rust tests in `overlay::tests` cover binding refusals and the written
  table, setup translation with and without the SR and FG links, the retry
  defect, DEVICE_RESET on a new chain or device and the Insert edge, panel edge
  logs and the capture schedule (`open01_f00`, `open01_f01`, `close01_f00`,
  `close01_f01`, none for the second cycle), and log truncation. With the
  sticky refusal changed back to `already_started`, the retry test failed with
  `left: 1`, `right: 5`.
- `cargo fmt --all --check`, `cargo clippy --workspace --all-targets --locked
  -- -D warnings` and `cargo test --workspace --locked` (226 tests) passed.
  Rerunning `tools/generate-rust-bindings.py` with bindgen 0.72.1 reproduced
  `overlay_abi.rs` byte for byte and changed no other binding.
- The Debug and Release DLLs export `rsf_anomaly_observation`,
  `rsf_anomaly_start` and `rsf_get_game_plugin_api` at ordinals 1, 2 and 3, the
  same as the staged `8322179a36c7` DLL. Neither imports
  `ReScaleFrame.Runtime.dll`; both hold the wide module name the cold entry
  passes to `GetModuleHandleW` and the export names resolved from that module
  (`rsf_plugin_session_prepare`, `rsf_get_overlay_api`,
  `rsf_get_overlay_diagnostics`, `rsf_input_dump_named_texture`,
  `rsf_get_d3d11_sr_api`). Compared with the baseline, the Release DLL adds the
  `ntdll.dll` and `api-ms-win-core-synch-l1-2-0.dll` imports recorded for the
  earlier Rust cuts and lower-case `kernel32.dll` and `user32.dll` entries from
  windows-sys; `user32.dll` was already imported.

## Gaps

The panel, its input withholding and the live option and SR/FG application were
not run. The overlay hooks fixture uses a WARP swap chain and system DirectInput
devices and the visibility fixture loads the real panel over a hidden window,
so both were built and not executed. A game run on the Standard and AVX
executables must confirm Insert open and close, mouse and keyboard withholding
and release, option and SR/FG application with saved INI values, and the
adapter log lines recorded in the baseline.

## Direct stage links

The overlay now gets SR choices, effective reports and next-frame settings
directly from the Rust Stage 5 owner. It gets FG startup defaults from the
installed Rust FG stage and uses that stage's existing chain identity check
to skip provider presents. The C++ overlay and stage facades previously
published two extra pointers and copied the same records across the private
ABI. Both publications and the unused native record types were removed.
The native overlay table now carries chain description, capture, and the two
hook installers.

The stage readiness checks and settings application order are preserved.
The pinned Rust workspace passed 382 tests, formatting and Clippy.
The Anomaly DLL and ten focused MSVC Release contracts passed, including
overlay API and input gate, SR and input hooks for both profiles, and the
Runtime SR API. The DLL kept the staged three exports and by-name Runtime
lookup. This change still needs foreground overlay and gameplay validation.
