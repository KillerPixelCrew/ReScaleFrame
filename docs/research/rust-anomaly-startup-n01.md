# Anomaly startup and observation source port

Baseline `14394e47`. [Cold-entry port](rust-anomaly-bootstrap.md) and
[native observer research](rust-anomaly-native-observer.md) describe the
earlier ownership transfers. This change adds source coverage for N01.

## Rust review findings

| Finding | Source evidence |
| --- | --- |
| Use the same FG capture settings decoder for the Anomaly panel and scheduler. | `CaptureSettings::decode` supplies both. Start spans 0 through `i32::MAX`, count 0 through 16, and interval 1 through `i32::MAX`. The INI's unsigned interpretation is retained before clamping. |
| Use the generated plugin-session options type in both Rust callers. | Anomaly bootstrap and the Runtime callback host use `rsf_plugin_session_options` and its generated version. Bootstrap also uses the SDK's synchronous options and probe helper. |
| Keep input-hook installation separate from the stages-configured flag. | The cold entry records DirectInput receipt and CInput slot progress independently of stage setup. The Rust retry fixture refuses the CInput slot after successful DirectInput installation and verifies that an overlay retry calls only the unfinished input step. |
| Share partial-pivot matrix inversion while preserving adapter tolerances. | GameSDK owns elimination. Its default retains the 1e-12 floor and division used by AC7 and Skyrim. Anomaly selects the original 1e-300 floor, finite-input check, reciprocal normalization, and zero-factor skip. Reference vectors cover row swapping, distinct near-singular admission, and nonfinite input. |
| Reuse the verified executable identity across cold startup and session preparation. | Runtime prepare synchronously calls the plugin on the same thread. The adapter lends the selected index for its exact static profile table during that call. Standalone prepare still resolves and hashes the process executable. Observer PE and instruction checks run on either path. |
| Read each variable-length instruction guard as one range. | `Image::matches_hex` bounds the range, decodes all bytes and calls the guarded copy once. A Rust recording leaf checks the call count and length and refusal before reads for malformed or out-of-range guards. |
| Give Windows module-path lookups one explicit terminator contract. | GameSDK supplies bounded owned lookup and explicit unit accessors. Anomaly and RDR2 lend the terminated view to Win32 and use the other view for paths. Wingman keeps its stack-backed loader-lock check through the same bounded lookup. |
| Consolidate the raw UTF-16 scans at Anomaly configuration boundaries. | Cold start, overlay, FG, motion and SR use the same borrowed UTF-16 scan. Cold start and overlay still reject empty paths, motion copies the terminator, and FG and SR use the units before it. |

## Native boundaries and fixtures

- `build_profile.h` forwards both name formats to Rust. The native tables
  still supply the exact executable identities and per-build layouts to
  the existing geometry, SR, input and motion callers.
- `engine_guards.h` retains the fixed-site ABI used by the SR position slot.
  Rust owns fixed and variable guard policy. The unused C++ hex helpers
  and symbol were removed.
- `native_hooks.cpp` invokes the Rust mapped-region check before the
  fault-contained copy. Windows PE layout extraction, MinHook calls and
  C++ trampoline invocation remain native. The observer's frame sample
  uses bounded Rust reads for the three profile RVAs and publishes only a
  complete sample. Native camera reads still use the guarded copy leaf.
- `runtime_module.h` retains the SR driver's carrier-owned Runtime borrow
  and colour-transport callbacks. The unused raw symbol export was removed.
  Rust uses GameSDK typed lookup.
- The startup and plugin ABI thunks in `bootstrap.cpp` and `plugin.cpp`
  still publish the native DLL entry points. `engine_slots.h` and
  `native_memory.h` keep the platform fault boundaries.
  `native_hook_ops.h`, `native_hooks.h`, `native_observer.h`,
  `native_observer_platform.h` and `native_profile.h` provide native
  linkage, ABI declarations or checked build data.
- `tests/anomaly_bootstrap.cpp` moved to `tests/anomaly_bootstrap.rs`.
  CTest still calls the exported entry with invalid paths, a foreign
  executable, and renamed foreign executables under both profiled names.
  The lifecycle, plugin and native-hook fixtures still exercise the DLL
  ABI and MinHook with their native test hosts. The build selector still
  serves the native SR and geometry fixtures.

The temporal frame planning, resolution planning, matrix helper and
history headers still serve native temporal, motion, SR jitter and
geometry fixtures or callers. They are outside this startup and observer
lifecycle change.

## Checks and limits

Reviewed callback types, native callers, CMake fixture registration and
the generated SDK option layout. The public headers used by bindgen and
the observer platform record are unchanged. Pinned Rust 1.94.1 formatting
and `git diff --check` passed. Builds, fixture execution, lint and
device or game verification remain open.

## Native entry and fixture follow-up from `82b39a63`

