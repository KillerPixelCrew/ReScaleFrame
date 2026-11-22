# Anomaly shader loading port

N05 follows the shader loader on `d12e6884` and the Rust compiler and Runtime
pack owner already on that base. It reuses the existing engine and shader
evidence. [Shader loader evidence](rust-anomaly-shader-loader.md),
[AVX profile](anomaly-avx-profile.md),
[Runtime patching gate](runtime-shader-patching-gate.md)

| Area | Source evidence and result |
| --- | --- |
| Family IDs | `shader_family.rs` assigns explicit IDs to the same eight families, preserving their C ABI values. Admission matches name and stage and returns the declared ID. Draws import those constants. Counters index by the ID and are sized from the table. The zero bucket prints `other` in reports and retains `none` in the family-name ABI. |
| Reflection | The loaded system `d3dcompiler_47` export returns an `ID3D11ShaderReflection`. Rust reads the version, all input/output/patch signature rows, resource bindings, and each buffer and variable in native order. One callback copies the complete description to current C++ consumers while the COM reflector and row storage are live. A failed descriptor read returns failure before publication. Compatibility and linkage continue to consume copied or borrowed snapshots under the existing synchronous contract. Both sides assert the private x64 layout. |
| Deployed-tree compile | The Rust tree owner still reproduces the stock DXBC before admitting a fresh variant. The fixture compiler now calls the existing `rsf-shader-compiler` owner with the selected export, empty source name, original flags and ordered macros. The Rust include vtable preserves `r3` before shared lookup, and the native path leaf still applies the same Windows narrow-path conversion and append rules. |
| Live compile | Anomaly still opens the already-loaded Runtime service and passes the engine's loaded compiler export. Rust checks the RIP-relative LEA at setup RVA plus six (`0x83b9ce` Standard and `0x832cce` AVX are the setup RVAs), signed address arithmetic, and both guarded includer entry prefixes before opening the pack. Native SEH wrappers still guard reads and the engine's include Open and Close calls. Runtime owns proof, cache, and variant creation. |
| Shader asset | `tools/anomaly_shader_constants.hlsli` retains identical HLSL bytes. Both the staging tool and its fixture use the new source filename. |

The standalone temporal fixture now lives in the Anomaly Cargo example
`anomaly_temporal_shader_device`. The Python verifier builds the locked
example under its existing local evidence directory and retains the TSV
case list and JSON report fields. The example uses the G06 compiler for
fixture VS/CS HLSL and reads the same FXC-built stock and patched DXBC.
The port keeps hardware with the debug layer, WARP fallback, VS stream
output, reflected b13 constants, the native MRT formats, per-sample MSAA
readback, and the current/prior clip, invalid history, alpha, stationary
motion, and jitter checks. The verifier now checks the pixel count against
the same alpha modes used to generate the case list. This is a source
comparison. The built binary and device results remain unverified.

This checkpoint has source and private ABI review, pinned 1.94.1 rustfmt,
and whitespace checks. A Rust regression fixture covers refusal on an
unreadable signature row. Builds, execution of that fixture, lint, device
checks, and Standard/AVX gameplay remain user-owned. No shaders, game
fingerprints, executable sites, or graphics-stage policy were changed.

## Live loader follow-up

The live Rust stage owns the pack box, checks the Runtime result and both
shader reflections, derives foliage constant offsets from stock reflection,
then creates and retains the variant. On success the published variant owns
only its reflection and shader/device leases. The Runtime's compile bytes are
copied within the prepare lease and released after shader creation. If create
returns an object with a failure result, the local lease releases it. Registry
linkage uses the retained Rust snapshots and still keys its cache by all bound
stock stages. Pack reload still prepares each replacement before publishing
the next generation.

Each snapshot owns the copied descriptor tables and reflected names while
preserving enumeration order. It can outlive both the COM reflector and the
pack whose compiler generated the variant. Both the live and native fixture
paths refuse an unreadable row using the same reflection owner.

