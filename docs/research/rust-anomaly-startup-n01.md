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
