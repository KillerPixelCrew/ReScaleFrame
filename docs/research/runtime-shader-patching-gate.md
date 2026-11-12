# Runtime shader patching: Anomaly milestone gate

12 November 2026. Uncommitted development work against published main ff5bff4.
No game was launched, no game installation was changed, and nothing was deployed
or committed. This report covers the simplified Anomaly-only milestone.

## Result and boundary

The shared Runtime owns a versioned C service for raw `.rsfpatch` loading,
source transforms, exact original-bytecode proof, compilation, reflection,
local cache, D3D11 shader inventory/creation, and constant-buffer allocation,
upload and binding. Anomaly supplies source acquisition, rules, eligibility,
previous-frame data, stage linkage and draw reinsertion. The JSON rule pack is
the sole rule representation. No game shader bodies, generated clones or caches
are packaged. Offline transforms remain a development oracle.

The adapter queries `rsf_get_shader_patch_api` from the already loaded Runtime.
Shader patch ABI is 1. Native overlay service ABI is 4, appending a live label
callback so current pack status reaches the shared panel. Rust panel ABI stays 17.
All DLL boundaries use C structures and explicit borrowed/owned lifetimes.

The first client registers one named `patches/motion.rsfpatch`; there is no folder
scan or dependency dispatcher. Different packs claiming the same original root
hash are refused. Same-id generations can coexist during transactional reload.
Anomaly's overlay exposes saved pack enablement, live status, Reload and Dump
intercepted shaders. Reload validates all previously known eligible variants
before committing and resets motion history. Failure retains the old generation.
Disabling selects stock draws without requiring another game session.

The local cache includes the pack, compiler binary, stock DXBC, root name/source,
entry, target, flags and ordered definitions, plus Runtime version and cache
revision. Original-bytecode proof runs before accepting a cache hit. Native
reflection and Anomaly's stricter linkage checks remain in place.

## Evidence and checks

| Check | Result |
| --- | --- |
| Release native gate, VS2026 | 213 discovered: 201 passed, 12 skipped; fmt and Clippy passed |
| Cargo workspace tests, locked | 73 unit tests passed, plus doc tests |
| Updated Anomaly loader contract | Focused rebuild/test passed after the full gate; exact input delegation, eligibility and stock refusal propagation checked |
| Anomaly packaging tests | 10 passed; includes no-corpus pack build and 48-byte maximum matching patterns |
| Native corpus parity | All 454 output files match the Python merged output byte for byte |
| Native source service over corpus | All 73 roots original-proven, patched-compiled and reflection-checked with System32 d3dcompiler_43 |
| Engine provider PE evidence | Both pinned standard/AVX executable hashes, includer table and Open/Close prologues verified read-only |
| Exported DLL | Runtime loaded and all 13 shader-patch API entries resolved |
| Full packager dry run | Refused existing local FidelityFX pin mismatch, before emitting archives |
| Diff whitespace | `git diff --check` passed |

The synthetic/WARP fixture covers all four source operations, deterministic output,
missing/ambiguous anchors, incorrect source hash, original-bytecode mismatch,
reflection, cache reuse, root-specific cache identity, conflicting packs, truncated
pack inputs, C ABI validation, D3D11 interception and generic constant-buffer
binding readback. It renders an original DXBC pixel shader returning red 0.25 and
an instruction-patched shader returning 0.5, then reads both results back. This
same-length instruction rewrite is synthetic-only. There is no bytecode game
client, general DXBC assembly, new DXBC interface/resource rewriting, D3D12 or
other backend implementation in this milestone.

The full gate ran `./eng/verify.ps1 -Configuration Release -VS2026`; the separate
Rust check ran `cargo test --workspace --locked`. The new shader-patch fixture
ran and passed, rather than skipping. The final native suite took 175.58 seconds.
Only the loader test source was extended afterward and its focused rebuild/test
passed; production code did not change after the full gate.

The overlay device fixture checks that live labels are fetched after current
option values on each Present. An existing overlay screenshot fixture was also
stabilized by putting its physical cursor at the simulated inside-click position
and restoring it afterward; previously the real cursor could close the panel
before its screenshots.

The 95,058-byte raw pack has SHA-256
`6d16974bbc2f34576e6f84978ba3ee802529a7764b7fad5c4b5e5119af673f70`.
It contains 449 original source hashes, 73 root records, 20 transformed-file
rules and four shared RSF-authored header assets. Byte offsets paired with short
anchors are intentional for repeated IO member blocks: whole-file hash validation
fixes their position. The result is exactly the Python output, with no shader
semantic differences claimed or required.

The engine filesystem acquisition itself is source-reviewed and exact-build
PE-checked, not game-tested. See [the design/evidence note](runtime-shader-patching.md)
for source revision, table RVAs, expected bytes, refusal rules and the earlier
tracked-file audit accepted by the coordinator. No ownership scanner, signatures,
trust labels, binary receipts or compression subsystem were added.

