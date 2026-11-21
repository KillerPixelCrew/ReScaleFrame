# AC7 game entry in Rust

Date: 2026-11-21. Base: `rust/orchestrator-anomaly` at 90f0db8.

## Question

Which part of the AC7 native adapter can move to Rust first without changing the two combined
DirectInput carriers, the native renderer or any accepted game path?

## Inventory

`games/ac7` builds one DLL and six static libraries:

| Target | Sources | Consumers |
| --- | --- | --- |
| `rsf_game_ac7` (`ReScaleFrame.Game.AC7.dll`) | `plugin.cpp` | loaded by both carriers through `rsf_plugin_session_prepare`; `game_plugin_contract` CTest |
| `rsf_ac7_render_scope` | `render_scope.cpp`, `native_renderer.cpp`, `truesky_depth.cpp`, `truesky_motion.cpp`, `contact_shadow.cpp`, `velocity_shader.cpp` | the game DLL; `rsf_ac7_velocity_pass`, and through it both carriers |
| `rsf_ac7_view` | `view_uniforms.cpp` | the game DLL and both carriers |
| `rsf_ac7_ui_rules`, `rsf_ac7_scene_color`, `rsf_ac7_motion_capture`, `rsf_ac7_velocity_pass` | one source each | linked into both carriers |

Before this change `plugin.cpp` held the exact build (`Ace7Game.exe`, SHA-256
`c7da97f5f8a807d4f1264adbb074146fcffe9bdc2ffa98791b822cd28e558f4f`, AMD64), the GameSDK table
and a five-state lifecycle around `rsf_ac7_native_renderer_*`. It is the only AC7 code that
depends on nothing but the GameSDK and the native renderer's C entry points, so it is the
coherent first owner.

The combined carriers are `rsf_proxy_dinput8` (`dinput8.dll`, `ReScaleFrame.Loader.dll`,
`carriers/version.dll`) and `rsf_proxy_ac7` (`ac7/bin/<Config>/dinput8.dll`). Both compile
`dinput8_proxy.c`, `dlss_bridge.c` and `preferences.c` and link the AC7 static libraries above;
the shared one also carries the version shim whose `DllMain` routes `Ace7Game.exe` to
`rsf_ac7_proxy_process_event`. Neither links `plugin.cpp`; they load the game DLL by path.

## Moved

The new crate `rsf-ac7` (`games/ac7/Cargo.toml`, staticlib linked into `rsf_game_ac7` through
`rsf_add_rust_static`) owns:

- `detect`: probe size, AMD64, ASCII case-insensitive exact name and hash. Null strings refuse.
- `rsf_ac7_publish_api`: output size, ABI 24, the `ac7` identity, name, Cargo version (equal to
  `VERSION`), `rendering_ready` 0 and the former status text.
- `lifecycle.rs`: one lock over handle, transition flag, activation flag and reason; transitions
  run their native leaf outside the lock. Prepare validates args and host and requires a
  session, refuses `BUSY` while a renderer or transition exists, builds the native options
  (pending capacity 256, render pass, log, user, render config, CPU event, scope state,
  diagnostics 0) and keeps the native output even when preparation is refused, which the pinned
  partial-activation path relies on. Start, quiesce, stop and status keep their checks, result
  codes and reason texts, including `deactivated after command ownership refusal`.

`plugin.cpp` now keeps the exported `rsf_get_game_plugin_api` and a constant leaf table
(`ac7_platform.h`) that wraps each `rsf_ac7_native_renderer_*` call in `try`/`catch`. A throw
returns false and the lifecycle answers `RSF_ERROR_NOT_READY` without running its finish step,
as the former catch did. `platform_abi.rs` mirrors the header; `tools/generate-rust-bindings.py`
has the targeted entry.

Unchanged: DLL name and export, contract test expectations, carrier probes and exports, linked
static libraries, native renderer behaviour and the Runtime's SR, FG and pacing ownership. The
crate makes no vendor, latency or presentation decision.

## Remaining AC7 owners

In `games/ac7`, still C++:

- `native_renderer.cpp`: entry and helper byte sites, enlarged UI depth/view gates, MinHook
  install and removal, scene-precision, velocity, TrueSky cloud resolution, depth and motion
  relays, input hook, controller facts, drain and pinning. Next coherent candidate: the site
  tables and expected bytes as data owned by Rust, with MinHook and code writes as leaves.
- `render_scope.cpp`, `view_uniforms.cpp`, `ui_rules.cpp`, `scene_color.cpp`,
  `motion_capture.cpp`, `velocity_pass.cpp`, `velocity_shader.cpp`, `truesky_*`,
  `contact_shadow.cpp`.

In `loader/proxy`, game-facing and shared by both carriers:

- `dinput8_proxy.c`: `rsf_ac7_proxy_process_event` (vectored handler, capture support,
  `dump_worker`), the decryption wait, `apply_jitter_patch` (+112B1F3),
  `apply_translucent_velocity_patch` (+11823DE), `apply_translucency_depth_patches`
  (+1168F6F, +1097A0C, +109D03A, +109D061, +11583A6 and the six +10xxxx comparisons),
  `apply_separate_translucency_patch` (+10BE329), console singleton +3A8B290 settings,
  `prepare_native_game` (DLL path and executable hash), `accept_ac7_window` (`UnrealWindow`,
  same process, then native preparation), `start_generation` (Unreal units, reversed infinite
  depth, `RSF_AC7_ENGINE_*` metadata), `%LOCALAPPDATA%\ReScaleFrame\AC7` paths, `AC7.ini`
  preferences, overlay actions and the crash report.
- `dlss_bridge.c`: `rsf_bridge_prepare_game` (hard-coded `Ace7Game.exe` probe, host services),
  native pass, scope and CPU routing, Unreal velocity decode, the D3D binding matcher and
  promotion fallback, UI classification and chain tracking.
- `dlss_bridge.h`: `RSF_AC7_ENGINE_TYPE`, `RSF_AC7_ENGINE_VERSION`, `RSF_AC7_PROJECT_ID`.
- `shim_loader.cpp`: the `Ace7Game.exe` branch of the version shim `DllMain`.
- `overlay_host.cpp`: generic by design, hosted by the AC7 carrier.

These carriers also hold Runtime-side mechanisms (presentation, overlay, FG start) that belong in
the orchestrator under AGENTS.md; moving them is separate from the adapter port.

## Checks

Source review against the removed C++; `rustfmt --edition 2024 --check` (rustfmt 1.11.0, the
pinned 1.94.1 toolchain is not installed); `cargo metadata --locked --offline`; `engine.json`
parse; `git diff --check`. bindgen-cli 0.72.1 and libclang are not installed, so the targeted
generator entry was not run and `platform_abi.rs` is hand-mirrored, with Rust size/offset
assertions and C++ `static_assert`s on the table and the options. Not compiled, linked,
Cargo-tested, Clippy-checked, CTest-run, deployed or game-tested.
