# STALKER Anomaly RSF integration: progress tracker

Checkpoint: end of 12 November 2026, CET. Work started 11 November 2026.
Status: STOPPED for today at the user's request. **Full in-game integration is NOT complete.**
SR stages 1 and 2, motion vectors and the overlay are game-accepted. Jitter, foliage motion and
temporal SR are built and fixture-tested. Their first game run stopped early: the one Stage 3
jitter burst missed its accuracy bar; on 13 November that miss was traced to the estimator
(ce3484c). Frame generation (`FG=1`) is built through a generic `rsf_get_d3d11_fg_api`; device fixtures on
the RTX 4070 present generated frames for DLSS-G, FSR3 and XeSS, and the Release gate passes
(200 discovered, 188 passed, 12 skips). It is not yet game-accepted (`docs/research/anomaly-fg.md`).
Worker specs, reports and commit messages: `.local/anomaly/orchestration/`.

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

All game runs used the standard DX11 exe on an RTX 4070 Laptop GPU, l01_escape from the
existing autosave, 1280x720 windowed unless stated. Evidence directories are under `.local/anomaly/`.

| Area | Game-accepted | Built and fixture-tested only | Still required |
| --- | --- | --- | --- |
| Carrier and adapter | Forwarding and exactly-once startup before device creation; `Mode=Observe` hooks install with 0 refusals (`wave2-observe`) | Exact-build detection, foreign-process refusal, lifecycle | AVX exe mapping |
| Geometry identification | Static world correctly ignored, shadow ~93% of entries, no unidentified dynamic draws, captured equals main identified with 0 errors (`wave3-game`) | Tree/grass identification (`docs/research/anomaly-foliage-motion.md`) | Live tree/grass coverage; LOD, particles; 22-32% of main draws outside visuals still to decide |
| Overlay (`Overlay=1`) | Panel at native size (1280x720, 1600x900) seen via adapter captures, FG Off only, click applied and persisted, relayout on vid_restart, DirectInput withheld while open and normal when closed (`wave3-game`, `wave4-game`) | Real SR backend/quality choices and sr_report for Stage 5 | Live SR controls; FG controls once FG exists |
| SR Stage 1 (FP16 twins, tonemap round trip) | Matches stock within run-to-run noise over ~7900 frames with 0 faults (`wave4-game`). Glass-pane difference is stock exterior brightness drift (`glass-diff/FINDINGS.md`) | | Scope frames, bright outdoor sky, additive particles |
| SR Stage 2 (`SRStage=2`, `SRScale`) | 1.0 matches stock; 0.67 and 0.5 registered with zero shift and only softer; SSAO variants, vid_restart and resolution change clean (`wave5-game`) | | PDA, scope, item-attached UI, fluid; no GPU gain measurable on this CPU-bound laptop (~40 ms frames) |
| Motion (`Motion=1`) | Core mesh vectors plausible for pan, walk and weapon; image unchanged vs stock; target follows render size (`wave5-game`) | Foliage (`MotionFoliage=1`): wind motion equals CPU model in the device fixture | Moving NPC; live foliage; terrain/LOD validity 0; first-window rebind 179 vs 180 |
| SR Stage 3 (jitter, mip bias) | | Halton jitter of world and six first-person projections once each; static scene gives 0.003 px worst motion in the fixture | Live jitter measurement, weapon/world coherence, sharpness |
| SR Stage 5 (temporal SR) | | `rsf_get_d3d11_sr_api`; DLSS, FSR3, FSR4, XeSS evaluate 24/24 frames through the Anomaly path; shared-host D3D11-to-D3D12 and RDR2 host fixtures pass | Every live acceptance item in `docs/research/anomaly-sr-seam.md` section 12; Stage 6 hints (opaque colour, reactive/transparency masks) |
| FG | | DLL access to the presentation bridge tested | Everything: source-frame ownership, pacing, depth/motion/UI transport, Present matching, retirement, overlay FG controls |
| PDA and scope | Not exercised: P/Tab/M/J did not open the PDA, the save has no scoped weapon | | Find the real PDA binding; a save with a scope |

## Build and verification

