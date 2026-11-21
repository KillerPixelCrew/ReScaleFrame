# Anomaly input facts and overlay input gate in Rust

This note records the move of `input_hooks.cpp` state and `overlay_input_gate.cpp` into
`games/anomaly/src/input.rs`, the one module this step adds to the `rsf-anomaly` static library.
It is a source port checked by CPU contracts. No game, device or window was run for it. The
baseline it must match is the C++ adapter at `8322179a36c7`
([baseline note](rust-anomaly-cpp-baseline.md)).

## Ownership after the port

| Concern | Before | Now |
| --- | --- | --- |
| `CInput::OnFrame` admission (`active`), in-flight count, `idle()` for quiesce | `input_hooks.cpp` atomics | `input.rs` (`rsf_anomaly_input_frame_enter/exit`, `_activate`, `_idle`) |
| Polling thread (`thread_local` set while an admitted `OnFrame` runs, restored on exit) | `input_hooks.cpp` | `input.rs`, carried through the enter/exit ticket |
| Received-event routing: device kind to `RSF_GAME_INPUT_KEYBOARD`/`_MOUSE`, only on the polling thread while active | bootstrap lambda plus `input::received` | `input.rs` (`rsf_anomaly_input_route`); bootstrap passes `input::received` directly |
| Gate device table, capture edges, flushes, synthetic releases, exclusive-mouse hand-off, `hardware_data` fact | `overlay_input_gate.cpp` | `input.rs` (`rsf_anomaly_input_gate_*`) |
| Site and complete-body guards, vtable slot 0 check, `VirtualProtect` slot exchange | `input_hooks.cpp` | unchanged native leaves in `input_hooks.cpp` |
| Detour that reads the engine frame counter, calls `fg::input_begin` and the original | `input_hooks.cpp` | native leaf; RAII exit keeps the in-flight count correct if the engine unwinds |
| DirectInput vtable discovery, MinHook trampolines, `calls_for`, observer call | `overlay_hooks.cpp` | unchanged (out of scope) |

`overlay_input_gate.h` keeps the `InputGate` class, `DeviceKind`, the capture bits and the
`DeviceCalls` table, so `overlay_hooks.cpp`, `overlay_service.cpp` and the fixtures compile
unchanged. The class now owns one Rust gate and forwards each method. The header asserts the
DirectInput values the Rust side hard-codes (`DIDEVICEOBJECTDATA` size 24 with the timestamp at
8, `DIMOFS_BUTTON0..7` = 12..19, `DIGDD_PEEK`, `DISCL_EXCLUSIVE`, `DIERR_INPUTLOST`,
`DIERR_NOTACQUIRED`) against `dinput.h`.

## Behaviour kept

The port is line-for-line in decisions: the same lock scope around device calls, the same flush on
both capture edges, the same release order (lowest code first) and capacity and peek handling, the
same acquire refusal and reacquire rules, and the same pass-through for untracked devices and
NULL-data engine flushes. Synthetic releases never set `hardware_data`, and no input event is
reported outside an admitted `OnFrame`, so the gate does not create latency facts. Rule 3 placement
is unchanged: the adapter reports input sampling and received records; fg_stage forwards them to
the Runtime CPU event API, which owns their meaning.

Two implementation choices differ from the C++ without changing results. The device table is a
`BTreeMap` rather than a hash map, which avoids seeding a hasher from the OS RNG for a table of
at most a few devices. The C++ gate could throw from `std::mutex` or allocation into the
`try/catch` in `overlay_hooks.cpp`; the Rust gate recovers a poisoned lock and otherwise cannot
fail except on allocation, which aborts.

No defect was found in the ported code, so no fixture was added. The input-hook fixture still does
not call the installed detour, because the synthetic image's original is not executable; the
admission and routing path is covered by the live game run only.

## Checks

Built the full MSVC Debug and Release trees (Visual Studio 18 2026) in a private tree with NVAPI
diagnostics off, because the root's NVAPI header path no longer exists. Neither build produced
errors or warnings.
The following contracts link the production Rust library and passed in both configurations:
`anomaly_input_hooks_standard_contract`, `anomaly_input_hooks_avx_contract`,
`anomaly_overlay_input_gate_contract` (the scripted gate), `anomaly_input_events_standard_contract`,
`anomaly_input_events_avx_contract`, `anomaly_sr_hooks_contract`, `anomaly_sr_hooks_avx_contract`,
`anomaly_plugin_contract`, `anomaly_lifecycle_contract`, `anomaly_bootstrap_contract`,
`anomaly_sr_profile_contract` and `latency_input_event_contract`. `cargo fmt --all --check`,
`cargo clippy --workspace --all-targets --locked -- -D warnings` and
`cargo test --workspace --locked` (216 tests) passed.

The export table of `ReScaleFrame.Game.Anomaly.dll` matches the staged `8322179a36c7` DLL by name
and ordinal (`rsf_anomaly_observation` 1, `rsf_anomaly_start` 2, `rsf_get_game_plugin_api` 3).
Neither DLL imports `ReScaleFrame.Runtime.dll`; the adapter still borrows the Runtime the carrier
loaded. The Release DLL adds two system imports compared with the baseline, `ntdll.dll` and
`api-ms-win-core-synch-l1-2-0.dll`, from the Rust standard library's lock (`WaitOnAddress`). With
the hash map, the Debug DLL also imported `bcryptprimitives.dll`. The Release DLL built with
`BTreeMap` does not.

Not run here: `anomaly_overlay_api_contract` and `anomaly_overlay_hooks_contract` create windows
(and the second a WARP swap chain and system DirectInput devices), and these two were
built but not executed. The overlay visibility fixture is registered only with a built panel DLL,
so it was not configured here; its CMake linkage was updated to the Rust gate without a build. The port still needs the
game run on Standard and AVX: panel open and close with held keys and mouse look, and FG input
facts.

The coordinator kept the original timing of the polling window. The native
detour reports the engine frame before starting the Rust polling scope.
A Rust regression checks that an admitted call cannot route buffered
records before polling starts, that nested polling restores the outer
scope, and that retirement stops routing while an in-flight call drains.

## Verified input entry and buffered receipt

Compared with `fc706ca9`, the Rust input owner now selects the
`CInput::OnFrame`, `KeyUpdate`, and `MouseUpdate` guards in order.
The complete OnFrame body check follows the three prologues. The
shared slot owner then checks and exchanges primary slot 0 using the
same Standard or AVX profile. The refusal reason is unchanged on
those paths. The native detour lends the guarded read and frame
report callbacks. Rust reads the selected engine counter, reports
the boundary and opens polling in that order. The original call and
unwind cleanup stay on the native frame.

The buffered-read gate now receives the native result, count, peek
flag and hardware fact after the device call. It admits only
successful, nonempty hardware reads that consumed records. The
observer runs after the device lock is released. The polling scope
still decides whether that device kind becomes a GameSDK input
event. The Runtime continues to own pacing and marker semantics.

The private profile and buffered-read records are 112 and 32 bytes
on x64. The existing Standard and AVX fixture now checks the guard
refusal reasons. A Rust fixture covers hardware, peek and failure
cases. Source comparison, pinned formatting, locked offline Cargo
metadata and whitespace passed. The updated native build, fixtures,
held-key gameplay and overlay release remain open.
