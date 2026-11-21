# Anomaly native geometry in Rust

The geometry migration from `f693b02` follows the existing Standard and AVX
evidence at xray-monolith `028ccf5`. The executable identities and sizes are
recorded in [engine.json](../../games/anomaly/engine.json). The
[geometry draw coverage](anomaly-geometry-coverage.md) and
[AVX profile](anomaly-avx-profile.md) identify the actual model pool, visual
entry, constant flush, view and viewport callbacks. This change keeps their
17 entry guards, seven layout guards and longer unique prefixes in the
native profiles.

| Producer | Ownership at the call boundary |
| --- | --- |
| Model create, child create, duplicate and delete | Rust tracks nested factory calls, walks guarded owned-child ranges and uses the existing generation registry. A pooled hierarchy retires before re-registration. Delete retires before the original call, and a bad hierarchy disables capture. The existing retirement queue still carries keys to the motion owner. |
| `CRender::Render`, HUD, sorted and emissive | Rust binds the sampled view and projection domain to nested scopes on the calling thread. The view predicate uses the guarded active byte, frame delay and device frame number. The selected sorted or emissive call owns its near interval. |
| Visual entry, phase and foliage | Rust owns the entry classification and stable visual facts. The native leaves read the selected profile's phase, world, skeleton layout, detail window boundary and progressive tree LOD. |
| `CBackend::constants.flush` | Rust routes the draw facts and counts captures. The copied world and verified hardware palette come from the draw-time engine data. The facade retains that copy across the original flush and supplies it to the same motion callback after the original returns. |
| Install and disable | Rust serializes capture epochs and keeps the module and profile paired. Native MinHook verifies and installs the x64 ABI detours and retains their trampolines. An install refused after activation retires the candidate epoch. |

The native stack guards keep scope storage pinned for each call, including
native unwinding. Rust holds only same-thread scope links. Engine addresses
stay opaque across the private ABI. World and bone transforms are native
row-vector float matrices. Rust verifies finite affine values and transposes
them to the column-vector representation. Hardware-skinned draws use the
verified bone instance order and palette layout. The snapshot owns copies of
those values. The live world, skeleton and staged vertex constants keep
their guarded native readers.

The routing continues to use the existing
[Rust motion history](rust-anomaly-motion-history.md) and the
[foliage](anomaly-foliage-motion.md) and [tessellation](anomaly-tess-motion.md)
contracts. Geometry supplies the view, projection domain, visual and draw
facts. Motion still decides history and shader use. The SR shared `rmNear`
callback still runs after the original; the projection window has been
opened before the callback. Geometry owns its counters and periodic coverage
text. The migration introduces no shader, public ABI or profile changes.

## Regressions

The production-linked geometry hook fixture simulates the failing sequences
with the real x64 detours redirected to local fixture functions. Each of
these three checks failed in an isolated probe built with the C++ geometry
owner from `f693b02`, for Standard and AVX independently.

| Sequence | Corrected behavior |
| --- | --- |
| An identified main-pass visual flushes in shadow or an unknown phase. | The callback receives the flush phase and the original entry classification, with capture and history delivery refused. |
| A Scope first-person interval contains a nested Main render and emissive first-person interval. | The inner `rmNormal` restores Main/World. The child render exit restores Scope/FirstPerson, and the outer `rmNormal` restores Scope/World. |
| A nested render has no readable view producer while the parent is Scope. | The nested render has an unknown view and the World domain. Its HUD inherits the unknown view. The parent view returns after the nested render. |

The fixture also changes the engine world during the original flush. The
post-flush callback and draw sink receive the same earlier copy, and the
callback observes that the original flush ran first.

## Verification

The worktree-local MSVC 19.52 builds used a worktree-local FetchContent
directory and the pinned MinHook checkout read-only. The initial yyjson
download failed its hash check. The worktree copy was then populated from
the pinned dependency used by the coordinator and passed the CMake hash
check.

MSVC Debug and Release each passed 35 focused CPU contracts. They cover
both profiles' geometry, SR, input hooks and input-event facts, as well as
the registry, motion, lifecycle and bootstrap. The shader-tool contract
includes four skipped cases that require the licensed vanilla shader fixture.
Both builds also produced the nine motion, SR and FG device fixture binaries.
Cargo formatting, locked workspace Clippy with warnings denied and all 256
locked Rust tests passed.

Both adapter builds match the staged C++ DLL from `8322179a36c7`. MSVC
`dumpbin /exports` gives ordinal 1 for `rsf_anomaly_observation`, 2 for
`rsf_anomaly_start`, and 3 for `rsf_get_game_plugin_api`. Binary strings
matched the same 18 names exported by the staged Runtime and used for lookup,
including session, SR, FG, presentation, overlay, shader-patch and input-dump
calls. The imports and
UTF-16 module string still identify the dynamic lookup of
`ReScaleFrame.Runtime.dll`.

Both rebuilt rules-only motion packs contain 20 rules and match the staged
pack byte-for-byte with SHA-256
`d2aa077a561ee71c084f806edb2f7cdcb0bf4ccaa17907a788324043f8622bb2`.
Device execution and moving-game acceptance remain open for this cutover,
including scope transitions and active motion coverage under real draws.

## Profile guard admission

Rust now owns the geometry install guard sequence with the shared
checked-image reader in `engine_guards.rs`. The native leaf lends the
selected Standard or AVX profile's seven layout sites, 17 detour entries,
nine unique prefixes, `SizeOfImage` and the guarded copy leaf. The tables
stay in `geometry_profile.h` and `native_profile.h`. Rust refuses a null
module before borrowing the profile, then a size mismatch. It then checks
each layout site and its unique prefixes before any detour entry, and each
detour entry the same way. The first mismatch refuses before HookSet writes
anything. The typed trampolines, HookSet, guarded engine reads, hook site
table and `install_native_geometry` are unchanged. `engine_guards` is now
compiled on every target because the geometry owner is.

Source comparison against the C++ guard at `4ad9c5e9` matched the C++ order: module, size,
layout sites, then detour sites, with the 16 expected bytes before the
unique guards at the same RVA and the guards in table order. Traced against
`tests/anomaly_geometry_hooks.cpp`: the null module, other-build profile
and blank image refusals, each flipped unique-prefix end, layout and entry
byte, the untouched entries after a refused install, and the guarded
install, restore and re-enable. Pinned Rust 1.94.1 formatting, locked
offline Windows Cargo metadata and Git whitespace checks passed. Compile,
link, Clippy, tests, fixtures and game verification remain user-owned.

## Vertex constant-buffer admission

Compared with `fab9fe5a`, the Rust geometry owner now checks the
slot, reflected size and requested span before resolving the staged
vertex constants. The native leaf still supplies the selected geometry
profile, including the table RVA, 14 slots and the size and data
offsets at `0x58` and `0x60`. The same guarded reads obtain the object,
its size and its data in that order. The existing read and write
helpers still use the guarded native copy.

A source comparison matched the profile facts, the 24-byte record,
all 512 modeled range cases and both native consumers. Checked address
arithmetic refuses a wrapping span before a guarded copy. Pinned
formatting, locked Windows Cargo metadata and whitespace checks
passed. The existing geometry hook fixture and game checks remain
unrun.