`powershell.exe -NoProfile -File eng/verify.ps1 -VS2026 -Configuration Release` passed at
e991e89: **192 discovered, 181 passed, 11 optional/device-dependent skips**, Cargo fmt and
Clippy (`.local/anomaly/verify-release-wave6.log`). The Debug Godot CRT mismatch is fixed at its
CMake cache source; the last full Debug run (163/152/11) predates today's later additions.

Evidence scripts, all run against the installed exe and passing at e991e89:
`tests/anomaly_geometry_evidence.py`, `tests/anomaly_sr_evidence.py`,
`tests/anomaly_motion_evidence.py` (`py -3 -I <script> --verify`).

The opt-in shared-host D3D11-to-D3D12 Streamline fixture is NOT part of ctest. Run it before any
deployment that changes the Runtime (see `docs/releases/v0.1.1-rdr2.md`; set `RSF_OVERLAY_DLL`).

Today's commits on main (not pushed): 6f7393d, d75a881, ae714a2, 3ac6cb0, e991e89.

## INI keys (`bin/ReScaleFrame.ini`, section `[Anomaly]`)

`Mode=Observe` (required by everything below), `Overlay=1`, `OverlayKeyPoll`, overlay capture
keys (`games/anomaly/src/overlay_service.cpp`), `SRStage=0..5`, `SRScale`, `SRJitter`,
`SRMipBias`, `SRStageCapture` (+`Start/Count/Interval`), `SR`, `SRBackend`, `SRQuality`,
`DLSSPreset`, `SRExposure`, `SRFirstPersonZero`, `SRCutDistance/SRCutAngle`,
`SRStreamlineDir/SRFidelityFXDir/SRXeSSDir`, `SRRequireSignature`, `Motion=1`, `MotionFoliage`,
`MotionShaderDir`, `MotionInvalidate`, `MotionCapture` (+`Start/Count/Interval`).

## Key implementation facts

- SR seam S-B: prologue of the first of `phase_sunshafts`/`phase_blur` in `phase_combine`.
  World targets, `Device.dwWidth/dwHeight`, an R24G8 depth twin for `HW.pBaseZB`, a render-size
  stand-in for `HW.pBaseRT` (phase_occq binds it with the depth twin) and the SSAO static
  viewport are swapped from `phase_scene_prepare` to the seam. Sites: `sr_profile.h`.
- rmNear is shared by geometry, Stage 3 jitter and motion through one callback slot.
- Motion replaces allowlisted shaders only after recompiling the original byte-exactly with the
  engine's inputs; the engine imports `d3dcompiler_43`. Generated shaders contain original game
  bodies and stay local (`tools/anomaly_motion_deploy.py` to `.local/anomaly/motion-deploy`,
  deployed to `bin/ReScaleFrame/anomaly-motion`).
- Foliage motion poisons unused grass rows in engine memory (off with `MotionFoliage=0`).
- Motion RTV2 is RGBA16F `(current-minus-previous UV x, UV y down, validity, 0)`; the shared
  decode now has a validity channel, and the pipeline applies the sentinel to invalid samples.
- Overlay draws into the presented real frame after SR, not after generation.
- Desktop capture is black on this hybrid-GPU laptop, and F12 is blocked while the panel is
  open; use the adapter's backbuffer captures.

## Research findings that constrain implementation

- r3 contains 449 files; `deffer_tree_s_bump_d.vs` recursively includes itself and stays blocked.
- G-buffer Z is linear view Z, not device depth. First-person projection has its own range.
- Shader replacement must cover cache hits AND compilation before creation, engine reflection
  and VS input-signature extraction.
- Physical PDA material animation is not object movement.
- Scope frames can render without Present. Native render-frame counts do not establish
  simulation/input ownership or FG eligibility.
- The stock engine crashes with `r2_ssao_mode` default plus `r2_ssao_blur`, even without RSF.

Sources: `docs/research/anomaly-{world-space-ui,native-hooks,geometry-coverage,overlay-wiring,sr-seam,foliage-motion}.md`,
`tools/anomaly_shader_README.md`, both shader-family READMEs.

## Installation state