The reconciled private motion platform changed from N04 ABI 10 to 11
and from 352 to 280 bytes. C++ and Rust assert the table size and the remaining shader callback
offsets. The adapter's existing Runtime shader-patch ABI stays at version 1.
The Standard/AVX profile still supplies the includer setup RVA and the
`d3dcompiler_43.dll` name. `engine_shader_includes.cpp` retains just that
profile handoff and the two SEH-protected calls into the engine vtable.

The live game target and SR hooks fixture no longer link
`shader_d3d.cpp`, `shader_loader.cpp`, or `motion_foliage.cpp`. The C++
motion contract and three deployed-tree device fixtures still use their
fixture facades for fake reflectors, preparation callbacks, and native
fixture inputs. These uses remain in `shader_d3d.cpp/.h`,
`shader_loader.cpp/.h`, and `shader_loader_platform.h`. The live caller-frame
layout declaration is now in
`shader_frame_layout.h`. Converting the remaining fixture facades would
also require changing those C++ motion fixtures.

## N04 reconciliation

The replay starts at `f40a25d5`. `6258fe6e` replays `872f68e0` with one
Windows Cargo dev-dependency table for both the temporal and tessellation
fixtures. `61e03df4` replays `768c7ced`. It keeps N04's generated frame
target and out-pointer return, including `written` and `main` at byte
offsets 20 and 21. The guarded views, viewport and bind helpers, callable COM
leaves, native draw-unwind scopes and retirement state machine retain their N04
owners. The typed pack validates and retains `ConstantOps` on admission.

The compiler caller-frame layout now comes from bindgen 0.72.1 and still
occupies 36 bytes with the same eight fields. The deployed-tree fixture facade
has a single compiler/reflector class. The Runtime session, engine SEH
includer and fixture-specific path conversion remain separate.

| PORT path | Source handoff |
| --- | --- |
| `games/anomaly/src/runtime_shaders.cpp`, `games/anomaly/src/runtime_shaders.h` | Closed by the live Rust pack and isolated `engine_shader_includes.cpp` leaf. |
| `tests/anomaly_temporal_shader_device.cpp` | Closed in source by the Rust example and existing Python entry point. |
| `games/anomaly/src/shader_d3d.cpp`, `games/anomaly/src/shader_d3d.h` | Retained for `tests/anomaly_motion_device.cpp`, `tests/anomaly_motion_far_device.cpp`, and `tests/anomaly_motion_tess_chain_device.cpp`. These fixtures construct the loader compiler/reflector, consume C++ descriptors, and use Windows filesystem include lookup. |
| `games/anomaly/src/shader_loader.cpp`, `games/anomaly/src/shader_loader.h`, `games/anomaly/src/shader_loader_platform.h` | The same device fixtures and `tests/anomaly_motion_contract.cpp` use these facades. The contract also uses mock compiler/reflector callbacks. `games/anomaly/src/motion_foliage.cpp/.h` and `motion_policy.h` refer to the fixture types. Their removal belongs with the corresponding fixture consumers. |

The scoped bindgen run reproduced the N04 target binding byte for byte.
Pinned formatting, locked offline Windows Cargo metadata and Git whitespace
checks cover this reconciliation. Build and runtime verification remain open.
## Motion and deployed-tree ownership follow-up from 0251a81a

This follow-up traces the mixed fixtures and live pack entry from the integrated
`0251a81a` tree. The profile, HLSL, patch assets, and source and DXBC digests
used by the fixtures still come from the same inputs.

