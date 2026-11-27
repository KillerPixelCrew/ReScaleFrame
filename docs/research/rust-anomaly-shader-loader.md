# Anomaly shader preparation in Rust

20 November 2026. Adapter cutover from `6b73308` on `NightHammer1000/rust-anomaly-shader-loader`.

## Evidence and ownership

The migration follows the existing [exact-build caller-frame evidence](anomaly-avx-profile.md),
[Runtime source-provider evidence](runtime-shader-patching.md) and
[tessellation interface tests](anomaly-tess-motion.md). The engine fingerprints in
[engine.json](../../games/anomaly/engine.json) still select
[motion_profile.h](../../games/anomaly/src/motion_profile.h). The original disassembly
evidence uses xray-monolith revision `028ccf5` as the source reference. This change
reuses that evidence and adds no executable sites or third-party shader code.

| Compile boundary | Standard 1.5.3 | AVX 1.5.3 |
| --- | --- | --- |
| `create_shader` call for an engine cache hit | `0x83b9b1` | `0x832cb1` |
| `create_shader` call for new engine bytecode | `0x83bb93` | `0x832e93` |
| Guarded D3DCompile setup | `0x83b9ce` | `0x832cce` |

Rust accepts those call RVAs plus five bytes relative to the selected image. The
native guarded reader fetches the original root pointer and byte length, entry
point and flags at caller RSP offsets `0x14f0`, `0x14f8`, `0x1500` and
`0x1510`. Ordered macro pairs start at `0x250`; a null name must occur within
128 entries. The root stays bounded at 16 MiB and strings at 512 bytes including
the terminator. Rust checks address addition before each read and supplies the
decoded data to the existing native request owner. The detour still creates the
stock shader first, with the original DXBC and VS input signature.

- Rust admits manifest metadata, hashes the original root's exact bytes, checks
  the target stage and derives the skin mode from the ordered defines. The
  native map, request and reflected data retain their existing owners. The private
  C calls borrow byte spans and reflected descriptors synchronously. (Superseded for
  the map: Rust now owns the admitted rows and their index, see
  [the manifest owner](rust-anomaly-shader-manifest.md).)
- The production path still borrows the loaded Runtime shader-patch service.
  The guarded engine includer and `d3dcompiler_43` entry remain native. Runtime
  transforms the same pack, recompiles the original with the supplied source,
  entry, target, flags and defines with the same empty compiler source name,
  compares it to the engine's DXBC byte for byte
  before cache lookup, and reflection-checks the cached or compiled output.
  Rust applies the adapter's compatibility checks to both kinds of result before
  the motion stage can use them. Stock reflection preserves the engine's buffer
  order and offsets. D3D reflection remains in the native `d3dcompiler_47` leaf.
- The offline fixture reaches the Rust tree/cache flow through the existing
  `ShaderLoader` constructor. The existing key includes namespace, both source
  hashes, target, entry, decimal flags, stock DXBC hash and ordered defines.
  A reflectable compatible cache hit skips both compiles as before. A rejected
  cache entry falls through to the exact original DXBC proof and verified motion
  source. Cache publication uses a private temporary file and rename. Production
  cache identity and its pre-cache proof stay with the unchanged Runtime service.
- Rust checks VS and PS compatibility, exact hull/domain patch constants, all
  required tessellation interpolants, and linkage across either the VS/PS pair
  or the VS/HS/DS/PS chain. The native motion stage still owns replacement object
  creation and draw selection. Refusals leave the stock path.

The old tessellation check verified membership and the total number of added
fields. Replacing TEXCOORD15 by another TEXCOORD12 at a different register
preserved that count and passed. The added test failed against the original C++
implementation in MSVC Debug. The Rust check requires each distinct field once.
The regression covers both stages' inputs and hull/domain outputs, with the
ordinary variants and stock-linkage refusal cases retained.

## Verification

Configured with CMake 4.4.4, Visual Studio 18 2026, MSVC 19.52.37029 and the
Windows SDK 10.0.26100.0. `build/anomaly-shader-loader` and its `_deps` are in
this worktree. The MinHook source and optional vendor fixture root were the
coordinator's read-only copies. The three yyjson files were copied to this
worktree after each SHA-256 matched `runtime/shader_patch/yyjson.lock.json`.

