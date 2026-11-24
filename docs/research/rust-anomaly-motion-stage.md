# Anomaly motion-stage owner in Rust

This cutover answers who owns the motion stage between the native shader, scene
and draw callbacks. It starts from the merged adapter at `a68567a`. The
[staged C++ baseline](rust-anomaly-cpp-baseline.md) at `8322179a36c7` supplies
the DLL and shader pack for comparison. The [Standard and AVX fingerprints and
source revision](../../games/anomaly/engine.json) still govern the engine
boundary. This work uses the verified engine addresses, expected bytes,
buffer layouts and shader rules.

The native shim keeps the exact `create_shader`, `phase_scene_begin` and
`phase_scene_end` guards and trampolines, both verified compiler return sites,
the empty source-name and D3DCompile setup guards, and the hull/domain creation
and wrapper guards. It still reads the selected profile's D3D11 device and
context, RCache, engine frame and projection fields under the existing guards.
The actual write to the RTV2 cache and the constant-buffer copies remain behind
the native SEH leaves. The Draw/DrawIndexed/instancing entry points keep their
original calling conventions. The private stage table checks its size and
version before Rust uses the leaves. The native guard implementation matches
the pre-port source byte for byte.

RTV2 stores current minus previous viewport UV, with Y down, validity in the
third channel and zero in the fourth. The existing b13 and replay owners
retain the draw and resource boundaries described in the earlier shader and
producer evidence. This scope uses that inventory for unchanged forward, UI
and unsupported geometry paths.

| Boundary | Owner and evidence |
| --- | --- |
| Startup and settings | Rust reads the motion INI options and registers with the existing overlay registry. Observe mode still prepares variants with Motion off so the player can enable it live. Disable, foliage, invalidation, reload and shader dump use the existing controls. The capture controls save the schedule for the next start, when the process assigns the Runtime dump service. `SRStageCapture` retains precedence. |
| Shader creation and reload | After the engine creates its stock shader, Rust admits the guarded compile frame, identifies the exact source hash and selects the corresponding pack entry and stage. [The existing loader](rust-anomaly-shader-loader.md) still checks the compiled original, cache result and reflection. Native leaves supply the exact compiler and engine includer, D3D11 creation and reflection storage. Rust owns the pack, immutable original request, device, stock and replacement leases. Reload publishes the replacement registry only after preparation and object creation succeed for every retained root. The previous generation stays usable on failure. Successful reload clears linkage and both histories. |
| G-buffer window | Rust consumes the native CRenderTarget, its `rt_Position` CRT at `+0x378` and the engine's Main/Scope identity. It runs the existing target and binder owners and sizes RGBA16F RTV2 to the actual position texture, including an SR twin. Main and Scope histories remain per-view and split windows reuse the existing frame. A new device clears linkage and histories. The published `FrameTarget` still carries the native engine frame, Main flag, extent and written flag for the SR seam and SR-off FG. [Producer and seam evidence](anomaly-sr-seam.md) |
| Core, foliage and tessellation draws | After the original `CBackend::constants.flush`, Rust records the supplied camera and copied draw snapshot and invokes the existing history decisions and b13 packers. Core rigid and SKIN_NONE use the previous world, SKIN_0 uses the bone-composed world, and hardware skinning uses the previous palette. The stock-reflected tree and detail layouts retain their staged constants, per-view history, route and row poison. A verified VS/PS pair can swap directly. A bound hull or domain shader requires a core VS and a verified, same-device VS/HS/DS/PS chain. Geometry shaders, missing stages and unlinked interfaces keep stock. [History](rust-anomaly-motion-history.md), [foliage](anomaly-foliage-motion.md), [tessellation](anomaly-tess-motion.md), [character elements and distance counters](anomaly-far-character-motion.md) |
| Draw completion | The existing thread-local pending action accepts only the matching immediate context. A prepared replacement chain pins its objects through the draw and stock restoration, including cancellation at the next flush or scene boundary. Refused draws keep the existing optional zero-motion replay. Its native state scope preserves the depth-write test and restores the touched GPU state. The Runtime copies the three motion diagnostic images under the accepted capture record. |