| Path | Source evidence |
| --- | --- |
| `games/anomaly/src/engine_shader_includes.cpp` | Retired. `motion/stage/shaders.rs` calls `RuntimePack::open_engine` directly and receives an owned Rust value. The generated records in `engine_shader_includes.h` pass the selected profile's compiler name and setup RVA along with the native include operations. The two native engine calls still use their original SEH scopes in `motion_stage.cpp`. `runtime_pack.rs` retains the RIP-relative LEA and both entry-prefix checks, closes each successful include through its scope, and drops the Runtime session before the compiler module. MotionPlatform ABI 12 is 288 bytes. |
| `games/anomaly/src/shader_d3d.cpp`, `games/anomaly/src/shader_d3d.h` | Retired. The three deployed devices pass the admitted manifest and compile request directly to the Rust D3D owner. `shader_loader/d3d.rs` retains the compiler and reflector, compiles with the original macro order, flags and empty source name, and retains complete stock and variant reflection snapshots through preparation. The common Rust tree policy checks both reflection and the original bytecode reproduction before publishing a fresh variant. The native Windows path opener and C++ assertion projection remain in `shader_loader.cpp`. |
| `games/anomaly/src/motion_d3d11.cpp`, `games/anomaly/src/motion_d3d11.h` | Retired. `tests/anomaly_motion_device.cpp`, `tests/anomaly_motion_far_device.cpp`, and `tests/anomaly_motion_tess_chain_device.cpp` use the typed device-owner pointer, direct uploads and borrowed target views from the Rust ABI. The core fixture retains the native unwind test across Rust replay begin and end, including its OM and PS restoration assertions. Far and tessellation fixtures keep the existing hardware selection and G-buffer, depth and vector comparisons. |
| `tests/anomaly_motion_contract.cpp` caller-frame case | `shader_loader/frame/fixture.rs` checks both selected layouts supplied from `motion_profile.h`. It calls the typed decoder and checks the two return sites, ordered defines, cross-build and non-return refusals, missing terminator, failed entry read, invalid length and entry string limit. The old callback-based decoder and request publisher are removed. CTest still reaches the case through the existing contract launcher. |

The CPU loader's fake compiler and reflector still call the common Rust tree
policy through the C++ contract facade. The three deployed device fixtures retain
their native scene setup, D3D11 draws, readback and result assertions. Motion
constants, history and foliage remain mixed through `motion_bridge.cpp`,
`motion_constants.h`, `motion_history.h`, `motion_foliage.cpp/.h` and
`motion_policy.h`. The verified native profile, live detours and their native
hook fixture remain in `motion_profile.h`, `motion_stage.cpp/.h`,
`motion_stage_platform.h` and `tests/anomaly_motion_hooks_native.cpp`.

Source checks covered this diff, bindgen 0.72.1, pinned Rust 1.94.1 formatting,
locked offline Windows metadata and Git whitespace. Build, fixture execution,
lint and device or game acceptance remain open.

## CPU fixture and device scheduling port from b5a7434c

| Path | Source evidence |
| --- | --- |
| `tests/anomaly_motion_contract.cpp` | Retired. The 23 existing case names now dispatch from `motion/fixture.rs` and `shader_loader/fixture.rs`. The native `tests/anomaly_motion_contract_main.cpp` only passes the selection and the Standard/AVX layouts from `shader_frame_layout.h`. CTest still invokes the full contract and the unknown-case refusal. |
| `games/anomaly/src/motion_history.h`, `motion_policy.h`, and `motion_foliage.cpp/.h` | Retired. Rust fixtures call the production motion history, foliage identity/remapping, policy and b13 packers. They retain moving SKIN_0/SKIN_2 vectors, previous camera and instance retirement, detail reordering and conflicts, reflected foliage layouts, and packed-byte checks against the published shader layouts. |
| `games/anomaly/src/shader_loader.cpp/.h`, `shader_loader_platform.h` | Removed the native mock compiler and reflector ladder and descriptor projection. The typed Rust CPU cases check manifest admission and rollback, fresh and cached tree compilation, stock-bytecode refusal, reflection compatibility and full tessellation linkage. The device fixture holds opaque Rust snapshots. The native loader boundary still supplies Windows file opening and projects the result bytes and manifest rows for the native scenes. |
| `games/anomaly/src/shader_loader/runtime_pack.rs` | The fixture and live loader now call the same typed Runtime variant admission. The check order remains Runtime status, stock reflection, nonempty variant, variant reflection, compatibility and byte copy. The session owner, engine include SEH calls, source proof and pack reload transaction retain their existing boundaries. |
| `tests/anomaly_motion_device.cpp` | Rust schedules root proof, foliage layout review, core draw, tree draw and detail draw in the existing order. Rust checks the foliage layout and remaps and packs both grass-history scenarios. D3D11 draw, readback, per-pixel comparisons and the replay unwind scope remain native. |
| `tests/anomaly_motion_far_device.cpp`, `tests/anomaly_motion_tess_chain_device.cpp` | Rust schedules 41 far-character pairs, the seven default tessellation cases and the two opt-in HM cases, including the create-only probe and diagnostic continuation. It decides and packs the prior bone/world, viewport and domain constants and uploads them to the Rust device owner. Native scene code still creates shaders, binds geometry, draws and compares GPU outputs. The raw-DXBC probe and driver crash reporting also remain native. |