Rust now publishes the GameSDK, cold-start and observation exports.
The adapter's module definition declares the existing names and ordinals
1 through 3. The DLL links the feature-enabled Rust archive and the
native fixture targets link archives without the DLL-only entry points.
The generated bootstrap platform record supplies the existing exact
profile table and downstream stage interfaces.

Cold startup installs the DirectInput observation hook, publishes its
receipt callback, and only then attempts the guarded CInput slot. Retry
state covers all three operations. The Rust fixture observes the order
and verifies one receipt publication when the engine slot needs a
second attempt. The overlay and the SR and motion stage owners still
provide their existing interfaces.

Rust reads and extracts the DOS and NT header facts. The Win32 header
types preserve the full read extent. Hook installation uses the
mapped-region check and the exception-contained copy; observer
preparation uses the exception-contained copy and verifies the exact
profile and mapped instruction guards separately. The native observer
detours keep their C++ invocation lifetime and original-call order so
a foreign exception cannot cross into Rust. The native host-log leaf
still catches foreign exceptions.

| PORT path | Disposition |
| --- | --- |
| `games/anomaly/src/bootstrap.cpp` | Removed. Rust owns the export and input observation order. `bootstrap_platform.cpp` binds the native profile and stage calls. |
| `games/anomaly/src/plugin.cpp` | Removed. Rust publishes the entry point and lifecycle callbacks using the existing GameSDK ABI. |
| `games/anomaly/src/native_hooks.cpp` | PE projection and mapped-read policy are in Rust. The C++ HookSet facade and typed MinHook and guarded-copy calls remain. |
| `tests/anomaly_plugin.cpp` and `tests/anomaly_lifecycle.cpp` | Removed. The existing GameSDK fixture archive loads the actual DLL and runs the recognition and lifecycle cases. The plugin contract also checks the three export ordinals. |
| `tests/anomaly_native_hooks.cpp` | Removed. Rust owns the guarded-copy, live detour, concurrent retirement, mapping and profile checks. `anomaly_native_hook_leaves.cpp` supplies the compiler-emitted functions and exact profile banks. |
| `tests/anomaly_bootstrap.cpp` | Closed by the earlier Rust subprocess port. |

The following PORT paths remain open:

- `games/anomaly/src/build_profile.h` and
  `games/anomaly/src/native_profile.h` still supply the exact build
  tables to the native input, geometry, motion and SR callers.
- `games/anomaly/src/engine_guards.h`,
  `games/anomaly/src/engine_slots.h`,
  `games/anomaly/src/native_hook_ops.h`,
  `games/anomaly/src/native_hooks.h`,
  `games/anomaly/src/native_memory.h`,
  `games/anomaly/src/native_observer.h`,
  `games/anomaly/src/native_observer_platform.h` and
  `games/anomaly/src/runtime_module.h` retain the native declarations,
  typed MinHook calls and fault-contained copy and slot exchange.
- `games/anomaly/src/native_observer_platform.cpp` still assembles
  native detour sites, reads camera samples through the SEH leaf,
  invokes engine originals and reports native stage diagnostics.
- `tests/anomaly_build_select.h` still serves the native SR and
  geometry fixture hosts.
- `games/anomaly/temporal/frame_plan.h`,
  `games/anomaly/temporal/history.h`, `games/anomaly/temporal/math.h`
  and `games/anomaly/temporal/planning.h` still serve the native
  temporal, motion, SR jitter and geometry callers and fixtures.

Targeted bootstrap bindings were regenerated with bindgen 0.72.1.
The full generator stops on the existing D3D11 SR admission invocation,
which lacks the SDK include path for `rescaleframe/game_frame.h`.
Checked source declarations, preserved CTest targets, scoped the two
feature archives, ran Rust 1.94.1 formatting, locked offline Cargo
metadata and `git diff --check`. Build, test execution, lint and
device or game verification remain user-owned.

## Verified profile and native actor follow-up from `8df04a40`

The generated private observer profile now carries the exact image
size, frame and dimension RVAs, and the guarded site banks borrowed
from the two native build records. Prepare checks the PE and
instruction guards and retains that immutable profile in the observer
engine. First hook installation and frame reads now take their inputs
from that same prepared profile. HookSet still verifies mapped
ownership, protection and replayed guard bytes before registration.

Rust assembles the hook sites with the selected bytes and RVAs.
The C++ actor defines a typed trampoline slot for each replacement
and keeps the actual calls, invocation retirement and geometry scope
on the native side. Camera samples use the native fault-contained
copy for each matrix, and Rust publishes the pair only after both
copies succeed. The public observation ABI and its version stay the same.

The profile-taking SR and geometry test hosts now delegate argument
selection to Rust. The first matching argument and the standard
default are unchanged. The native hook fixture now lends the native,
motion and selected SR site banks separately, and Rust seeds the
fixture image in the original order.