Ignored local logs:

- `.local/anomaly/shader-patch-final-verify.log`
- `.local/anomaly/shader-patch-cargo-test.log`
- `.local/anomaly/shader-patch-package-test.log`
- `.local/anomaly/shader-patch-parity.log`
- `.local/anomaly/shader-patch-package-dry-run.log`
- `.local/anomaly/shader-patch-loader-build.log` and `shader-patch-loader-test.log`

## Files

- `runtime/shader_patch/`: public API, pack/source engine, compiler/cache/reflection,
  D3D11 service, synthetic DXBC rewrite, pinned yyjson setup, notices and usage.
- `runtime/orchestrator/CMakeLists.txt`, `include/rescaleframe/overlay_api.h` and
  `src/overlay_api.cpp`: core service build/export and generic live labels.
- `games/anomaly/patches/motion/`, `src/runtime_shaders.*`, `src/shader_loader.*`,
  `src/motion_stage.*`, `src/motion_d3d11.*`, `src/bootstrap.cpp`,
  `src/native_observer.cpp`, `src/overlay_service.cpp`, `src/adapter_options.h`,
  `CMakeLists.txt`, `engine.json` and `PROGRESS.md`: rules/source provider and
  production adapter migration, live controls and evidence/readiness scope.
- `tools/rsf_shader_pack.py`, `tools/package-anomaly.py`,
  `tools/anomaly_motion_deploy.py`: build/inspect CLI, explicit rules-only
  release membership and development-only offline tool description.
- `tests/shader_patch*.cpp`, `tests/anomaly_shader_provider_evidence.py`,
  `tests/package_anomaly_test.py`, `tests/anomaly_motion_contract.cpp`,
  `tests/overlay_dll_api_device.cpp`,
  `tests/anomaly_overlay_visibility.cpp`, `tests/CMakeLists.txt` and
  `tests/anomaly_support.cmake`: synthetic/device, corpus, provider and package checks.
- `docs/dependencies.md`, `docs/implementation.md`,
  `docs/releases/anomaly-install.md`, `docs/releases/v0.1.2-anomaly.md` and
  `docs/research/runtime-shader-patching*.md`: dependency, architecture,
  current-source packaging correction and gate records.

Only yyjson (MIT) is incorporated, pinned at
`ebb8d7e5aaa51bea4edefb39ee73a235066b0023`, under ignored
`vendor/shader-patch/yyjson` with its original licence. CMake obtains only its
three pinned files when absent, and source archives include those files.
3Dmigoto and zstd evaluation downloads remain unincorporated. Original dependency
notices accompany the binary package. No publishable release ZIP was produced
from this uncommitted tree; the existing clean-source release gate remains.
A full packager dry run with `.local/streamline/sdk` refused the existing local
`vendor/fidelityfx/Kits/FidelityFX/signedbin/amd_fidelityfx_upscaler_dx12.dll`:
expected `d0dcccc74a43c44ba435b7a369b456e0970d8a4464e4bd683119b374f2c9fb46`,
found `aad4fccccb29084a655f9a4f9f613bd22c8c03fb4a0f8840148cc6543b4ccace`.
The existing release pin was left unchanged. Release packaging needs the pinned
SDK input; this does not affect the independently tested corpus-free pack build.

## Authorized game acceptance still required

1. Start each pinned standard/AVX executable with only the rules pack installed
   and the game's own source/archive inputs. Expect 73 allowlisted roots in the
   startup log and nonzero `intercepted` and `runtime_built` on first use.
2. Confirm family/stage variant counters include the expected core, foliage and
   hull/domain permutations. Exact original DXBC proof must succeed before any
   replacement is accepted. Count actual `motion`, `valid`, replay and target
   usage; builds alone do not establish that draws use the variants.
3. Repeat the scene/session with the same pack. `cache_hits` must increase while
   original proof still passes. Record first-use compile stalls before proposing
   background compilation. No local generated cache may enter a package.
4. Inspect `refused` and its named counters: `source`, `hash`, `anchor`, `compile`,
   `original`, `reflection`, `format`, `internal`, plus adapter frame/create/
   linkage refusals. Unrecognized shaders are expected to remain stock; eligible
   failures need the logged reason. Never infer success from a generic total.
5. Toggle the pack off/on live and restart to verify persistence. Stock draws
   while disabled must not expose stale motion. Test Reload with valid updated
   rules and with a truncated/bad pack; the latter must retain working variants
   and show refusal. Successful reload must reset history without stale objects.
6. Exercise tree/grass, rigid/skinned models, tessellation, scene changes and
   resize/device reset. Inspect motion and temporal SR for the original acceptance
   criteria; verify constant-buffer state restoration and stock fallback.
   Dump is user-triggered and writes local DXBC/disassembly only.

Existing Anomaly game acceptance belongs to the earlier deployed paths. No new
provider/reload acceptance or general adapter readiness is claimed here.
