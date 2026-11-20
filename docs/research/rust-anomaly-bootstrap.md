# Anomaly cold entry in Rust

This note covers the move of `games/anomaly/src/bootstrap.cpp` into the
`bootstrap` module of the `rsf-anomaly` static library. It is a source port with
CPU contracts. No game, device or window run was made for it.

## What moved

`bootstrap.rs` now owns the synchronous cold entry that the thin carrier calls
before the engine creates DirectInput or its device:

- the configuration checks (non-empty, existing file, not a directory, absolute
  path) and the `rsf_anomaly_start` status values 0 to 9, unchanged;
- exact build selection. The ASCII executable name must be profiled before
  the file is hashed, then the name and SHA-256 go through the same Rust
  `match_build` that plugin detection uses;
- the adapter log, `ReScaleFrame.Anomaly.log` beside the INI, with the same
  `[pid:tick] message` lines;
- the `[Anomaly] Mode` and `Overlay` reads and the mode refusal;
- the borrow of the already-loaded `ReScaleFrame.Runtime.dll` through
  `GetModuleHandleW`, without a reference, and the plugin session started
  through `rsf_plugin_session_prepare` and `rsf_plugin_session_start` resolved
  from that module. The adapter neither imports nor loads a Runtime, and the
  crate does not depend on `rsf-orchestrator`;
- the step order: restart options, SR, motion, the FG bridge and input facts,
  then the Observe session, then the overlay and its FG Present sink;
- the entry lock, the started/observed state and the retained partial session.

`bootstrap.cpp` keeps the `rsf_anomaly_start` export, so the name and ordinal
stay in the MSVC export table. It also holds a table of noexcept leaves that
forward to the existing C++ owners: `build_profiles` labels, `rsf::sha256_file`,
`register_restart_options`, the SR, motion and FG option readers and
configurators, `fg::install`, the overlay input hooks with the engine input
hook, `overlay::start` and `fg::set_overlay_sink`. Profiles, the native observer
and the option and policy owners did not change. Rust reaches the leaves through
a pointer table, so the static library has no unresolved link-time symbols and
the Rust unit test can supply its own leaves.

## Defect fixed

The C++ entry repeated cold-start steps when it was called again after a
refusal. After an overlay refusal (status 9), the retry registered the three
restart options again, so the overlay would list them twice. After an Observe
refusal it also reconfigured SR and motion, adding a second motion pack row,
and re-ran the FG and input installs. Their own guards limited those installs to
log noise. With a partial session still owned after a busy prepare or a failed
start, the retry did all of this before refusing with status 5.

The Rust entry registers restart options once and configures the Observe stages
once. A retry that still owns a partial session now refuses with status 5 before
it calls any stage. `bootstrap::tests::retry_repeats_only_unfinished_steps`
counts the leaf and session calls in both cases. With the registration guard
removed, the test fails with `left: [2, …]`, `right: [1, …]`.

## Checks

- MSVC (VS 2026) Debug and Release builds of `rsf_game_anomaly`, the CPU
  fixtures below and the `rsf_anomaly_fg_device_test` device fixture (built
  only, no device mode run). This private build set `RSF_NVAPI_INCLUDE` to
  not-found because the root's configured nvapi path no longer exists. That
  only removes the Runtime's optional NVAPI diagnostics.
- Passed in Debug and Release: `anomaly_bootstrap_contract` (null, empty and
  missing INI refuse; a foreign host and copies named `AnomalyDX11.exe` and
  `AnomalyDX11AVX.exe` with foreign hashes refuse with status 2 before any
  Runtime is loaded), `anomaly_lifecycle_contract`, `anomaly_plugin_contract`,
  `game_plugin_contract`, `anomaly_sr_profile_contract`,
  `anomaly_native_hooks_contract`, `anomaly_sr_runtime_api_contract`, and
  the Standard and AVX `anomaly_input_hooks_*` and `anomaly_input_events_*`
  contracts.
- `cargo fmt --all --check`, `cargo clippy --workspace --all-targets --locked
  -- -D warnings` and `cargo test --workspace --locked` (217 tests) passed.
- The Debug and Release DLLs export `rsf_anomaly_observation`,
  `rsf_anomaly_start` and `rsf_get_game_plugin_api` at ordinals 1, 2 and 3,
  the same as the staged `8322179a36c7` adapter. Neither imports
  `ReScaleFrame.Runtime.dll`. The Rust standard library adds `ntdll.dll` and
  `api-ms-win-core-synch-l1-2-0.dll`, plus a `kernel32.dll` import from
  `windows-sys`.

## Gaps

The accepted cold path, from a profiled executable to the borrowed Runtime,
session, FG bridge and overlay, needs the real game executable. It ran only
through the Rust unit test with stand-in leaves and session calls. A game run
must confirm the adapter log sequence and the live behavior recorded in
[the C++ baseline](rust-anomaly-cpp-baseline.md).
