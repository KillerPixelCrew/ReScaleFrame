# Skyrim Special Edition

Native adapter for Steam SkyrimSE, researched on 1.7.104.0. It does not use SKSE.
Current code implements guarded pass-through observers at the native
temporal-jitter producer and main pre-HDR post-processing boundary. It does not
implement SR, FG or latency control.

The generic XInput carrier loads `ReScaleFrame/ReScaleFrame.Runtime.dll` and
asks the runtime to select, prepare and start `ReScaleFrame/ReScaleFrame.Entry.dll`.
For the current development deployment, Entry.dll is the built Skyrim game DLL.
Executable detection, site patterns and native state offsets stay in this adapter.
The carrier has no game detection or engine hook policy.

The adapter finds its sites by signature, not by build: admission scans the loaded
executable once and resolves every hooked function, patched call, return address and
read global from a pattern that is unique in its code sections, with relocations and
displacements as wildcards (`src/discovery.rs`). Each site's pattern is matched again
in live memory before its disabled hook is created or enabled. A missing, ambiguous or
changed site refuses admission and the prepare log names it. The executable is
recognized by name; its SHA-256 is computed for the host setup and logs only. Start
enables them; quiesce stops observations; stop disables them and retires host
services after callbacks drain. The pinned adapter/trampoline remain process-lived. Native calls
are preserved once, and at most sixteen samples per observer are logged per session.
No diagnostic option or INI-only switch is introduced.

The 15 November development run reached a responsive game window and recorded all
sixteen native samples without SKSE. This validates loading and state access, not
image quality, actor motion, FG, latency or mod compatibility. Community Shaders
currently causes activation to refuse until ownership handover is implemented.
`rendering_ready` remains zero. A subsequent bounded starting-cell run also
recorded sixteen post-processing calls and native colour/motion-candidate texture
descriptors. This verifies descriptor access, not pixel contents or vector units.
The probe also logs four main-camera cache/projection/history samples and the
native main-depth descriptor. The first sample has zero previous-history components;
no vendor inputs or input-frame identity are inferred from these logs.

Native update begin/end and render-entry CPU facts reach the runtime's shared
event cache. Input polling is hooked, but gameplay polling ran on worker threads
outside the update's thread-local scope in the bounded run. Worker handoff and
input-to-simulation ownership are not verified; the runtime uses explicit missing-
input substitutes. No vendor pacing or low-latency acceptance is claimed.

The Rust crate `rsf-skyrim-se`, linked into the same DLL, owns the GameSDK
identity and observer admission (`src/plugin.rs`), the lifecycle with its
process-lived Runtime session, DPI startup and subsystem order (`src/session.rs`),
the observer hook transactions (`src/observers.rs`) and the cold native creation
root with its Steam Present cooperation (`src/creation.rs`, `src/layout.rs`).
`src/plugin.c`, `src/native_creation.c` and `src/render_detours.c` keep the typed detours, native
leaves and the C render, CPU and compatibility subsystems. See
[the migration note](../../docs/research/rust-skyrim-native-creation.md) for the
moved behaviour and the remaining Skyrim owners.

The Community Shaders 1.9.1 and SSE Display Tweaks 0.5.25 profiles remain exact
third-party module profiles. Discovery has been verified against the installed
executable (`discovery::tests`), not yet inside a game run.

See [research](../../docs/research/skyrim-se-integration.md) for the discovery
evidence, the source/binary evidence, deployment receipt location and remaining work.
