# Anomaly motion manifest owner in Rust

21 November 2026. Port from `68e286e` on `NightHammer1000/rust-anomaly-shader-manifest`.

## Question and path before the change

Which side owns the admitted motion manifest, and can one owner serve both
manifest forms? Rust already validated both forms (`shader_loader/manifest.rs`):
the fixture text manifest and the Runtime pack metadata read through
`rsf_patch_api.root`. Each validated row was then copied into a C++
`ManifestBuilder`, which kept `std::map<std::string, ManifestEntry>
by_original`. Two lookups used that map: `ShaderLoader::lookup` for the fixture
and the Runtime-delegate test path, and the motion platform leaf `find_entry`,
which the Rust motion `Pack` called on every `create_shader` and for every
registered variant on reload. The digest index therefore lived in C++ while
the validation and the hash it was keyed by lived in Rust.

This is a source-level ownership change. It adds no engine sites, reads no new
engine memory and changes no shader bytes; the exact-build evidence remains in
[the shader loader audit](rust-anomaly-shader-loader.md) and
[Runtime shader patching](runtime-shader-patching.md).

## Ownership after the change

| Concern | Owner |
| --- | --- |
| Header, namespace, record, family/stage, name and hash validation | Rust, unchanged rules and messages |
| Duplicate original rejection | Rust, per candidate (`HashSet` of borrowed originals) |
| Row storage and namespace | Rust `Manifest`, boxed copies made only after the whole candidate validates |
| Digest index and iteration order | Rust, rows sorted by original SHA-256; exact binary search |
| Fixture/test handle | C++ `Manifest`, a `shared_ptr` to the opaque Rust object (copies share it) |
| Production handle | `RuntimeShaders::manifest`; the Rust `Pack` borrows it via `PackInfo` |
| Engine memory, includer, D3D compile/reflect, ABI marshalling | C++, unchanged |

Private ABI (`shader_loader_platform.h`): `rsf_anomaly_shader_manifest` and
`rsf_anomaly_shader_pack_manifest` now return an owned
`rsf_anomaly_shader_admitted*`, or null after reporting one refusal reason.
`_admitted_view` lends the namespace and the ordered `rsf_anomaly_shader_entry`
rows, `_admitted_find` the exact-digest lookup, `_admitted_free` releases it.
The row layout (three byte spans, stage, family; 56 bytes) is asserted in Rust,
in C, and against the C++ `ManifestEntry` spelling. The sink callback per row,
`ManifestBuilder` and `by_original` are removed.

Motion platform ABI 3 (`motion_stage_platform.h`, `motion/stage/abi.rs`):
`find_entry` and the C++ `ShaderEntry` copy are removed (344-byte table).
`PackInfo` carries the admitted manifest pointer instead of a namespace copy and
a count, and `prepare_shader` takes the Rust row itself. The Rust `Pack` looks
rows up in its own manifest and passes the borrowed `&Entry` through prepare.

Preserved behaviour:

- Membership is exact: a 63-byte, upper-case or unrelated digest misses.
- A refused candidate never replaces an admitted manifest. `admit_runtime_manifest`
  assigns only after Rust admits and the handle is built; a control-block
  allocation failure also leaves the old manifest and reports `manifest storage
  failed`.
- Borrowed lifetimes: a fixture `Prepared::entry` points into the loader's shared
  manifest. In production, rows are borrowed from the pack while the stage's
  loader lock is held. `create_shader` still clears `Prepared::entry` before a
  variant outlives the pack, as before.
- Teardown order: the Rust `Pack` drops its lease, `close_pack` deletes
  `RuntimeShaders`, its destructor closes the Runtime session, and the manifest
  member is released last. The Rust pointer is never used after the lease.
- Reload/swap: reload still opens a complete new pack (own session and manifest)
  under the loader lock, maps the old variants through the new manifest, and swaps
  only on success. The old pack and its manifest stay until the swap. The manifest
  is immutable, so concurrent readers need no lock of their own.
- `shader_created` now splits the loader guard so the borrowed row can stay alive
  across counter updates; the lock scope is unchanged.

No defect in the previous implementation was reproduced, so there is no
separate fix. License and boundary: first-party MIT code only, all of it in the
Anomaly adapter; no Runtime, GameSDK or export change.

## Verification

CPU and PE evidence only. No device, game, visible window or `eng/verify.ps1`
ran. Logs are in the worktree under `build/shader-manifest/`.

