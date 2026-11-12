# STALKER Anomaly RSF integration: progress tracker

Checkpoint: 12 November 2026, CET. Work started 11 November 2026.
Status: resumed 12 November on the user's request; wave 1 (geometry integration, Runtime overlay
API, Debug gate fix) is complete. **Full in-game integration is NOT complete.**
Worker specs and reports: `.local/anomaly/orchestration/`.

## Goal and non-negotiable constraints

- Add RSF upscaling, frame generation and the shared RSF overlay to Anomaly 1.5.3.
- Deliver through a DLL shim. Do not replace/rebuild either game executable.
- Treat `D:/xray-monolith` as read-only reference source.
- Keep detection, signatures, native layouts, shader conventions, engine input and
  settings policy in `games/anomaly/`. Shared runtime/presentation/overlay services
  remain game-independent, with one Runtime DLL owner.
- Separate world render resolution, presentation/UI resolution and PDA content
  texture dimensions. A physical PDA is scene-attached material content, not an
  output-space HUD merely because its texture was drawn by UI code.
- Do not advertise detection, CPU history, compiled shaders or device fixtures as
  working game SR/FG. Rendering-readiness flags remain false.

## Current acceptance matrix

| Area | Verified today | Still required |
| --- | --- | --- |
| Source/shader research | Packed r3 inventory, colour/depth/UI contracts and guarded native-boundary mappings | Complete live producer/pass/permutation coverage |
| Thin DirectInput carrier | Native forwarding, export/ordinal/dependency contracts, synchronous exactly-once startup and refusal paths | Anomaly process startup and device-order acceptance |
| Adapter/bootstrap | Exact-build detection, foreign-process refusal, lifecycle and cold entry built/tested | Actual Anomaly activation; current explicit `Mode=Observe` only instruments boundaries |
| Native boundary hooks | Guarded x64 detour/trampoline/restoration fixture; standard-DX11 site map | Live invocation and ABI/camera/PDA/scope captures; AVX native mapping |
| Temporal CPU state | Independent main/scope and first-person histories, lifetimes, bones/deformation snapshots, jitter/math/reset tests | Feed from real engine producers, convert native matrix conventions and upload immutable per-draw GPU state |
| Core mesh shaders | Changed static/model VS/PS, prior palette path, motion MRT and hardware fixtures retained | Engine cache/reflection interception, actual draw binding and missing-family coverage |
| Foliage shaders | Changed tree/detail variants and real hardware MRT2 fixtures retained | Real tree/detail lifetime/wind/fade capture and identity-remapped previous batch data |
| Geometry hook producer | Audited and built into the adapter; byte-guarded lifecycle/render hooks feed Rigid and HardwareSkinned snapshots to history; two contracts registered; evidence script verifies layouts against the installed exe | Live hook invocation, a draw-time call site, view/projection producers, real palettes; single-bone/software/tree snapshots |
| World/UI render scaling | CPU dimension/frame plans only | Real target allocation/viewport changes, native-size screen UI/PDA and scope lifecycle |
| SR | Existing generic RSF services inspected | Complete verified scene-colour seam, inputs, dispatch and output reinsertion |
| FG | DLL access to existing presentation bridge tested | Real source-frame ownership/pacing, depth/motion/UI transport, Present matching and retirement |
| Overlay | Runtime exports `rsf_get_overlay_api` (configure/present/notify/visible/toggle/capture/status/settings); C++/C11 contracts and a hardware D3D11 fixture with the real panel pass. Adapter recipe: `.local/anomaly/orchestration/overlay-api-worker-report.md` | Anomaly wiring, DirectInput gating, live controls/status, composition acceptance. Overlay rides in the presented real frame, not after generation |
| Game validation | Vanilla engine/device startup observed under Frida, followed by shutdown | No injected Anomaly SR, FG, overlay or gameplay acceptance yet |

## Built components and tests

### Main tree verification

Commands, from `D:/Coding/ReScaleFrame`:

