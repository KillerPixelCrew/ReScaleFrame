# Anomaly native HookSet in Rust

This port starts from `afaf89566521d`. The Standard and AVX 1.5.3
fingerprints, RVAs, exact guards and xray-monolith correspondence are in the
existing profile evidence. The port uses the same observer, geometry, SR
and motion site tables and their caller-specific guard checks.
[Standard evidence](anomaly-native-hooks.md) · [AVX evidence](anomaly-avx-profile.md) · [Observer lifecycle](rust-anomaly-native-observer.md)

Rust admits batches of one to 24 sites against a PE32+ AMD64 image of at
least 4096 bytes. It checks the PE offset, exact image size, each 16-byte RVA
range, the bytes and duplicate targets before any MinHook registration.
Win32 leaves return typed DOS and NT facts and copy bytes through the
observer's SEH guard. Each read still needs one committed, readable region
owned by the module allocation. Instruction reads also require executable
protection. The observer's build hash and longer guards remain checked at
prepare, and geometry, SR and motion keep their existing stage guards.

MinHook `c3fcafdc1014` keeps a single pending queue per linked instance.
`MH_ApplyQueued` can stop after patching part of the batch, and its thread
freeze can move a suspended instruction pointer into a trampoline before
patching. The older enable retry could leave queued enables after a refusal,
and an incomplete disable could leave the old enabled flag set. Rust
serializes queue/apply across the sets, queues all targets off on an enable
refusal, and marks a failed disable as no longer known to be fully enabled.
Registrations from a failed create are removed before any enable attempt.
After that boundary their original slots and MinHook trampolines survive
until process exit under the existing adapter pin. The four D3D11 motion
Draw detours use the same transaction with their existing COM slot reads,
so their apply cannot consume a partially queued engine batch.

## Verification

Rust tests inject create, queue and apply failures. They check cancellation
of queued enables, a partial apply followed by a failed rollback, recovery
on the next enable after a failed disable, and unchanged original slots
after publication. They also check minimum image size, both count limits,
PE facts, exact RVA boundaries, duplicate sites and all sixteen guard bytes.
Two isolated reruns restored the old enabled-flag and enable-retry behavior
one at a time. The retry regression failed at the expected assertion in
each run.
The native CPU fixture checks mapped page ownership, protection and
region boundaries, real MinHook rollback on an existing registration, and
a detour that calls its original after another thread restores the target.
It maps each build's observer, motion and SR detour prologues to check
refusal on a changed guard and installation and restoration on an exact
match. The geometry CPU fixtures cover the corresponding geometry tables.

MSVC 19.52 x64 passed 18 focused CPU contracts in each of Debug and
Release. Rust 1.94.1 passed formatting, workspace Clippy with warnings
denied and all 339 locked workspace tests. Both DLLs retained the baseline
export names and ordinals and resolve the session entry points dynamically
from the carrier-loaded Runtime. Live Standard and AVX gameplay and device
validation remain open.
