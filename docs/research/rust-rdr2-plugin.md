# RDR2 game plugin in Rust

This note covers the first RDR2 Rust slice: the production target `rsf_game_rdr2`
(`ReScaleFrame.Game.RDR2.dll`) moved from `plugin.cpp`, `render_camera.cpp`,
`native_renderer.cpp` and `scene_boundary.cpp` into the `rsf-rdr2` static library. It is a source
port. Nothing was built, compiled, linked or tested for it, and no game, device or window run was
made. The ASI (`rsf_asi_rdr2`) and the attach helper (`rsf_rdr2_attach`) stay C++ for a follow-up.

## What moved

| Former C++ | Rust module | Owns |
| --- | --- | --- |
| `plugin.cpp` | `lib.rs` | `rsf_get_game_plugin_api` contents, `RDR2.exe` + SHA-256 + AMD64 recognition, call-structure size/ABI validation, `session_id` check, plugin info strings |
| `render_camera.cpp` | `camera.rs` | worker-view decode at `+0x40..+0x680`, `+0x671` active flag, jitter-match check, jitter removal from `+0x80`/`+0x180`, double-precision inverses and reprojection, refusal on non-finite or singular input |
| `native_renderer.cpp` | `renderer.rs`, `layout.rs` | image identity (`0x65eec92e`, `0x7397600`), `+26f3a3c`/`+26c97d4` entry bytes, the 0x168-byte DLSS descriptor, TLS worker view (`+5b403e0`, block `+14d8`/`+61c`), frame counters `+542904c`/`+5429058`, frame time `+59b393c`, NGX `GetI` slot 11 creation flags, refusal reasons 1 to 7 with their messages and dedupe, per-feature history and `native_gap` reset, SR pass identity and transitions, FSR2 2.1.2-prefix normalization and engine states, `ffx_fsr2_api_x64.dll` identity and export bytes, caller report, game-submit return `+26df79b`, swapchain global `+571c7e8`, queue/Present observer attach, lifecycle ordering and busy rules |
| `scene_boundary.cpp` | `scene.rs`, `layout.rs` | blit/marker/output/sleep/mode entry bytes, `+26b4250` texture accessor bytes, the three `call +5f1e08` sites and their returns, blit return `+5c11b9`, draw-context colour view and resource chain, marker ring and required mask, scene ring and 16-blit limit, final-scene and window passes, SDK sleep/mode latency reports, input-source framing and input event flags, hook rollback, start/quiesce/stop order |

`plugin.cpp` keeps the `rsf_get_game_plugin_api` export, so the DLL name, export and ABI 24
contract are unchanged. It lends the leaf table in `native_platform.cpp` to
`rsf_rdr2_publish_api`. The table carries only typed leaves: `ReadProcessMemory`, module and
export lookup, `gs:[0x58]`, QPC, caller module path, MinHook create/enable/disable/remove, four
detour shims that capture `_ReturnAddress()` for the pop, postfx output, blit and FSR dispatch
sites, D3D12/DXGI COM calls (queue type, table clone, swapchain identity, resource description,
transition barrier, swapchain description and buffers, back-buffer index, release), and the
window-message hook and `XInputFacts` observation. The crate imports no native symbol, so the
camera fixture links it without the plugin's leaves. Both sides assert the table's size and
offsets; bindgen-cli was not available, so the table is hand-written like the Anomaly hook tables.

`render_camera.h` now declares the crate's camera C ABI (`rsf_rdr2_decode_render_camera_abi`) and
an inline wrapper with the old signature, so `tests/rdr2_camera.cpp` is unchanged. Its CMake target
links `rsf_rdr2_rust` instead of compiling `render_camera.cpp`. The Rust tests restate that fixture
and cover detection, publication, lifecycle refusals before preparation, FSR normalization, call
target checks, scene identity and SR pass construction.

## Preserved behaviour

The host receives the same calls in the same order: `log`, `render_feedback` (reason, session,
frame), `process_upscale` between the same transitions, `submit` and `present` brackets,
`render_pass` for the final scene (begin 0, the recording command list) and the window
(begin 1, null list), `process_latency` for markers 0 to 6, sleep and mode, and `cpu_event`
input facts. A host refusal of sleep or mode still runs the native routine; an accepted marker
still skips it. Every refusal still returns to the game's own DLSS or FSR2 call for the frame and
resets RSF history, and the FSR2 fallback still writes only the shared-prefix reset byte of the
game's own description. Prepare still returns `NOT_READY` for a host without upscale, submit or
Present services or for another image, `NATIVE_REFUSED` for different entry bytes, and stop still
returns `BUSY` once a queue or swapchain observer is attached. `rendering_ready` stays 0.

## Deliberate differences

- Entry bytes and PE headers are copied through `ReadProcessMemory` before comparison instead of
  read with `memcmp` in place. Values are the same; an unmapped page refuses instead of faulting.
- A null NGX `GetI` entry now refuses SR for the frame instead of calling address 0.
- The scene producer shares the renderer's single retained `IDXGISwapChain4` reference instead of
  taking a second `IDXGISwapChain3` reference. Neither was released before stop could succeed.
- The window-message leaf counts its own in-flight hook procedures, including `CallNextHookEx`,
  and quiesce/stop include that count. The former reader scope covered the same span.
- The input source is cleared at the input-sample marker with compare-exchange instead of a
  separate load and store.
- The FSR2 caller report converts the module path to UTF-8 with `WideCharToMultiByte` instead of
  `%ls`.
- The plugin version string is the Cargo package version, which `eng/verify.ps1` keeps equal to
  `VERSION` and therefore `RSF_VERSION_STRING`, as in the Anomaly plugin.

## ASI and attach targets

`rsf_asi_rdr2` and `rsf_rdr2_attach` share no source with the plugin. Their shared inputs, left
intact, are `runtime_setup.h` (also `tests/unity_sr_d3d12.cpp`), `native_capture.cpp` (also built
into `tests/rdr2_capture.cpp`), `engine_settings_policy.h` (`tests/rdr2_engine_settings.cpp`), and
the ASI's build-order dependency on `rsf_game_rdr2`. Remaining RDR2 adapter work: `asi_entry.cpp`,
`social_club.cpp`, `camera_probe.cpp`, `native_capture.cpp`, `script_input.cpp`,
`native_settings.cpp`, `engine_settings.cpp` and `attach_entry.cpp`.

## Known limitations

- Unbuilt and untested. MSVC, Cargo build, Rust tests, Clippy, CTest, the plugin contract and the
  camera fixture are user-owned verification for this change.
- The queue and swapchain observers still pin the module until process exit, as before; no
  queue-drain or unhook protocol exists.
- The SR route, FSR2 route and scene producer keep their earlier acceptance scope; this port adds
  no game evidence.

## Checks performed

Source review against the removed C++; `rustfmt --edition 2024` (rustfmt 1.11.0 from Rust 1.100;
the pinned 1.94.1 toolchain is not installed here); `git diff --check`; `cargo metadata --locked
--offline` to confirm the lockfile entry resolves unchanged; a JSON parse of `engine.json`.