| Path | Source disposition |
| --- | --- |
| `games/anomaly/src/native_observer_platform.cpp` | Hook site assembly, frame field selection and camera sampling moved to Rust. Keeps typed detour actors, native stage bridges and exception or SEH leaves. |
| `games/anomaly/src/native_observer_platform.h` | The private profile and trampoline bindings now have generated layouts. The public observation record remains unchanged. |
| `tests/anomaly_build_select.h` | Argument selection moved to Rust. The facade still projects a native `BuildProfile` reference, supplies the opposite profile by reference identity and prints the fixture banner. |
| `tests/anomaly_native_hook_leaves.cpp` | Rust owns bank traversal and image seeding. C++ supplies the compiler-emitted target functions, guarded copy and exact profile bank pointers. |

`games/anomaly/src/build_profile.h` and
`games/anomaly/src/native_profile.h` remain the single compile-time
source for the exact Standard and AVX identities and records. The
native input, geometry, motion and SR owners still consume them, as do
the profile evidence tools and native compile-time assertions. The
observer platform, slot exchange, PE and memory fault boundaries,
MinHook and stage wrappers remain native. The residual temporal headers
remain with their temporal and SR consumers.

Source checks compared all eight native actor bodies and the binding
order against `8df04a40`, preserving callback order and original-call
arguments. The exact profile files and public observation declaration
matched the base. Targeted bindgen 0.72.1 regeneration, pinned Rust
1.94.1 formatting, locked offline Cargo metadata and `git diff --check`
passed. Added guarded-camera and selected-profile batch fixtures remain
unrun. Builds, test execution, lint and game or device checks remain open.

## Canonical profile and native events from `0251a81a`

The immutable Standard and AVX records now live in
`games/anomaly/src/native_profile.rs`. Cold startup and GameSDK
recognition read the same name and digest records. The observer keeps
the admitted index and uses it for PE and guard checks, hook assembly,
and frame reads. Synchronous Runtime prepare can borrow the verified
index on the calling thread. Standalone prepare still resolves the
process executable and verifies its hash.

`tools/generate-anomaly-native-profile.py` projects those records into
`games/anomaly/src/native_profile.h`. The generated block retains the
field names, section markers, and guard syntax consumed by the
existing profile evidence readers. Run the generator after editing
the Rust records. The source-only `--check` mode verifies that the
projection matches, checks Standard and AVX index order, and checks
both builds' extended guard prefixes. `build_profile.h` uses the
generated names and keeps the native geometry, SR and motion links and
the compile-time fixture interfaces.

| Paths | Source disposition |
| --- | --- |
| `bootstrap.rs`, `bootstrap_platform.cpp`, `bootstrap_platform.h`, `bootstrap_abi.rs`, `executable.rs`, `lib.rs`, `plugin_entry.rs` | Rust owns runtime identity and labels. The native bootstrap record now contains stage calls and native input installation leaves. |
| `native_profile.rs`, `native_profile.h`, `build_profile.h`, `tools/generate-anomaly-native-profile.py` | Rust owns the identity, observer sites, extended guards and frame facts. The generated C++ block projects those facts for native callers and source evidence. |
| `engine_guards.rs`, `input/profile.rs` | Both profile owners use the same compile-time 16-byte guard decoder. The CInput evidence records retain their original bytes. |
| `native_observer.rs`, `native_observer_platform.cpp`, `native_observer_platform.h`, `native_observer_abi.rs` | Rust owns before and after event dispatch. Native actors keep the original calls, the invocation destructor and the scene geometry scope. The existing Rust FG C entries serve the observer table directly. |
| `native_hooks/fixture.rs`, `tests/anomaly_native_hook_leaves.cpp` | The fixture uses the same canonical native sites as production. Native leaves supply the motion and selected SR banks, guarded copying and compiler-emitted functions. |
| `bootstrap/tests.rs`, `native_observer/tests.rs`, `tools/generate-rust-bindings.py` | Updated fixture inputs and generated private bindings. The selected-hook fixture exercises the canonical AVX bank and an invalid index. |

The End actor keeps its late completion path for a frame opened
before quiesce. Scene, UI and reset-begin facts precede their
original calls. Begin, camera and copy facts follow their original
calls. Reset end retains the invocation lease without emitting a
fact. The native RAII destructor still retires every detour during
C++ unwinding. The public observation payload stays at ABI 1.

The remaining native edges are the generated compile-time profile
adapter, typed engine trampoline calls, SEH copy and slot exchange,
MinHook operations, geometry scope and native stage/report leaves.
The fixture still borrows stage-owned site banks and native functions.
No N04 or N06 owner or caller interface needed an edit.

Source comparison against `0251a81a` matched two hashes, eight
layout values, sixteen fixed guards and four extended guards.
It also matched all eight original-call argument lists, trampoline
bindings and the public observation declaration. Targeted bindgen
0.72.1 regeneration, the native profile projection check, pinned
Rust 1.94.1 formatting, locked offline Cargo metadata and
`git diff --check` passed. Build, fixture execution, lint and game
or device checks remain open.