```text
powershell.exe -NoProfile -File eng/verify.ps1 -VS2026 -Configuration Release
```

After wave 1 this completed successfully: **168 discovered CTest entries, 157 passed
and 11 optional/device-dependent skips**, followed by Cargo formatting and Clippy.
It includes the geometry and overlay API contracts. Evidence:
`.local/anomaly/verify-release-wave1.log`.

The Debug failure was a godot-cpp CMake cache entry forcing `MultiThreadedDLL` onto
every target on reconfigure. `integrations/godot/CMakeLists.txt` now drops it after
godot-cpp and the root CMakeLists clears stale copies. A fresh Debug tree built with
no LNK2038 and ran 163 discovered, 152 passed, 11 skipped (`build/wk-debug-*.log`);
that run predates the geometry/overlay additions.
No Rust behaviour was changed; workspace Rust unit tests were not separately run.

### Carrier and adapter

- Carrier: `loader/proxy/src/thin_*`, target `rsf_proxy_thin_dinput8`.
- Output: `build/windows-x64/thin/bin/{Debug,Release}/dinput8.dll`.
- Adapter: `games/anomaly/src/{plugin,bootstrap,native_observer,native_hooks,native_profile}.*`.
- Output: `build/windows-x64/bin/{Debug,Release}/ReScaleFrame.Game.Anomaly.dll`.
- Runtime: same bin directories, `ReScaleFrame.Runtime.dll`.
- Main registration includes `tests/thin_loader.cmake`: 46 carrier contracts.
- A focused main-tree run passed those 46 plus four Anomaly detection/lifecycle/
  bootstrap/native-hook contracts: 50 passing CTest entries.
- The cold entry is `uint32_t __stdcall rsf_anomaly_start(const wchar_t*)`.
  It borrows the carrier-loaded Runtime and uses generic plugin-session services.
- Only explicitly selected `[Anomaly] Mode=Observe` currently starts observation.
  It is not a rendering mode and does not activate SR or FG.

One malformed-DLL negative test initially opened a Windows Bad Image modal. This
was an automation defect, not an Anomaly file defect. Production loads now scope
thread-local loader error suppression and restore it before adapter entry.
Independent malformed-runtime and malformed-adapter checks passed with both
process/thread error modes initially zero: no modal, modes restored, genuine
forwarding retained and Win32 error 193 logged.
Evidence: `.local/anomaly/modal-regression/result.json` and
`.local/anomaly/carrier-worker.md`.

### Runtime DLL presentation access

- `runtime/orchestrator/include/rescaleframe/d3d11_present_api.h`.
- `runtime/orchestrator/src/d3d11_present_api.cpp`.
- `rsf_get_d3d11_present_api`, API version 1, exposes 22 existing operations.
- Real C++ and C11 DLL-import tests are registered in the main tree and passed.
  Worker Debug/Release results were also inspected.
- No additional runtime owner was introduced. ABI availability is not provider
  availability. Existing native FG still lacks a complete separate `frame.ui`
  transport; exposing UI-mode settings does not fix that gap.

### Temporal CPU primitives

`games/anomaly/temporal/` contains four C++20 headers and its feeding/convention
README. Main-tree execution passed all 17 internal cases and correctly rejected
an unknown test selection. Important convention: these CPU matrices use column
vectors; native engine bytes must be converted, not blindly copied.

### Shader tooling and genuine motion fixtures

Keep these two generations of work distinct:

1. `tools/anomaly_shader_tool.py` is inventory/planning/declaration-only staging.
   Its eight tests passed in the main tree. Its 130 byte-identical old/staged FXC
   pairs prove non-interference, NOT temporal output.
2. `tools/anomaly_temporal_shader.py` and `games/anomaly/shaders/core/` add actual
   prior-state mesh clips and motion output. Retained worker evidence reports
   458 old/new FXC pairs. `.local/anomaly/core-handoff/device-report.json` records
   hardware execution with 192 VS checks, 860 PS comparisons, 776 invalid-history
   checks and 340 stationary/jitter checks, with `runtime_ready=false`.