- Launch path that works: `.local/anomaly/wave*-game/tools/` (Process.Start with cwd at the game
  root, `-ltx` test profile in appdata removed afterwards, `-start ... autosave`, F12 engine
  screenshots, console quit by scancodes). Loading never finishes unless the game window is
  clicked (user, 13 Nov); then dismiss the loading screen with a key tap before captures. Saves are backed up before every run and have stayed byte-identical.
- 13 Nov: the user found the game starting on the Intel iGPU and set Anomaly to NVIDIA in the
  NVIDIA Control Panel. Engine logs of every earlier run name the RTX 4070, but presentation
  crossed the iGPU display path ("2-GPU path"). Re-measure frame time; treat the earlier
  "CPU-bound, ~40 ms" conclusion as unverified.
- Both game exe hashes are unchanged.
- RSF files are staged in `D:/Anomaly 1.5.3/bin`: `dinput8.dll`, `ReScaleFrame.ini`,
  `ReScaleFrame/` (Runtime, adapter, overlay, and from the last run the motion shader folder
  and vendor runtimes). Each run's rollback is `<run dir>/tools/deploy.ps1 -Rollback`.
- The e991e89 game run (`.local/anomaly/wave6-game/result.json`) stopped early at the user's
  wrap-up. It deployed the build, the vendor runtimes (streamline, fidelityfx, xess) and the
  current motion folder, and ran one Stage 3 jitter burst (16 frames, SRScale 0.67). Counters
  were clean (jittered == restores == mip applied, 4 first-person jitters per frame, no
  refusals), but `--measure-jitter` FAILED the 0.1 px bar: worst 0.163 px whole frame, 0.124 px
  on a static wall, 0.15-0.27 px elsewhere. The burst started before the loading screen was
  dismissed, so the last frames show the loading screen and HUD/shadow stillness could not be
  checked. The game then exited by itself with a clean shutdown; the cause is unknown.
  Sharpness, Stage 3 motion, foliage and all Stage 5 backends were not exercised.
- Final state: game not running, saves byte-identical, INI `Mode=Observe`, `Overlay=1`,
  `SRStage=0`, `Motion=0`. Rollback: `.local/anomaly/wave6-game/tools/deploy.ps1 -Rollback`.

## Next session, in order

- [ ] Investigate the Stage 3 jitter measurement miss (0.12-0.27 px vs the 0.1 px bar): rerun
      the burst after dismissing the loading screen, check the measurement method against a
      synthetic burst, then check units and the projection site. Find why the game exited by
      itself after that run.
- [ ] Finish the unreached e991e89 items: weapon/world coherence, HUD/shadow stillness,
      sharpness; live foliage coverage and wind;
      temporal SR per backend (convergence, panning, weapon ghosting, foliage, reset on load,
      exposure A/B, live switching, teardown).
- [ ] Fix whatever that run found, then rerun only the failed items.
- [ ] Find the PDA key binding (no user.ltx exists, so defaults come from the archives; run the
      console `bind_list` and read the log). Per the user, Anomaly starts with the PDA NOT
      equipped: open the inventory and equip it into the PDA slot first, then use the key;
      find a save with a scoped weapon; exercise PDA text and scope
      alternation with SR on (no history contamination).
- [ ] Stage 6 hints: opaque-only colour from `generic_temp`, reactive/transparency masks.
- [ ] Frame generation: source-frame ownership and pacing (scope frames render without Present),
      HUD-less/UI transport, Present matching, GPU retirement, overlay FG controls.
- [ ] Remaining coverage: LOD sprites, particles, terrain validity; the 179 vs 180 rebind count.
- [ ] Measure performance on a GPU-bound setting or machine; this laptop is CPU-bound.
- [ ] Debug full-gate rerun; AVX exe mapping.

## End-of-day result

Today took the integration from a tested foundation to working in-game pieces: the overlay,
SR Stages 1-2 and core motion vectors are accepted in Anomaly, and temporal SR evaluates all four
backends through the real Anomaly input path in fixtures. Temporal SR and frame generation are
not yet accepted in the game, and rendering-readiness flags stay 0.
