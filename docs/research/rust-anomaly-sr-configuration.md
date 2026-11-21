# Anomaly SR configuration owner in Rust

The port starts from `8268cb9`. After the [SR stage driver port](rust-anomaly-sr-stage.md),
`sr_stage.cpp` still held a second copy of the SR configuration. Its
`configured_options` (`StageOptions`) kept the retained INI path and the
Streamline, FidelityFX and XeSS directories. `refresh_live()` mirrored the six
live Rust options into it. `sr::options()` exposed the whole struct, but only
three readers used it: `selected()` in `native_observer_platform.cpp`,
`.capture` in `motion_stage.cpp`, and the SR hooks fixture. The Rust stage
reached the paths back through the `configured_ini` and `vendor_file` leaves.

This is an ownership change. It changes no hook site, guard, shader, buffer,
producer, render-target layout, jitter formula or vendor call. The seam research
and its shader and buffer coverage audit still apply.

## Ownership after the change

| Concern | Before | Now |
| --- | --- | --- |
| Scalar `[Anomaly]` SR keys | Rust parser, native `StageOptions` copy | Rust `State.config` only |
| Retained INI (SRScale re-read after a device reset) | `StageOptions::ini`, read back through `configured_ini` | Rust `State.paths.ini`, passed to the existing `ini_text` leaf |
| SRStreamlineDir / SRFidelityFXDir / SRXeSSDir and the package beside the INI | C++ `read_options` (`std::filesystem`, `GetPrivateProfileStringW`, `plain_directory`) | Rust `Ini::vendor_directories` through the existing `ini_text` leaf (`MAX_PATH` buffer), Rust `plain_directory` |
| Vendor availability for the overlay | `vendor_file(directory, name)` joined the native path | Rust joins the path. The `file_exists(path)` leaf calls `GetFileAttributesW` |
| Live panel options | C++ `register_live_options`, `live_get`/`live_set` templates calling Rust, then `refresh_live` | Rust `live_options()` registers the same six entries with the overlay registry, as the motion stage does |
| TemporalDriver options (Stages 4 and 5) | C++ built `TemporalOptions` from `StageOptions` | Rust passes a transient `rsf_anomaly_sr_driver_config` with UTF-8 directories through the `configured` leaf. The C++ facade copies them into `TemporalDriver::configure` |
| D3D11 host log | Set by C++ `configure` | Set by the same `configured` leaf |
| Observer install decision | `sr_selected` leaf → `sr::options().selected()` | `STAGE.selected()` in `native_observer.rs` |
| Motion capture refusal | `sr_capture` leaf → `sr::options().capture` | `STAGE.capture()` in `motion/stage/capture.rs` |

`selected()` and `capture()` read the existing lock-free `stage` atomic and a
new `capture` atomic beside it. The C++ reads they replace did not lock. The
observer and motion install paths therefore still take no SR stage lock.

The bootstrap leaf keeps its signature. It now calls `sr::configure(ini,
apply, log, user)`, which forwards to `rsf_anomaly_sr_stage_configure_ini`.
That function parses the keys. When `apply` is set and a stage or the capture
is selected, it keeps the keys with the INI path and directories, registers the
live options and returns whether one is selected. The rule is unchanged:
nothing is kept unless it is selected, and configuration is refused while the
hooks are installed.

`sr::read_options` now returns the scalar `rsf_anomaly_sr_stage_config` for the
contract fixture. `read_scale` and `read_mip_bias` are unchanged. The native
`plain_directory` in `sr_settings.h` had no remaining caller and was removed.
The FG options reader now uses the Rust helper (same UTF-16 rule), and its
four cases moved from `anomaly_sr_pass.cpp` into a Rust test. The stale
`sr::options()` double in `anomaly_fg_device.cpp`, which nothing read, was
removed.

## Preserved behaviour

