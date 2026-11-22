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

## Child-range admission

Compared with `ef38645b`, the Rust geometry owner now selects the
owned child ranges that it walks before model reuse and retirement.
The native leaf lends the selected build's four hierarchy vtable
RVAs, child-vector offset and guarded reader. Rust reads the object's
vtable, checks the borrowed flag for a recognized hierarchy, and
reads the three-pointer range only for an owned vector. The existing
range validation, walk bound and capture-fault behavior remain in
the same owner.

Source comparison matched the four RVAs, guarded read order, 32-byte
profile layout and all 16 private slots. The geometry fixture now
distinguishes an unreadable borrowed hierarchy from a malformed owned
one. Pinned formatting, locked Windows Cargo metadata and whitespace
passed. The fixture and game checks remain unrun.

## Geometry reads and palette transfer

The N03 pass compared the geometry path at `8adf010b` with the existing
Standard and AVX evidence above. Rust now checks address arithmetic and
interprets phase, viewport activity and delay, device frame, world,
skeleton mode, bone count and instances, and progressive tree LOD.
Native code lends four selected RVAs and guards copies. Phase fallback,
active-view predicate, world sample timing, supported skin modes,
palette layout tag and bone order match the prior path.

The C++ snapshot owner lends its vector once. Rust reads up to 64
native matrices per callback, validates every affine transform and
writes into the exclusive destination span. At the 1,024-bone limit
this is 16 read calls and one storage loan. The finish call clears
all bones and the layout tag on a failed read, invalid affine lane
or allocation refusal. The copied world remains available. Geometry
and motion share the row-to-column transpose and widening operation.
The geometry-only affine check remains separate from motion camera
conversion. The geometry, motion and SR readers share the nullable
empty slice helper with profile and draw-callback borrow lengths.

The native hook fixture covers a 65-bone copy and rejection with a bad
matrix at index 64 on both profiles. The mixed geometry fixture checks
a wrapping instance range, and the Rust cases cover the maximum batch
count, late read refusal, visual lookup and the 19 classification
vectors moved from the native harness. The fixture-only retirement
queue adapter moved out of the production compilation unit.

The native leaves still have concrete callers. The geometry detours
invoke C++ member trampolines and pin Rust's scope storage in native
RAII guards, including engine unwinding. The copy leaf catches
inaccessible engine memory with SEH on MSVC. The profile tables are
still consumed through the shared C++ build profile by geometry,
SR and the evidence tooling. The snapshot bridge lends the existing
C++ optional world and vector to the native motion-stage caller.
The registry and retirement headers expose the native fixture and
callback records, and keep C++ exceptions on the native side.
The native fixtures still cover the actual MinHook trampolines,
platform memory and C++ snapshot/history bridge.

Source checks compared the nine private callback slots, the two
writer slots and the unchanged Standard and AVX profile bytes.
Pinned Rust 1.94.1 formatting, locked offline Windows Cargo metadata,
an unchanged bindgen 0.72.1 geometry registry regeneration and Git
whitespace checks passed. Build, lint, tests, fixtures and device or
moving-game acceptance remain user-owned.

## Remaining geometry source and fixtures

The N03 follow-up at base `76720485` moves palette address admission,
registry lookups and retirement delivery through Rust's typed geometry
owners. The palette reader verifies the start and exclusive end of each
batch using the existing 168-byte instance stride and 64-byte transform
offset. The native leaf still guards each 64-byte read and transfers up
to 64 matrices per call. A partial or non-affine batch still clears the
published palette, while the copied world remains available.

The dispatch path uses typed flush, draw, near and retirement callbacks.
The registry invalidates its whole batch before reporting any retirement.
The locks for the callback slots are released before the callbacks run.
The native flush still captures before the original call, then reports
after the original upload. The native rmNear still runs the original,
enters the near scope and reports it to the existing SR callback. The
motion flush still consumes a borrowed world and palette through the
same call record. The inline adapter now lives with that record, so the
separate geometry feed translation unit is gone.

Both profile banks keep their original RVAs, byte strings, table order
and layouts. The real MinHook fixture lends the selected bank to Rust to
seed the synthetic PE. Rust writes detour bytes, layout bytes and then
extended prefixes in the original order. The C++ fixture still owns the
executable allocation and calls the native install, restore and re-enable
paths for Standard and AVX. It still mutates every extended prefix,
layout site and entry site to verify guarded refusal, and checks 65-bone
capture and rejection when the last matrix is invalid. The pure registry
cases now exercise the Rust owner directly. The mixed fixture still
checks TLS, guarded world and palette reads, capture into temporal
history and retirement across threads. Its exception guard stays with
the native history callback.

Source comparison matched the two guarded profile banks and 5,120
modeled palette span cases, including addresses at and beyond the old
exclusive-end limit. N01's observer sources and N04's motion stage
sources match the base, as do the SR and FG stage sources. Pinned Rust
1.94.1 formatting, locked offline Windows Cargo metadata, targeted
bindgen 0.72.1 regeneration of the unchanged registry binding and Git
whitespace checks passed. Builds, Clippy, native fixtures and game or
device execution were outside this pass.

## Rust fixture orchestration and hook plans

The follow-up at base `b5a7434c` moves geometry hook-record assembly into
the Rust geometry owner. It pairs each exact-profile site with the typed
replacement and the address of its native original slot, using the shared
hook record. Both C++ profile banks remain the immutable source for the
guard spans and the compile-time profile fixture. Staged vertex constant
reads and writes now resolve and copy in Rust through the same guarded
native leaf. Slot admission, the reflected-size check and read ordering
are unchanged. The public C++ interfaces still serve N01 observation,
N04 motion and N06 SR.

Rust drives eleven native hook scenarios and five mixed geometry scenarios
in their previous order. This includes disabling the fixture detours,
retiring geometry, and clearing the retirement sink before the synthetic
production-install scenario. The Rust guard driver seeds each selected
bank, corrupts every extended prefix, layout site and detour entry in
order, checks no early writes, and checks install, restore, both wrong-bank
refusals and re-enable. It holds no allocation slice across installation.
C++ still supplies the executable PE allocation, the exact Microsoft x64
trampolines and their RAII scopes, real MinHook and guarded copy leaves,
and the native history and worker-thread callbacks. Executable fixture
memory and trampolines retain their process lifetime.

Source comparison matched the profile banks, the N01, N04, SR and FG
callers, both hook fixture registrations, 139 native hook assertions
outside the guard scenario and 71 mixed-contract assertions. The native
syntax pass caught an existing missing include for the geometry key
adapter. Restoring the include allowed MSVC syntax-only checks to pass
with warnings as errors for the geometry source and both fixtures.
Pinned Rust 1.94.1 formatting, targeted bindgen 0.72.1 regeneration,
locked offline Windows metadata and whitespace checks passed. Build,
link, lint, fixtures and device or game execution remain pending.