```powershell
cmake --preset windows-x64-vs18 -B build/anomaly-shader-loader `
  "-DFETCHCONTENT_BASE_DIR=$PWD/build/anomaly-shader-loader/_deps" `
  "-DFETCHCONTENT_SOURCE_DIR_MINHOOK=D:/Coding/ReScaleFrame/build/windows-x64/_deps/minhook-src" `
  "-DRSF_ANOMALY_SR_VENDOR_ROOT=D:/Coding/ReScaleFrame/build/releases/rdr2/stage-rdr2-a2bcc0c3/ReScaleFrame"
$targets = @(
  'rsf_game_anomaly', 'rsf_anomaly_patch_pack', 'rsf_anomaly_plugin_test',
  'rsf_anomaly_lifecycle_test', 'rsf_anomaly_bootstrap_test', 'rsf_anomaly_motion_contract_test',
  'rsf_anomaly_motion_hooks_test', 'rsf_anomaly_sr_hooks_test', 'rsf_anomaly_motion_device_test',
  'rsf_anomaly_motion_tess_device_test', 'rsf_anomaly_motion_tess_chain_device_test',
  'rsf_anomaly_motion_far_device_test')
$cpu = '^anomaly_(motion_contract|motion_unknown_selection|motion_hooks_contract|plugin_contract|lifecycle_contract|bootstrap_contract|shader_tool_contract|sr_hooks(_avx)?_contract)$'
foreach ($config in 'Debug', 'Release') {
  cmake --build build/anomaly-shader-loader --config $config --target $targets --parallel 2
  ctest --test-dir build/anomaly-shader-loader -C $config -R $cpu --output-on-failure
}
cargo test --workspace --locked
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo fmt --all --check
git diff --check
```

Both builds compiled the adapter, focused fixtures and four motion device
executables. A Debug rebuild returned `CL.exe` exit `-1` with no compiler
diagnostic at parallelism four. The final two-job rebuild passed. All nine CPU
contracts passed in each configuration, covering both profiled caller
frames, bounds, manifest refusals, fixture original-bytecode reproduction and
cache recovery, Runtime result handling including verification of a reported
cache hit, and four-stage compatibility and linkage. The cache fixture asserts
the exact key `9d93a1707dc4402f46d0e05821221752f0ed091580cbafe9d0945137ba6519e7`.
Locked Cargo tests passed 222 unit tests plus doc tests with Rust 1.100.0.
Clippy and formatting passed.

Compared Debug and Release DLLs to the staged
`D:/Coding/ReScaleFrame/.local/anomaly/staging/8322179a36c7/ReScaleFrame/ReScaleFrame.Game.Anomaly.dll`
using `dumpbin /exports` and an ignored worktree script
`build/anomaly-shader-loader/compare-pe.py` (pefile and Capstone). The PE export
names and ordinals match exactly.

| Ordinal | Name |
| --- | --- |
| 1 | `rsf_anomaly_observation` |
| 2 | `rsf_anomaly_start` |
| 3 | `rsf_get_game_plugin_api` |

The lookup cross-references retain `GetModuleHandleW(L"ReScaleFrame.Runtime.dll")`
and `GetProcAddress` for `rsf_plugin_session_prepare`,
`rsf_plugin_session_start` and `rsf_get_shader_patch_api` with no static
Runtime DLL import. The staged adapter's startup module lookup is at RVA
`0x4a3a` with session symbol lookups at `0x3e15` and `0x3e28`. Debug calls the
module lookup at `0x16190` and the symbol lookup through the helper at
`0x6da67`. Release calls the module lookup at `0x2b33` and calls
`GetProcAddress` through `r14` at `0x2b52` and `0x2b62`. The staged
Runtime exports all three requested names.
Both cold-start fixtures independently refuse a foreign executable hash before
using the Runtime. The two rebuilt 95,049-byte packs have SHA-256
`d2aa077a561ee71c084f806edb2f7cdcb0bf4ccaa17907a788324043f8622bb2`,
matching the staged pack under `patches/motion.rsfpatch` (`Get-FileHash`).

This cutover has CPU and PE evidence. Device executables were built only. Live
engine include acquisition, real permutation compilation and cache reuse, reload,
and rendered motion on Standard and AVX remain open on this build.