The old capture scheduler tested `stats.frames`, which counted both Main and
Scope window openings, against a schedule documented in main-view frames. The
old close path also completed whichever active record it held without checking
the closing window's identity. Rust counts distinct Main engine frames and
requires the accepted reset generation, immediate context, position resource
and motion texture at close. It spends the requested count only after the
Runtime accepts the record. A fake dump-table regression interleaves Scope, a
repeated Main opening, an admission refusal and a close from a different reset
generation. It checks the exact source IDs and counts the emitted images.

The previous variant registry could replace an entry under a given stock address
without invalidating the cached linkage decision for that address. The Rust
registry invalidates the cache on replacement. A CPU regression replaces a
linked VS under the same key with one that fails linkage, while a pending chain
keeps the former shader alive until it is dropped. The selection tests also
cover foreign devices, partial tessellation, bound geometry and four-stage
linkage.

## Verification

`build/anomaly-motion-stage` and its FetchContent directory are in the worker
worktree. The MinHook source was read from the coordinator's pinned
`c3fcafdc10146beb5919319d0683e44e3c30d537` checkout. The three yyjson inputs
were copied only after matching `runtime/shader_patch/yyjson.lock.json`. CMake
used the coordinator's staged vendor directory for device fixture registration.
Vendor SDK headers were absent from this build configuration.

```powershell
cmake --preset windows-x64-vs18 -B build/anomaly-motion-stage `
  "-DFETCHCONTENT_BASE_DIR=$PWD/build/anomaly-motion-stage/_deps" `
  "-DFETCHCONTENT_SOURCE_DIR_MINHOOK=D:/Coding/ReScaleFrame/build/windows-x64/_deps/minhook-src" `
  "-DRSF_ANOMALY_SR_VENDOR_ROOT=D:/Coding/ReScaleFrame/build/releases/rdr2/stage-rdr2-a2bcc0c3/ReScaleFrame"
$targets = @(
    'rsf_game_anomaly', 'rsf_anomaly_patch_pack', 'rsf_anomaly_plugin_test',
    'rsf_anomaly_lifecycle_test', 'rsf_anomaly_bootstrap_test', 'rsf_anomaly_motion_contract_test',
    'rsf_anomaly_motion_hooks_test', 'rsf_anomaly_sr_hooks_test', 'rsf_anomaly_geometry_hooks_test',
    'rsf_anomaly_motion_device_test', 'rsf_anomaly_motion_tess_device_test',
    'rsf_anomaly_motion_tess_chain_device_test', 'rsf_anomaly_motion_far_device_test',
    'rsf_anomaly_fg_device_test', 'rsf_anomaly_fg_policy_test')
