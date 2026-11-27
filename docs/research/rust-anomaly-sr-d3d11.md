# Anomaly SR D3D11 resource and pass port

This port starts at `2775855`. It keeps the C++ Host and the Standard and AVX
hook profiles while moving the Anomaly-specific D3D11 ownership and draw
sequencing into Rust. The S-B seam still follows the main-view world window
and precedes the first of sunshafts and blur. The existing Stage and swap
owners supply the frame, device, source and target facts. [SR seam research](anomaly-sr-seam.md),
[AVX profile](anomaly-avx-profile.md), Stage 5 Rust driver

| Path | Resource and shader contract |
| --- | --- |
| Stage 1 round trip | Rust owns the promoted RGBA16F twins for UNORM8 or typeless UNORM8 sources, retains the original source dimensions, and requires FP16 blending and typed UAV support when requested. The PS reads `t0` with output and source extents in `b0` and writes to the engine RTV. |
| Stage 2 world window | Rust creates render-size twins, preserves the non-promoted engine view descriptors, and publishes the actual 44-byte texture description for the engine cache. It owns the `R24G8_TYPELESS` depth texture, its `D24_UNORM_S8_UINT` DSV and `R24_UNORM_X8_TYPELESS` SRV, and the `HW.pBaseRT` scratch texture in the engine RTV's format. |
| Stage 2 spatial seam | The upscale VS and PS read reduced colour, position, distortion and depth in `t0..t3` and write the output-size colour, position, distortion and depth. The existing stencil-only clear precedes the draw. The engine's post chain consumes the output allocations. |
| Stage 4/5 prepare | Rust keys the targets to the colour texture's D3D11 device, both extents and the output format. It owns the render-size RGBA16F colour and motion MRTs, immutable all-invalid motion, output-format texture and 1x1 R32F exposure of 1.0. Prepare binds colour, depth and motion in `t0/t3/t5`, and its extent and first-person constants in `b1`. The current frame's motion still comes from the existing RTV2 producer. |
| Stage 5 reinsertion and fallback | On successful evaluation, reinsertion samples reconstructed output in `t4` and follows the spatial pass's position, distortion and depth writes. An ordinary failure creating the optional shaders or states keeps the spatial pass available. The Stage owner still calls spatial recovery on prepare, evaluation or reinsertion refusal. |

C++ still builds the SDK descriptors, compiles the existing VS/PS entries,
calls COM, and uses the shared immediate-context state save and restore.
The draw leaf suppresses and restores predication around the three-vertex
draw. Rust orders the state scope, constant uploads and bindings. The
facade lends typed views from Rust owners. The Host can still release the
twin RTV/SRV/UAV leases before sampling the texture's diagnostic reference
count. Partial construction drops the leases already acquired. The source
texture and view inputs to each draw remain borrowed for that synchronous
call. The existing stage lock, restore-before-draw order and engine lifetime
rules still govern the world window.

The previous immutable empty-motion upload used a 32-bit `rw * 8` row pitch
and unchecked allocation size arithmetic. The new CPU regression supplies
a width whose old pitch wraps to zero and checks that Rust returns
`TwinError::Create` before allocation. Real D3D11 dimensions ordinarily
reject such a request earlier. Rust now calculates the pitch and word
count with checked arithmetic and handles a failed zero-buffer reservation.
The native state wrapper returns the existing draw refusal if allocating
its snapshot fails. The mock checks that a save refusal leaves bindings
untouched. Additional CPU checks cover the exact binding sequence for all
four draws, optional shader failure, partial twin creation and device/size
retirement.

The swap owner's cut at `d2c1bdb` intersects `sr_d3d11.cpp/.h` at
`TargetFields`. Keep the Rust facade's `std::data(f.desc)` and
`TargetFields fields{}`, which accept both the old `std::array` and the
swap owner's C array. Keep the swap cut's corresponding round-trip fixture
changes. The integrated branch needs its own build and checks.

## Verification

The build directories and FetchContent base were local to the worker
worktree. MinHook came from the coordinator's read-only checkout at
`c3fcafdc10146beb5919319d0683e44e3c30d537`. The initial configure attempted
the repository's yyjson download and rejected the bytes on its locked hash.
The retry used the coordinator's local yyjson files, verified by the existing
lock at `ebb8d7e5aaa51bea4edefb39ee73a235066b0023`. The first MSVC 19.44 Debug
build reached the adapter but failed the existing SR plan fixture's C4127
under warnings-as-errors. The final verification uses MSVC 19.52 x64,
Windows SDK 10.0.26100.0, Rust 1.100.0, and the vendor-header-free
configuration.

```powershell
cmake --preset windows-x64-vs18 -B build/sr-d3d11-vs18 `
  -DFETCHCONTENT_BASE_DIR:PATH="$PWD/build/sr-d3d11-vs18-deps" `
  -DFETCHCONTENT_SOURCE_DIR_MINHOOK:PATH=D:/Coding/ReScaleFrame/build/windows-x64/_deps/minhook-src `
  -DFETCHCONTENT_FULLY_DISCONNECTED:BOOL=ON