Rust 1.94.1 from `D:/Coding/ReScaleFrame/.local/rust-1.94.1-verification`
(`RUSTUP_HOME=.../toolchains`, `CARGO_HOME=.../cargo`) with
`CARGO_TARGET_DIR=build/shader-manifest/cargo`:

```powershell
cargo fmt --all -- --check                                              # rust-fmt.log
cargo clippy --workspace --all-targets --locked --offline -- -D warnings # rust-clippy.log
cargo test --workspace --locked --offline                               # rust-test.log
```

All passed: 382 tests (overlay 57, Anomaly 70, orchestrator 236, upscaler 19).
The three new Anomaly tests cover ordered rows that outlive the source text,
exact-digest misses (63 bytes, upper case, a motion hash), text refusals with
their line messages, and pack admission that copies Runtime strings, reports
duplicate, family/stage, read and empty refusals, and looks rows up by pointer
through the private ABI. Mutation: removing the row sort failed two of them
(`rust-mutation-unsorted.log`); restored.

MSVC 19.52 (Visual Studio 18 2026, SDK 10.0.26100.0), tree
`build/shader-manifest/native`, yyjson copied from the coordinator's `vendor/`
after all three SHA-256 values matched `yyjson.lock.json`:

```powershell
cmake --preset windows-x64-vs18 -B build/shader-manifest/native `
  "-DFETCHCONTENT_BASE_DIR=$PWD/build/shader-manifest/native/_deps" `
  "-DFETCHCONTENT_SOURCE_DIR_MINHOOK=D:/Coding/ReScaleFrame/build/windows-x64/_deps/minhook-src" `
  "-DRSF_ANOMALY_SR_VENDOR_ROOT=D:/Coding/ReScaleFrame/build/releases/rdr2/stage-rdr2-a2bcc0c3/ReScaleFrame"
# build/shader-manifest/native-check.ps1, log native-check.log, Debug then Release:
cmake --build build/shader-manifest/native --config $config --target rsf_game_anomaly rsf_anomaly_patch_pack `
  rsf_anomaly_plugin_test rsf_anomaly_lifecycle_test rsf_anomaly_bootstrap_test rsf_anomaly_motion_contract_test `
  rsf_anomaly_motion_hooks_test rsf_anomaly_sr_hooks_test rsf_anomaly_motion_device_test `
  rsf_anomaly_motion_tess_device_test rsf_anomaly_motion_tess_chain_device_test rsf_anomaly_motion_far_device_test --parallel 2
ctest --test-dir build/shader-manifest/native -C $config --output-on-failure `
  -R '^anomaly_(motion_contract|motion_unknown_selection|motion_hooks_contract|plugin_contract|lifecycle_contract|bootstrap_contract|shader_tool_contract|sr_hooks(_avx)?_contract)$'
```

Both configurations built without warnings and passed all nine contracts. The
motion contract now also checks that a copied `Manifest` keeps rows alive after
the parsed owner is gone, ordered pack rows, and that both Runtime refusals keep
the admitted pack manifest. The four motion device executables were built, not
run. `shader_patch_synthetic_device` (the Runtime patch fixture) needs a device
and was skipped; the Runtime was not changed.

PE parity (`dumpbin` lists in `build/shader-manifest/pe/`), against staging
`D:/Coding/ReScaleFrame/.local/anomaly/staging/9367f1306570`:

- Exports of Debug and Release `ReScaleFrame.Game.Anomaly.dll` equal the staged
  ones by name and ordinal: 1 `rsf_anomaly_observation`, 2 `rsf_anomaly_start`,
  3 `rsf_get_game_plugin_api`.
- No static Runtime import. Both DLLs still contain the wide
  `ReScaleFrame.Runtime.dll` name and `rsf_get_shader_patch_api`,
  `rsf_plugin_session_prepare` and `rsf_plugin_session_start`.
- Release imports use the same DLL set as staging, with symbol differences. A
  Release build of the unchanged parent `68e286e` (`git archive` into
  `build/shader-manifest/base-src`, tree `base-native`, `base-check.log`)
  separates them: the staged build's FLS imports were already absent at the
  parent. The only import this change adds is
  `vcruntime140.dll!__std_type_info_compare`, used by the `shared_ptr` deleter
  lookup. Debug imports are not comparable to the Release-only staging.
- `motion.rsfpatch` from Debug, Release and the parent build: 95,049 bytes,
  SHA-256 `d2aa077a561ee71c084f806edb2f7cdcb0bf4ccaa17907a788324043f8622bb2`,
  equal to the staged pack.

Open: live pack open, permutation replacement and reload on Standard and AVX are
unverified on this build.