`motion_bridge.cpp` and `motion_constants.h` retain the packing and matrix
calls used by the native device scenes and the existing SR jitter fixture.
The native game hook and its fixture still own the verified engine ABI and
unwind scopes. The shipping game SDK, live SR and startup paths, profile
data, shader files and embedded fixture shader strings were unchanged in
the source diff.

The source comparison matched all 23 CPU names, nine far pair templates
and nine tessellation labels against `b5a7434c`. MSVC x64 syntax-only
checking with the repository's warning flags passed for the changed native
fixture units. Bindgen 0.72.1 reproduced the schedule ABI after pinned
Rust 1.94.1 formatting. Locked offline Windows metadata resolved both
fixture features. Build, linking, lint, fixture and device execution remain open.

## Draw and readback port from 6c935471

`6c935471` already contains the fixture ownership replay at `2f8d4721`.
This follow-up uses that integrated tree directly.

| Path | Source evidence |
| --- | --- |
| `motion/device_fixture/draw.rs`, `draw/resources.rs` | Rust creates the fixture position, albedo, depth and raster state, applies the stock signature's 16-byte input elements, binds the triangle or three-control-point patch topology, and issues the ordinary draws. Core and foliage keep D24S8, less-or-equal and depth-plus-stencil clears. Far and tessellation keep D32F, less and depth-only clears. The native fixtures still choose the exact shader objects and the two- or three-target output for each pass. |
| `motion/device_fixture/draw/bindings.rs` | The same D3DCompiler 47 reflection drives stock b13 exclusions, slot enumeration, identity matrices and default values. Far and core retain the 0.5 alpha reference. Core binds textures and clamp samplers on PS only. Far and tessellation retain wrapping samplers and per-stage SRVs. Tessellation retains the 4-by-4 displacement ramp. Foliage still copies and maps the bound VS buffer before patching named values and rebinding it. Fixture owners hold the emitted COM resources through the same scene or draw scope. |
| `sr_fixture_readback.rs`, `tests/anomaly_motion_native.h` | The three motion fixtures share the Rust staging copy, map, row-pitch copy and unmap. D32F adds a four-byte texel to the existing fixture helper. C++ allocates the same destination element type and extent for each comparison. |
| `shader_loader/d3d.rs`, `shader_loader.cpp/.h` | Rust emits the simulated engine's stock DXBC and then independently recompiles the original tree to prove that DXBC before allowing the motion variant. The stock-only call and supplied-bytecode path remain for the tampered and foreign permutation refusals. The compiler entry, flags, ordered defines and include lookup order are the same. Native Windows path opening preserves the STL narrow filename conversion and root/drive/UNC append behavior. |

The three native scene files retain their geometry data, shader creation,
diagnostic probes, assertion loops and COM attachments at the call boundary.
The core fixture's intentional C++ throw and both replay draws still run
inside the native restoration scope. The live compiler trampoline, engine
originals and SEH operations remain in the existing native stage.

The source comparison matched shader assets and raw shader strings, the
far and tessellation comparison bodies, core and grass comparison blocks,
and the core replay unwind callback against `6c935471`. Pinned 1.94.1
formatting, bindgen 0.72.1 reproduction, locked offline Windows metadata
and MSVC x64 syntax-only checks passed. Build, linking, fixture execution,
lint and device acceptance remain open.