3. `tools/anomaly_foliage_shader.py` and `games/anomaly/shaders/foliage/` add tree
   and detail prior-deformation paths. Retained hardware output explicitly names
   Intel UHD Graphics and contains rendered FP16 MRT2 vectors, original MRT0/1
   preservation and invalid-history checks. See `.local/anomaly/foliage-worker.md`
   and `foliage-device-report.txt`. These were inspected, not rerun by the parent.

Shared motion output is RTV2 / RGBA16_FLOAT:
`(current-minus-previous normalized UV X, UV Y down, validity, 0)`, cleared zero.
VS b13 is family-specific. PS b13 is a separate 48-byte viewport/jitter/ABI block.
Core VS history is 3824 bytes; foliage VS history is 5056 bytes.
Shared IO uses `rsf_previous_clip:TEXCOORD12` and flat `rsf_valid:TEXCOORD13`.
Foliage previous projection is unjittered, so its PS previous-jitter UV must be zero.
Read both shader READMEs before binding either family.

Original licensed shader bodies remain local. Do not deploy declaration previews
or publish generated game assets. Missing previous state is invalid coverage,
not a valid zero velocity or permission to invent camera-only object motion.

## Research findings that constrain implementation

- r3 contains 449 files. The source plan identifies 55 geometry-stage roots;
  another 44 roots remain unclassified and nine shadow roots are excluded.
- `deffer_tree_s_bump_d.vs` recursively includes itself and remains blocked.
- `combine_1.ps` has genuine shader-local pre-tonemap colour, but no complete
  stock FP16 scene texture has been demonstrated. Generic intermediates are LDR8.
  Sky/clouds, forward content, volumetrics, bloom and fixed UNORM resolves prevent
  a simple allocation-format substitution from becoming a valid HDR seam.
- G-buffer Z is linear view Z, not hardware/device depth. First-person projection
  and compressed viewport depth need their own correct conventions.
- Shader replacement must cover cache hits AND compilation before creation,
  engine reflection and VS input-signature extraction. Blind CreateVertexShader
  or D3DCompile interception is insufficient.
- Motion RTV2 must survive relevant engine target binds. Unsupported occluding
  draws must invalidate their covered samples; a frame clear alone is insufficient.
- Physical PDA material animation is not object movement. Keep its content producer,
  projected geometry, occlusion and any provider history hints distinct.
- Scope frames can render without Present. Native render-frame counts do not
  establish simulation/input ownership or FG eligibility.

Sources and exact boundaries:
`docs/research/anomaly-world-space-ui.md`,
`docs/research/anomaly-native-hooks.md`,
`tools/anomaly_shader_README.md`, and both shader-family READMEs.

## Live experiments and installation state

- Some ordinary launches exited with code 1 before useful renderer diagnostics.
  Do not claim this launch-path problem is solved.
- Moving a custom fsgame file outside the installation changes native fs_root and
  archive entry-point resolution. Absolute alias edits alone did not produce a
  working isolated baseline. Preserve this failed approach rather than repeating it.
  The user reported the missing-file dialog again:
  `Cannot find file d:/coding/rescaleframe/.local/anomaly/live\gamedata\configs\system.ltx.`
  This is the private test-profile path, not evidence that the installed game's
  files require reinstalling. The baseline/profile problem remains unresolved;
  no corrective launch was attempted after the latest stop request.
- A later Frida-started vanilla process reached `_XRAY_1.5`, the engine main loop
  and D3D11 initialization on an NVIDIA GeForce RTX 4070 Laptop GPU. The log later
  records normal engine/device shutdown. This was a diagnostic vanilla baseline,
  not RSF injection or visual/gameplay acceptance.