$contracts = @(
  'rsf_anomaly_sr_swap_test', 'rsf_anomaly_sr_plan_test', 'rsf_anomaly_sr_profile_test',
  'rsf_anomaly_sr_hooks_test', 'rsf_anomaly_sr_jitter_test', 'rsf_anomaly_sr_pass_test',
  'rsf_anomaly_sr_runtime_api_test', 'rsf_anomaly_sr_reconstruction_test',
  'rsf_anomaly_motion_contract_test', 'rsf_anomaly_motion_hooks_test',
  'rsf_anomaly_plugin_test', 'rsf_anomaly_lifecycle_test', 'rsf_anomaly_bootstrap_test'
)
$buildOnly = @(
  'rsf_anomaly_sr_roundtrip_test', 'rsf_anomaly_sr_temporal_device_test', 'rsf_anomaly_sr_jitter_device_test',
  'rsf_anomaly_motion_device_test', 'rsf_anomaly_motion_tess_device_test',
  'rsf_anomaly_motion_tess_chain_device_test', 'rsf_anomaly_motion_far_device_test',
  'rsf_anomaly_fg_device_test', 'rsf_sr_backend_switch_device_test'
)
$sr = 'swap_(contract|unknown_selection)|plan_contract|profile_contract|hooks_(avx_)?contract|' +
      'jitter_(contract|unknown_selection)|pass_(contract|unknown_selection)|runtime_api_contract|' +
      'sun_cascades_contract|reconstruction(_window)?_contract'
$match = "^anomaly_(sr_($sr)|motion_(contract|unknown_selection|hooks_contract)|(plugin|lifecycle|bootstrap)_contract)$"
foreach ($config in 'Debug', 'Release') {
  cmake --build build/sr-d3d11-vs18 --config $config --parallel 5 --target rsf_game_anomaly @contracts @buildOnly
  ctest --test-dir build/sr-d3d11-vs18 -C $config --output-on-failure -j 4 -R $match
}
cargo fmt --all --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
```

Debug and Release each passed all 20 selected CPU contracts. These covered
swap, render plan, motion, jitter, reconstruction, the Stage 5 pass and
Runtime API, both profiles' SR hooks, and cold adapter loading. The Stage 5
fixture queried the real Runtime SR API directly and through the adapter's
by-name lookup. Both configurations built all nine device fixtures for the
build-only check. Cargo formatting and Clippy passed, as did all 232 locked
Rust tests, including eight new SR resource and draw contracts.

The source files, generated HLSL string payloads and NUL-terminated strings
embedded in both adapter builds and the staged DLL compare byte-for-byte
with the source at `2775855`. The shader compile helpers, entry names and
shader profiles still match the baseline.

| HLSL | Bytes | SHA-256 |
| --- | ---: | --- |
| `rsf_sr_roundtrip.hlsl` | 2,389 | `084b99524cbfa95a34e87634a964fc4b3ad7005b2fdd456fa39acce5c8295fd7` |
| `rsf_sr_upscale.hlsl` | 6,245 | `809536414d9ba79b73770581b7a9bee5d153ec33178c5d9975c1f60f34439a23` |

`dumpbin /exports` found the same ordered export pairs in both adapter
builds and in the staged `8322179a36c7` Anomaly DLL (SHA-256
`b978d99e2506d4d4864d267a109cd354a31e834c8e420d27dba4d8e9031fdca5`):
`1 rsf_anomaly_observation`, `2 rsf_anomaly_start` and
`3 rsf_get_game_plugin_api`. The import and binary-string checks confirmed
`GetModuleHandleW`, `GetProcAddress`, the UTF-16 Runtime DLL name and
the same session, SR, FG, presentation and overlay lookup names. The adapter
has no static import of that DLL.

This verification covers the source, PE boundary, CPU contracts and
fixture builds. Device execution, vendor output, moving-image quality
and live-game acceptance remain outside the evidence for this change.

## C11 D3D11 leaf and C-only SR headers (N06/N07 native pass)

`sr_d3d11.cpp` and the typed C++ facades in `sr_d3d11.h` are gone. Rust
registers the `rsf_sr_d3d11` table (`sr_d3d11/native.rs`,
`rsf_anomaly_sr_d3d11_register`); the first registration's foreign leaves are
kept, so the table address stays process-lifetime as before.
`sr_d3d11_platform.c` supplies the four leaves Rust cannot express:

| Leaf | Behaviour kept |
| --- | --- |
| `default_view` | `rsf_d3d11_create_shader_view` for an SRV with no template view. |
| `save_state` / `restore_state` | A zeroed `rsf_d3d11_state` (`calloc` for the former `new (std::nothrow)`), freed after restore; restore still runs on a null snapshot. |
| `guarded_log` | The foreign log sink runs under `__try` filtered on the MSVC C++ exception code `0xE06D7363`, the former `catch (...)` under `/EHsc`. Hardware faults still propagate. The exception object is not destroyed, as with any SEH catch of a C++ exception. |

The same file keeps the `D3D11_TEXTURE2D_DESC` field checks and the DXGI,
usage, bind and format-support values as `_Static_assert`s against the SDK.
`rsf_anomaly_sr_twin_error_name` (Rust) replaces `twin_error_name` with the
same names. `sr_d3d11_platform.h`, `sr_stage_platform.h`, `sr_stage.h` and
`sr_swap.h` now hold C declarations only; `sr_jitter.h` and `sr_pass.h` had no
users outside `sr_stage.cpp` and were removed. Callers use
`rsf_anomaly_sr_configure`, `rsf_anomaly_sr_install` and the Rust
`rsf_anomaly_sr_stage_*` exports; the N01 `stage_entries.h` entries forward to
them.

Checks: MSVC `ClCompile` with the project flags for every touched source,
MSVC and strict MinGW C11 syntax checks of the C headers and the leaf, and an
unchanged regeneration of `sr_stage_abi.rs` and `sr_swap_abi.rs`. The tip did
not link (Rust orchestrator errors), so no fixture or device run is part of
this evidence.