- Defaults and clamps are those of the existing Rust parser, unchanged.
- Directory policy: an explicit key wins. Otherwise `<INI parent>\ReScaleFrame\{streamline,fidelityfx,xess}`.
  Both pass through the same plain-directory rule (verbatim and UNC prefixes,
  `/` to `\`, trailing separators). By source review, Rust `Path::parent` and
  `join` give the same result as `std::filesystem::path::parent_path` and
  `operator/` for an absolute path, the `\\?\` form, a bare file name and a
  drive-relative name. The tests exercise the first two.
- UTF-16: the INI and the directories stay `OsString` (unpaired surrogates
  kept). The UTF-8 copies for the TemporalDriver use lossy replacement, as
  `WideCharToMultiByte(CP_UTF8, 0)` did.
- Absent vendor: no file means no backend is offered. A directory named like
  a runtime is not a file. Below Stage 5 or unconfigured, nothing is offered.
- Unconfigured: stage 0, no capture, no INI, so no reset re-read. A stage
  configured without an INI also keeps its scale.
- The overlay labels, kinds, ranges, divisors, get/set wiring and saved keys
  are unchanged. The six options are still registered at the same point in the
  cold start.

## Private ABI

`tools/generate-rust-bindings.py` with bindgen 0.72.1 regenerated
`sr_stage_abi.rs` (new `rsf_anomaly_sr_driver_config`; `configured_ini` and
`vendor_file` replaced by `file_exists` and `configured`; platform size
unchanged) and `native_observer_abi.rs` (`sr_selected` removed). A second run
changed no file. The hand-written motion table dropped `sr_capture`. Its
`MotionPlatform` ABI is now 4 (was 3) and its size 336 (was 344), with the C++
`static_assert` and Rust `const` assertions updated together. No public SDK,
Runtime or export change.

## Defects

None reproduced. The removed mirror could diverge in one unreproduced case. If
copying `StageOptions` threw after Rust had accepted the configuration, the
copy fell back to `{}`. The observer would then skip the SR install the Rust
stage expected. That path no longer exists.

## Verification

CPU, build and PE checks only. No device, game, visible window or
`eng/verify.ps1` ran. Logs are under `build/sr-configuration/` in the
worktree.

Rust 1.94.1 from `D:/Coding/ReScaleFrame/.local/rust-1.94.1-verification`
(`CARGO_TARGET_DIR=build/sr-configuration/cargo`):
`cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets --locked
--offline -- -D warnings` and `cargo test --workspace --locked --offline`
passed with 401 tests (overlay 57, Anomaly 76, orchestrator 249, upscaler 19).
The new Anomaly tests cover:

- the cold entry keeps only a selected configuration (null INI, stage 0,
  capture alone, read without apply);
- default and overridden directories from a `\\?\` INI path, with UNC and
  forward-slash overrides, the TemporalDriver options and the availability
  lookup including a partial Streamline package;
- the SRScale re-read from the kept INI after a reset, and none without an
  INI;
- the plain-directory cases moved from C++, plus surrogate preservation;
- the six panel entries;
- a Windows test against a real INI and files on disk, using
  `GetPrivateProfile*W` and `GetFileAttributesW`.

Mutation: dropping `plain_directory` from the directory policy failed the
directory test (`rust-mutation-no-plain.log`); restored.

MSVC 19.52 (Visual Studio 18 2026, SDK 10.0.26100.0), tree
`build/sr-configuration/native`. MinHook came from the coordinator's read-only
source override. The three yyjson files were copied from the coordinator's
`vendor/` after their SHA-256 values matched `yyjson.lock.json`:

```powershell
cmake --preset windows-x64-vs18 -B build/sr-configuration/native `
  "-DFETCHCONTENT_BASE_DIR=$PWD/build/sr-configuration/native/_deps" `
  "-DFETCHCONTENT_SOURCE_DIR_MINHOOK=D:/Coding/ReScaleFrame/build/windows-x64/_deps/minhook-src" `
  "-DRSF_ANOMALY_SR_VENDOR_ROOT=D:/Coding/ReScaleFrame/build/releases/rdr2/stage-rdr2-a2bcc0c3/ReScaleFrame"
pwsh build/sr-configuration/native-check.ps1 -Config Release   # native-check-release.log
pwsh build/sr-configuration/native-check.ps1 -Config Debug     # native-check-debug.log
```

Both configurations built the adapter, the patch pack and 17 fixture
executables without warnings. All 19 focused contracts passed in both:

- plugin, lifecycle and bootstrap;
- SR hooks on Standard and AVX;
- SR pass and its unknown selection;
- overlay API and input gate;
- SR Runtime API;
- motion and its unknown selection, motion hooks;
- input events on Standard and AVX;
- SR swap, reconstruction window and sun cascades;
- the shader tool.

The SR hooks fixture now configures through temporary INIs. It checks that
SRStage=0 is neither applied nor installed. It reads selection without
applying it, including SRStageCapture alone. It checks the colour transport
and signature keys through the real Win32 reads.

The FG, SR temporal, motion and overlay hooks device fixtures were built and
not run.

PE parity (`build/sr-configuration/pe-check.ps1`, lists in
`build/sr-configuration/pe/`), against staging
`D:/Coding/ReScaleFrame/.local/anomaly/staging/9367f1306570`:

- Debug and Release `ReScaleFrame.Game.Anomaly.dll` export the staged names
  at the staged ordinals: 1 `rsf_anomaly_observation`, 2 `rsf_anomaly_start`,
  3 `rsf_get_game_plugin_api`.
- Neither imports `ReScaleFrame.Runtime.dll`. Both keep the wide module name
  and every staged by-name lookup:
  - `rsf_get_d3d11_sr_api`
  - `rsf_dlss_pipeline_set_colour_transport`
  - `rsf_plugin_session_prepare` and `rsf_plugin_session_start`
  - `rsf_get_overlay_api` and `rsf_get_overlay_diagnostics`
  - `rsf_get_shader_patch_api`
  - `rsf_get_d3d11_fg_api` and `rsf_get_d3d11_present_api`

  The only extra `rsf_` string is `rsf_anomaly_shader_admitted`, which came
  with `7debba3`, after that staging.
- `motion.rsfpatch` from Debug and Release: 95,049 bytes, SHA-256
  `d2aa077a561ee71c084f806edb2f7cdcb0bf4ccaa17907a788324043f8622bb2`, equal
  to the staged pack.

## Gaps

No game run. Standard and AVX runs need to confirm, on this build:

- the Stage 5 cold start, with the vendor package beside the INI and with an
  explicit SRStreamlineDir;
- the overlay backend list and the six live options, applied and saved;
- the SRScale change after `vid_restart`;
- SRStageCapture refusing the motion capture;
- the observer installing SR only when selected.

## Coordinator integration and game checks

The Release worker DLL matched the exact adapter used by the Standard
default-path run. The worker branch and worktree were removed after
the merge and evidence archive. The merged configuration passed 401
pinned Rust tests and 16 focused MSVC Release contracts.
[Standard game evidence](rust-anomaly-sr-pass.md#package-relative-sr-paths-on-the-configuration-candidate)

An AVX opt-out run used the same worker DLL with SR, SRStageCapture,
motion, FG and the FG bridge off. It selected the AVX profile and
configured the overlay with zero SR and FG choices. The Windows
automation helper retained an active request and could not bring the
game forward. The background run registered the lifecycle callback
and reached its time limit before the actor update. The runner stopped
the isolated instance and restored the files. This run verifies the
recorded startup settings. Native gameplay and clean engine teardown
in this opt-out configuration still needed a foreground run. Logs are
in `rust-sr-configuration-optout-avx-346191e9` under the clone's
`evidence/` directory.

## Merged AVX opt-out retest

The foreground retest at `d0cc7ad` used the merged Rust configuration,
shader patch Runtime and exact AVX executable. SR, SRStageCapture,
motion, FG and the FG presentation bridge were disabled for the run.
The overlay offered zero SR and FG backend choices. Gameplay reached
the actor update and resized from 1280×720 to 1600×900. Insert opened
the overlay and the native D3D11 host drew it at the new size.

The desktop screenshot was black. The game's own captured 1600×900
backbuffer shows the rendered room, hands, HUD and overlay. The panel
shows upscaling unavailable and FG off. Draw counters showed zero
motion-window draws and the engine ran its scripted quit, destroyed
Direct3D and returned code 0. The runner restored all staged files.
The backbuffer PNGs, their hashes, INI, engine log and payload hashes
are under `.local/anomaly/staging/live-58fe95a-stage5/evidence/` in
`merged-optout-avx-d0cc7ad`.