- The Frida spawn helper appended an executable argument already supplied by the
  caller, so its observed command line contains the executable twice. Fix launch
  argument construction and reproduce a clean baseline next session.
- `rs_fullscreen` was logged as an unknown command. Do not assume that requested
  test windowing setting took effect; determine the installed build's real control.
- No RSF `dinput8.dll`, `ReScaleFrame.ini` or `ReScaleFrame/` directory is installed
  under the game `bin`. No RSF shader replacement was deployed.
- Both game EXE hashes remain unchanged; the reference source tree is Git-clean.
- Two newly created test profiles were backed up and removed from the game:
  `rsf-observe-fsgame.ltx` and `appdata/rsf-test.ltx`.
  Backups and exact cleanup records: `.local/anomaly/session-end/`.
- Native logs/cache generated by vanilla testing may remain in game appdata.
  Existing user settings/saves were not replaced. Do not delete appdata wholesale.
- At checkpoint there is no running Anomaly test process or active delegated
  worker. The Frida session was detached, REA binary closed, owned broker stopped
  and its expired local session-token file removed. Restart analysis explicitly
  next session; do not print credentials or reuse the stale session identity.

## Next-session checklist, in order

- [x] Read `AGENTS.md`, this file, both shader READMEs and worker handoffs.
- [x] Audit and integrate the interrupted geometry worker's source/tests. Removed the
      superseded standalone `tests/anomaly_geometry/CMakeLists.txt` and the unreferenced
      `tests/anomaly_geometry_HudEvidence.java` (no retained output).
- [ ] Reproduce a reliable vanilla launch with preserved native fs_root, correct
      command line, explicit test settings and backups. Capture the actual window,
      adapter/device and normal shutdown before blaming injection.
- [ ] Stage the thin carrier + exactly one Runtime + adapter with a rollback
      manifest. Verify Observe-mode hook activation/order and real camera/UI/
      PDA/scope counters. Do not deploy to an already-running game.
- [ ] Connect source-frame/view/first-person and geometry-lifetime producers to
      frozen history. Capture real final palettes and detail wind/fade/item state.
- [ ] Merge core/foliage artifacts, implement coherent cache/reflection hooks,
      reserve/bind b13 after native uploads and maintain motion MRT/validity/resolve.
- [ ] Implement real world/output/UI/PDA target and viewport separation, scope
      resets and the complete linear-colour reconstruction boundary.
- [ ] Wire SR evaluation/reinsertion through the shared Runtime; test raw guides
      and pixel output, not just success counters.
- [ ] Wire FG source/input/submission/Present ownership, one pacing owner,
      HUD-less/UI resources and GPU retirement; keep world-space PDA in its proper role.
- [ ] Integrate the SHARED OVERLAY: toggle, native-resolution rendering, truthful
      requested/effective/backend/input status, supported SR/FG/quality/latency
      controls, persistent settings, resize/reset and FG-safe composition.
      Publish required generic DLL services instead of embedding another runtime.
      Anomaly polls DirectInput: a window subclass alone may not stop game camera/
      weapon input while the panel is open. Test real capture and closed-panel pass-through.
- [x] Register and rerun geometry tests, then full Release verification; fix the Debug
      Godot CRT mismatch. Core/foliage shader device fixtures remain standalone verifiers.
- [ ] Game-test moving actors/weapons, skinning, trees/grass, stationary/panning
      scenes, PDA text/scrolling/occlusion, scopes, menu transitions, overlay input,
      resize/reset, provider changes and teardown. SR and FG require separate acceptance.

## End-of-day result

A tested loader/ABI/history foundation and device-tested motion-shader components
exist. They are not yet connected into a complete Anomaly renderer. **World/UI
render scaling, live temporal production, SR, FG and the overlay remain unfinished.**

Latest closeout: the late worker report confirms completed core/foliage shader
fixtures and an interrupted geometry worker. No live delegated workers remain.
Only this documentation was updated after the latest stop request. No further
implementation, build, game launch or deployment was performed.
