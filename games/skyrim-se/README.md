# Skyrim Special Edition

Exact-build native adapter for Steam SkyrimSE 1.7.104.0. It does not use SKSE.
Current code implements guarded pass-through observers at the native
temporal-jitter producer and main pre-HDR post-processing boundary. It does not
implement SR, FG or latency control.

The generic XInput carrier loads `ReScaleFrame/ReScaleFrame.Runtime.dll` and
asks the runtime to select, prepare and start `ReScaleFrame/ReScaleFrame.Entry.dll`.
For the current development deployment, Entry.dll is the built Skyrim game DLL.
Executable detection, expected bytes and native state offsets stay in this adapter.
The carrier has no game detection or engine hook policy.

The adapter checks the complete on-disk executable SHA-256 and the loaded jitter
and post-processing entries' expected bytes before creating disabled hooks. Start
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

See [research](../../docs/research/skyrim-se-integration.md) for fingerprints,
the source/binary evidence, deployment receipt location and remaining work.