$cpu = '^anomaly_(plugin_contract|lifecycle_contract|bootstrap_contract|motion_contract|motion_unknown_selection|motion_hooks_contract|shader_tool_contract|sr_hooks(_avx)?_contract|geometry_hooks(_avx)?_contract|fg_policy_contract|fg_policy_unknown_selection|input_events_(standard|avx)_contract)$'
foreach ($configuration in 'Debug', 'Release') {
    cmake --build build/anomaly-motion-stage --config $configuration --target $targets --parallel 2
    ctest --test-dir build/anomaly-motion-stage -C $configuration -R $cpu --output-on-failure
}
cargo fmt --all --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
git diff --check
```

MSVC 19.52.37029 and Windows SDK 10.0.26100 built the adapter, the
four motion device fixtures and the FG device fixture in Debug and Release.
Each configuration passed 15 CPU contracts. Four cases inside the shader-tool
suite skipped because the licensed vanilla fixture was absent. Rust 1.100.0
passed formatting, workspace Clippy and 251 locked workspace tests, including
the three new motion-stage cases.

DUMPBIN and the PE/disassembly audit compared both builds to the staged
`8322179a36c7` `ReScaleFrame.Game.Anomaly.dll`. The export directory name and
the full name and ordinal inventory match:

| Ordinal | Export |
| --- | --- |
| 1 | `rsf_anomaly_observation` |
| 2 | `rsf_anomaly_start` |
| 3 | `rsf_get_game_plugin_api` |

Both builds borrow `ReScaleFrame.Runtime.dll` with `GetModuleHandleW`.
The bootstrap session prepare and start queries, the shader-patch API
and all eight motion input-dump functions resolve by name with
`GetProcAddress`. The staged Runtime exports these names. The PE import and
delay-import tables have no Runtime DLL entry. The shader API module and export
lookup call RVAs are `0x4d9f8` and `0x4da11` in the baseline, `0x26dcf6`
and `0x26dd1e` in Debug, and `0x900b8` and `0x900d1` in Release.

Both 95,049-byte motion packs matched the staged pack byte for byte with
SHA-256 `d2aa077a561ee71c084f806edb2f7cdcb0bf4ccaa17907a788324043f8622bb2`.
The device executables were built for later testing. Live detour admission,
real shader acquisition and reload, motion images and moving-game SR and FG
output remain unverified on this revision.

## Profile guard admission

Rust now owns the motion install guard sequence in
`engine_guards/motion.rs`, following the SR and geometry guard ports and
using the shared checked-image reader. The platform `guards` leaf keeps its
signature and platform ABI 8. It now only lends the selected Standard or AVX
profile from `motion_profile.h`: three detour sites, the unique prefixes, the
two `create_shader` call sites, both hull/domain create sites with their
`CreateHWShader` wrappers, the `shader_compile` prologue, D3DCompile setup and
tessellation dispatch codes, their RVAs and the `pSourceName` RVA. The guarded
copy leaf comes from `guard_image`. The tables, typed trampolines, HookSet and
the Rust install refusals that run before the guard (image, size, profile,
build identity, `hooks_created`) are unchanged.

Rust keeps the C++ short-circuit order and names. Each detour site checks its
16 bytes, then each unique prefix at the same RVA in table order, and refuses
with the site name. Then come both call sites by name, the `shader_compile`
prologue, the D3DCompile setup, the hull/domain dispatch, each create site
prologue by name followed by its wrapper (`CreateHWShader (hull/domain)`), and
finally the single zero byte at `pSourceName`. A null result admits the build.
The old `"unknown"` default could never be returned, because every refusal
path set a name. A new Rust case breaks each check from last to first on a
fixture image and requires the earliest one's name each time.

Source comparison against the C++ `guards_match` at `a5867f2f` matched the
input fields, order and refusal strings. Both versions reach the same
`matches_site` and `matches_hex_guard` reader. The dynamic Runtime boundary,
the `GetModuleHandleW` and `GetProcAddress` lookups, and the existing motion
tests are unchanged. No engine analysis or instruction site was added. Pinned
Rust 1.94.1 formatting, locked offline Windows Cargo metadata and Git
whitespace checks passed. Compile, link, Clippy, tests, fixtures and game
verification remain user-owned.

## Motion window admission

Compared with `bfbed34e`, Rust now checks the scene callback's
target against the guarded `CRender::Target` read before requesting
the position RTV and surface. The native leaves still read
`HW.pDevice` and `HW.pContext` first, then the guarded position
fields from the same profile. After both views are present, Rust
reads the native frame, render view and reset generation in the
original order. The typed engine reads and the verified Standard
and AVX profiles match the baseline.

On a fresh motion window, Rust applies the same SR-stage MSAA mask
to the options word. An unreadable word still counts as MSAA and
skips the texture descriptor query. The native leaf still borrows
the source surface for the typed D3D11 texture query. The motion
target creation, clear, history and capture order match the baseline.

The private platform table moves to version 9 and 352 bytes.
Source comparison covered the callback positions, the short-circuit
reads and public declarations. Pinned formatting, locked offline
metadata and whitespace passed. Native linkage, fixtures, and
Standard and AVX motion and fallback checks remain open.

## Stock shader creation admission

Compared with `9c64d6d6`, the detour still attaches the device before
the engine call. It returns the original result and lends the signed
low 32 bits, target, bytecode, output object, return address, and
caller stack to Rust. The stack still starts eight bytes after
`_AddressOfReturnAddress` on the native detour frame.

Rust checks the successful result and active stage before reading
engine memory. It admits the same vertex, pixel, hull and domain
targets, then reads the stock interface at the verified object
offset `0x10`. An overflowing address now refuses before the
guarded read. The filename keeps the readable prefix of at most
520 bytes. The original compiler-frame decode and stock-bytecode
copy still use guarded reads, and the request target retains its
reserved terminator.

The private creation record is 56 bytes on x64. Source review covered
the detour, reader order, ABI declarations and unchanged Standard
and AVX profiles. The added fixture covers hull admission, a partial
filename and overflowing object addresses. Compile and link, fixture
execution, live shader reload and motion checks remain open.

## Native draw calls and restoration

At `a7ea5419`, the motion stage invoked both the original draw and
the zero-motion replay through Rust callback frames. The C++ detours
now call both on native frames. Rust still takes the pending action
only for its matching context, pins the shader chain or replay
resources, and decides whether the second draw is eligible.

An active replacement restores the stock shaders after the original
draw. A retired replacement restores them before the original draw
and keeps the shader chain alive until the call exits. Replay keeps
its earlier active check, depth-write gate, zero pixel shader and
restoration sequence. The native scope guards ask Rust to restore
and release saved state during C++ unwind as well as normal return.
Completion counters advance only for calls that returned.

The two scopes use caller-owned storage and allocate no heap memory
per draw. The private stage table, native detour signatures, profiles
and shader pack match the baseline in source. The motion device
fixture now injects a C++ exception during replay and checks that the
depth state, render targets and pixel shader are restored. The fixture
and live Standard and AVX checks remain unrun.

## N04 D3D11 leaves and target cleanup

Source baseline: `846e3a45` and the producer and shader evidence above.
The N04 port moves resource creation, upload, RTV clear and binding, shader
binding and replay state reads and writes to Rust. It uses the pinned Windows
COM declarations and the shared System32 compiler. The engine-specific thunk
keeps its C++ call frame for compiler stack capture and guaranteed restoration
when an original native draw unwinds. Guarded engine reads and executable
fingerprints are unchanged. The fixture facades still lend their existing
records to the Rust owners.

The replay call snapshots eight RTVs and the DSV, depth state and stencil
reference, blend state and factors and sample mask, plus the pixel shader and
up to 253 class interfaces. The Rust scope restores this state after the
original draw and releases each received COM reference, including the path that
refuses depth-read-only replay. The stock shader binding query releases only
the GetShader reference. Its render callback borrows the engine binding and
the selected replacement chain separately pins its stock and replacement
objects through restoration.

A window now revalidates the position RTV and surface on replacement even if
the source frame, view, generation and context are unchanged. Window admission
reports unreadable options, the observed MSAA bit, unreadable descriptors,
unsupported sample or array counts and allocation failure independently.
When an RTV may still be borrowed by RCache, cleanup checks the matching
backend and context. A missing or foreign cache refuses cleanup and leaves
the device owner live. The contract fixture now checks both refusal routes
and a successful retry on the original cache.

Model and foliage history share the same frame identity and rotation decision.
Foliage reuses its maps and model history keeps retirement watermarks. Motion
reads the geometry verdicts and draw kinds from their shared definitions.
Core packing reports invalid matrices separately from bone overflow while the
native boolean wrapper still emits invalid b13 bytes on either refusal. Pack
constant operations now come from the opened pack's API table, without
interpreting its handle. The motion platform ABI is 10. Motion and FG share
one generated target payload, with the FG caller's existing flag offsets.

Pinned Rust formatting, scoped bindgen regeneration, locked offline Cargo
metadata and Git whitespace checks passed. This is source evidence. Builds,
lint, fixture execution, device checks and game acceptance remain user-owned.
The device fixtures continue to cover native D3D11 entry points, including
deployed or hardware-dependent cases. The foliage shader device harness is
still built by its Python driver.

### N04 checklist handoff

Source port closures with retained FFI or fixture declarations:

- `games/anomaly/src/motion_bridge.cpp`
- `games/anomaly/src/motion_constants.h`
- `games/anomaly/src/motion_d3d11.cpp`
- `games/anomaly/src/motion_d3d11.h`
- `games/anomaly/src/motion_device_platform.h`
- `games/anomaly/src/motion_foliage.cpp`
- `games/anomaly/src/motion_foliage.h`
- `games/anomaly/src/motion_history.h`
- `games/anomaly/src/motion_policy.h`
- `games/anomaly/src/motion_stage.h`
- `games/anomaly/src/motion_stage_platform.h`
- `games/anomaly/src/motion_target.cpp`
- `games/anomaly/src/motion_target.h`
- `games/anomaly/src/motion_target_platform.h`

Closed Rust findings:

- Use the geometry producer's draw kinds and verdicts in motion history selection.
- Distinguish an unreadable render option from observed multisampling.
- Rotate Anomaly model and foliage histories from one frame identity.
- Use one motion frame-target description between Anomaly motion and FG.
- Keep the motion pack's opaque handle opaque to the draw stage.
- Invalidate window readiness when the engine replaces the position surface.
- Distinguish no binding from a binding that cannot be inspected.
- Reuse the Anomaly host-log callback wrapper in motion and FG.
- Keep matrix validation failures distinct from bone-count refusals.
- Anomaly motion device and binder tables unwrap bindgen Option leaves on every use.

The reconciled branch also closes these Rust findings:

- Use the range-aware option builder for Anomaly motion controls.
- Use one Anomaly frame-dump directory resolver for SR and motion.

The adapter option builder keeps N02's boolean apply/save callback, including
false on a refused INI write. Motion's capture interval remains restart-marked,
with 1 as its minimum. SR and motion use the existing SR environment path
length rule and retain their separate admission and capture scheduling.

The reconciled branch ports these fixture paths:

- `tests/anomaly_motion_hooks.cpp`. Rust checks the event sequence and scope
  facts. The native launcher still drives the real detour and constructs the
  production C++ scopes.
- `tests/anomaly_motion_tess_device.cpp`. The Rust example preserves the HLSL
  probe, NVIDIA-first hardware selection, debug/no-debug assertions and CTest
  skip code 77. The shader body is unchanged.

Open port paths are `games/anomaly/src/motion_stage.cpp` and
`games/anomaly/src/motion_profile.h`. Rust derives the four immediate-context
draw slots from the pinned SDK vtable. It reads the position CRT views through
the shared guarded geometry reader and samples the viewport with the SDK
binding. The C++ trampoline retains the engine stack address and the
draw-unwind scopes, as well as the remaining engine and shader glue. The
profile header supplies fingerprinted data consumed by the C++ fixture and
Python evidence readers.

The remaining fixture ports are `tests/anomaly_foliage_shader_device.cpp`,
`tests/anomaly_motion_contract.cpp`, `tests/anomaly_motion_device.cpp`,
`tests/anomaly_motion_far_device.cpp` and
`tests/anomaly_motion_tess_chain_device.cpp`. They still exercise native D3D11
or C++ fixture adapters. The binder contract adds cleanup refusal coverage
and the MotionDevice-backed tests use the Rust replay and resource calls.

### Reconciliation from 89f6df5d

The FG table keeps the integrated size-checked shape and Rust COM bindings.
The new motion target replaces only its return payload. The integrated Runtime
Auto selector, FG capture decoder, history retirement policy and source
readiness rules remain in their original owners. Cache cleanup can accept
proof that the original slot is empty after an immediate-context change. It
still refuses to issue an OM clear on a different context when that slot
contains the borrowed RTV.

The prior source commit remains on its original branch. The reconciled replay
starts after `89f6df5d` with `e77c8f3e`. N05 follow-up commits are prospective
and were compared by a read-only three-way merge. Keep N05's standalone
temporal fixture alongside the N04 fixtures and combine the Windows dev
features in one Cargo table. For `768c7ced`, keep N05's typed RuntimePack
ownership and borrowed reflection logic. Fill ConstantOps during typed pack
admission, preserve N04's generated frame-target return and guarded draw
helpers, and converge the C++ and Rust stage ABI to the resulting layout
together. Keep the N04 removal of production motion fixture sources as well
as the N05 engine-include source rename.

### Independent fixture ports from f40a25d5

The standalone foliage device fixture now has a Rust driver. The Python
harness still uses FXC and the local licensed DXBC corpus to produce the same
seven VS variants and flat/bump PS pairs. The Rust driver uses Windows SDK
reflection for constant slots, packing and generated signature probes. It
compares stream-output current and previous clip with the original shaders,
checks both native MRTs and RGBA16F motion at texel (8,8), and retains the
missing-history, missing/invalid foliage state and row-index cases. It tries
hardware and then WARP, as the native fixture did. The Python harness builds
the Windows x64 MSVC Cargo example in its temporary staging directory.

The RTV2 binder case in the mixed CPU fixture now calls the production Rust
binder directly. The fake cache uses the same RT/ZB/dirty offsets. The Rust
case retains the native test cases for rebind, foreign targets, unavailable
cache, write refusal and cleanup retry. It also covers both replacement-context
results: a borrowed RTV blocks cleanup, and an empty original slot permits
retirement. CTest still reaches it through the same contract case. The
C++ virtual BinderHost and its two adapter files were removed.

All three deployed motion devices now use Rust SDK admission before entering
their existing fixture scenarios. The far and tessellation-chain drivers keep
hardware-only admission with NVIDIA-first selection, feature level 11 and
optional debug fallback. The core driver keeps its separate first NVIDIA
or default hardware choice, retries without debug and then tries WARP. Rust
returns owned COM references, which the C++ callers adopt in their existing
ComPtrs. Compiler choice, fixture skip codes, deployed shader callers and
the frame comparisons stay in the original drivers.

New PORT source closures:

- `games/anomaly/src/motion_target.cpp`
- `games/anomaly/src/motion_target.h`
- `tests/anomaly_foliage_shader_device.cpp`

The four mixed fixtures remain open: `tests/anomaly_motion_contract.cpp`,
`tests/anomaly_motion_device.cpp`, `tests/anomaly_motion_far_device.cpp` and
`tests/anomaly_motion_tess_chain_device.cpp`. Their remaining code consumes
the N05 manifest, reflection and loader facades, along with typed motion
packers and device helpers. The fixture port in this change leaves those
caller hunks intact.

Remaining native inventory and source boundaries:

| Paths | Remaining use |
| --- | --- |
| `motion_bridge.cpp`, `motion_constants.h`, `motion_history.h`, `motion_policy.h` | Mixed C++ fixtures still construct typed snapshots and byte spans, borrow previous history, and translate result flags across their Rust ABI. The Rust packing and policy owners are already in production. |
| `motion_d3d11.cpp`, `motion_d3d11.h` | Typed COM compatibility surface for the deployed C++ drivers. The device and replay implementations already live in Rust. The replay template owns a C++ scope whose destructor restores state when the caller throws. |
| `motion_foliage.cpp`, `motion_foliage.h` | Borrow native reflected stock layouts and carry the C++ fixture history inputs and finite-lifetime outputs. N05 is reconciling the reflection owner and live callers. |
| `motion_profile.h` | Verified executable profile data is consumed by the shared build selection, compiler-frame fixture and Python evidence readers. Changing its source joins the N05 compiler-frame work. |
| `motion_stage.cpp`, `motion_stage.h`, `motion_stage_platform.h` | The shader trampoline lends its original stack frame and the native draw guards preserve restoration across C++ unwind. N05 is reconciling the stage table and shader preparation. |
| `tests/anomaly_motion_hooks_native.cpp` | The Rust driver already checks order and facts. The remaining launcher selects the real C++ detour, builds its PE-backed relocation target and creates the production RAII draw/context scopes. It remains an open native fixture path. |

The source check compares the native fixture conditions against the Rust
drivers and the installed windows 0.62.2 signatures. Pinned Rust formatting,
unchanged scoped binder bindgen output, Python parsing, locked offline Windows
Cargo metadata and whitespace checks were used. Build, link, fixture execution
and visual or device acceptance are outside this source-only check.

### Final N04 port: device fixtures and the motion profile

The three deployed device fixtures are now Rust (`motion/device_fixture/core_device.rs`,
`far_device.rs`, `tess_chain_device.rs`) behind C11 launchers in `tests/`. Each case was ported line by
line from the C++ driver it replaces: the same deployment lookup (`RSF_ANOMALY_MOTION_DEPLOY`, then
`.local/anomaly/motion-deploy[-tess]` under the repository root, now from `CARGO_MANIFEST_DIR`), skip code
77 conditions, define sets, geometry, CPU models, tolerances, coverage minima and debug-layer rules. The
deployed-tree shader path goes through `shader_loader/deployed.rs`, a fixture-only Rust facade over the
same `compile_tree`/`prepare_tree` code the removed C++ projection reached through the C ABI. The scene
math and the octahedron mover shared by the far and tessellation-chain drivers live in `scene.rs`; the
GPU, readback, binding and owner helpers in `harness.rs`.

One check changed form. The C++ driver threw a C++ exception out of the replay redraw to show that the
native scope restored the OM state on unwind. The Rust driver panics out of the redraw inside
`catch_unwind` (panic hook silenced) and checks the same restored state: the replay scope's `Drop` now
does the restoring, which is what the Rust owner needs to prove. The tessellation driver's crash filter
(`SetUnhandledExceptionFilter`) and its RSF_TESS_* diagnostics are kept.

The per-build motion profile moved from `motion_profile.h` to `motion_profile.rs`, in native_profile.rs
build order with each build stating its own sites, guards, caller frame and hex guards. Rust now runs the
guard check (`engine_guards/motion.rs` over the Rust record), derives the caller-frame layout and the
engine compiler profile, and lends the three detour sites to the native leaf, which only adds its
trampolines. The stage selects the record from the SHA-256 the native installer passes, and refuses when
none matches. `rsf_anomaly_motion_platform` was ABI 13 (272 bytes): `guards`, `shader_profile` and
`frame_layout` are gone, `copy` (the guarded copy behind the guard image) and the site list in
`install_hooks` are new. Round 5 removed the table: Rust calls the `rsf_anomaly_motion_native_*` leaves
by name. `tools/anomaly_motion_evidence.py` and the motion case of
`tools/anomaly_profile_evidence.py` parse the Rust profile; the parser was checked offline against both
build sections (5 sites, 12 integer fields, 3 hex guards, 2 hull/domain sites, 1 unique guard per build,
hex lengths equal to the removed C++ table). The evidence scripts themselves need the installed
executables and were not run.

`motion_stage.h` and `motion_stage_platform.h` now hold only C declarations. `build_profile.h` no longer
carries a motion record and the motion code no longer includes it; it still has C++ users through the
geometry (`geometry_profile.h`) and SR (`sr_profile.h`) records (N03, N06 and their fixtures).

After N03 (C11 geometry producer, `bff85670`) and N06 (`sr_profile.rs`, `rsf_anomaly_sr_stage_jitter_uv`)
landed, nothing in `motion_stage.cpp` needed C++ any more, so it became the C11 leaf
`motion_stage_platform.c`:

- The create_shader detour still lends `_ReturnAddress()` and `_AddressOfReturnAddress() + 8` (the caller's
  rsp at the call) to Rust's frame decoder. The G-buffer window detours are unchanged.
- The four immediate-context draw detours share one `draw_call` record. The draw scope and the replay scope
  are retired on return and during a C++ unwind out of the engine's draw: `__try/__finally` under MSVC, a
  cleanup handler under GCC (`-fexceptions`, already set for the anomaly C leaves). As in the C++ RAII
  version, the draw scope closes after the replay, with `completed` set once the engine's draw returned.
- The engine includer is called through its vtable inside `__try/__except`, as before.
- The engine reads use `rsf_anomaly_motion_engine`, which Rust fills at install from `native_profile.rs`
  (Device.dwFrame) and `sr_profile.rs` (HW device and context, RCache, RImplementation.Target, CRender::o
  word 3, rt_Position, the CRT RTV and surface offsets, RCache.xforms m_v and m_p). The RTV2 binder layout
  (`BACKEND_RT_OFFSET`, `BACKEND_ZB_OFFSET`, `BACKEND_RT_CHANGED_OFFSET`) is Rust's own constant now.
- `rsf_anomaly_motion_stage_install` takes the build index the observer admitted; Rust resolves the native,
  SR and motion records from it and refuses an unknown index. The platform is ABI 13, 256 bytes.

`tests/anomaly_motion_hooks_native.cpp` became `tests/anomaly_motion_hooks_native.c` over the geometry C API.
Its only C++ input was the flush site index from `geometry_profile.h`; the Rust fixture now finds the
`CBackend::constants.flush` site in `geometry_profile.rs` and passes the index to the launcher's install.

Checks: `cargo fmt --all -- --check`; `cargo clippy -p rsf-anomaly --locked` with the default features and
each of `motion-binder-fixture`, `motion-hooks-fixture`, `motion-device-fixture`, `native-entry` and
`native-hook-fixture` (no new warnings in N04 files; two N02/N08 compile errors at the tip were patched
locally, not committed, so the crate type-checks). The crate's lib test target does not compile at the tip
because of other chunks' test modules, so `cargo test` could not run. MSVC 14.44 `/Zs /W4 /WX` passed for
`motion_stage_platform.c`, the five C11 launchers, `sr_stage.cpp`, `tests/anomaly_sr_profile.cpp`,
`tests/anomaly_native_hook_leaves.cpp` and C and C++ units including the stage headers; MinGW GCC
`-fsyntax-only -std=c11 -Wall -Wextra -Werror -fexceptions` passed for the leaf and the launchers.

`rsf_anomaly_motion_hooks_test` was built (CMake needed two local, uncommitted fixes in
`tests/CMakeLists.txt` and `tests/anomaly_fg.cmake` to configure at the tip) and CTest
`anomaly_motion_hooks_contract` passed. Every executable that links the crate fails to start at the tip with
0xc0000139 because the overlay's window subclassing imports `comctl32.dll!GetWindowSubclass`, which only
Common Controls v6 exports, and the test executables carry no v6 manifest; the pass was measured after merging
a Common Controls v6 dependency into that one built executable with `mt.exe`. The contract, device and plugin
targets could not link: they depend on `rsf_rust_core` (orchestrator), which does not compile at the tip. The
device fixtures were not run, per the port rules.

Two fixtures outside N04 lent the C++ motion record to their Rust checks and were changed with it:
`tests/anomaly_sr_profile.cpp` (its contract now reads the motion sites, calls and unique guard from
`motion_profile.rs`; the native projection drops the motion owner and banks) and
`tests/anomaly_native_hook_leaves.cpp` (lends only the SR bank; the Rust hook fixture installs the motion
sites from `motion_profile.rs`). `sr_stage.cpp` reads the motion target through
`rsf_anomaly_motion_stage_frame_target`.