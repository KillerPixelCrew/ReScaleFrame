# Skyrim native final-draw coverage probe, 15 November 2026

Question: which native draws actually write the primary motion target, with which shaders,
targets, write masks, vertices, skin weights and bone history? The offline census
([Effect/Water note](skyrim-effect-water-motion-20261115.md)) and the bound b9/b10 check
([integration note](skyrim-se-integration.md#live-skeletal-history-and-small-family-audit-15-november-2026))
cannot answer this. A repair through the shader replacement service is justified only after a
final draw proves a missing interface or buffer.

Executable: SkyrimSE 1.7.104, SHA256
`846efccf0c1374d71f892907f46549560f2fcb0a75cb87a3eed438baa0f1402f`, matched IDA database
`.local/skyrim-research/SkyrimSE-1.7.104-research.i64`. Shader archive `shaders011.fxp`, SHA256
`72cfc73eb0938d5949f0b3a8683722a33b671a34b41b750ba138de8d154c5fbc`.

## Why the earlier draw probe saw nothing

`probe-skinned-draws-v2.js` hooked only DrawIndexed. It filtered on graphics thread 43256,
which it had learned from earlier probes in the same process, PID16972. Its separate
seven-method counter also counted zero draws, and the native counter stayed at 79991. The most
likely reading is that the game was not rendering during that window. The probe had no check
that frames were advancing, so it could not tell an idle game from missing coverage. No
renderer failure or coverage conclusion follows from it.

## Static facts used by the new probe (IDA, this session)

- Draw scope. `RSF_Research_Main_RenderWorldWithOutput` (RVA `0x656d10`, renamed) calls
  World `0x656e60` at `0x656e2d`, so return `0x656e32` is on the stack. RenderEntry calls
  that wrapper at `0x656a78` with `xor edx,edx` (output 0, return `0x656a7d`). It also calls
  it at `0x656834` with `edx=14` (return `0x656839`). A draw whose backtrace contains both
  `0x656e32` and `0x656a7d` therefore belongs to the primary World output 0, the same scope
  the adapter admits. This needs no hook at the adapter's MinHook sites. Byte `0x332e470` is
  also set by UpdateTick and `0x659ad0`, so it is recorded but is not a scope proof.
- Native cached state. `RSF_Research_Renderer_FlushCachedState` (`0x1010de0`) binds RT ids
  `0x20cfc18[8]` (terminated by -1) through RTV `renderData+0xa58+0x30*id`, where pointer
  `0x3330188` holds `renderData = 0x3331f50`, the renderer at `0x3331f40` plus 0x10
  (stored by `0x0e97b0` and `0x1007b20`). This agrees with the adapter's static texture
  `0x3331f40+0xa58+0x30*id` and RTV `+0x10`. Main depth texture `0x3333f58` is
  `renderData+0x2008`, and DSV slices are `renderData+8*(slice+19*(id+54))`. It also binds depth id/slice
  `0x20cfc38/0x20cfc3c`, cube `0x20cfc40`, depth state `0x20cfc88/0x20cfc90` with ref
  `0x20cfc94`, and blend `0x3330e30[52*0x20cfca8 + 26*0x20cfcac + 2*0x20cfcb0 + 0x20d01d0]`.
  Native draw helpers (for example `0x100bcd0`) flush before they draw, so at draw time these
  ids describe the bound state. Records also query the D3D state directly so the two can be
  compared.
- Shader identity. The native VS wrapper keeps its DXBC at +0x68 (size +0x10). The PS reader
  `0x100ffe0` frees the bytecode after `CreatePixelShader`, and its wrapper keeps only the
  descriptor (+0), the COM object (+8) and constant groups. A live PS is therefore identified
  by family (from its VS hash) plus native descriptor, then looked up in the archive
  inventory. Several native VS descriptors share identical bytecode, so a VS hash alone is
  ambiguous and the join uses hash plus descriptor.

## Probe and contract

Ignored research material: `.local/skyrim-research/probe-native-draws-v3.js` (template
SHA256 `F848F2F0FEB92E9530E813CD4E388A4CEA60E40B3B3461F743BC68FFD8AACA21`),
`prepare-native-draw-capture.ps1` and `analyze-native-draws.py`.

`prepare-native-draw-capture.ps1 -TargetProcessId <pid> [-ReportDirectory <run report>]` checks
the process name and executable hash and refuses an existing output directory. It writes a
configured `native-draws/probe.js` and `prepare.json`. The coordinator then runs REA
`instrument_with_frida` with attach, that PID, `source_kind=file`, that path and
`duration_ms=12000`, while the game is foregrounded and rendering a scene.

The probe:

- refuses before hooking on a PID mismatch, a guard-byte mismatch (uploader `0x1578c80`,
  call sites `0x656e28`, `0x656a73`, `0x65682c`) or a moved render-data pointer;
- spends 1.5 s doing memory reads only, and refuses unless the native frame advances by at
  least 10 and native-return immediate-context draws come from exactly one thread;
- hooks the immediate context's DrawIndexed, Draw, DrawIndexedInstanced, DrawInstanced,
  DrawAuto, both Indirect methods, Dispatch, DispatchIndirect and ExecuteCommandList, plus
  the native bone uploader;
- issues every D3D query and staging copy inside a native draw callback, on the owner thread
  and the immediate context (GetType checked), and releases every reference before the draw
  continues;
- counts two complete frames by native RT ids, depth/cube, blend/depth indices, VS/PS
  descriptors, skinning and draw site;
- takes at most 16 records in four frames. Motion-target records must be primary World
  output 0. Each record holds the draw arguments, the actual RTV/DSV with native ids,
  formats and sizes, blend targets and write masks, depth-stencil and rasterizer state,
  viewports and scissors, IA state, VS bytecode and hash, and VS/PS b0..b2 and b12 bytes. It
  also holds the referenced index range (at most 64 KB), the referenced vertex range (at
  most 256 KB) and, for skinned draws, the CPU current/previous arrays compared with the
  bound b9/b10;
- caps readback at 8 MB, stops after 9 s, detaches and writes `manifest.json`. Draws on
  another context, another thread or from a foreign module, overlap, budget and limit
  exclusions are counted in `exclusions`.

Skinned coverage needs the player body or another actor in view, for example third person
after one press of the default POV key. In first person with nothing in the hands, the
skinned categories can be empty. The manifest then shows `uploads=0`, and Effect/other
motion-target records still answer the Effect literal (1,0) question.

## First attempt: preflight refusal

In combined run `.local/skyrim-research/fg-frida-20261115-220223`, PID43648, REA evidence
`ev_c3e1f9789de739826fb00543bb7ec5501bb29602870153ea11110c835e1ad105`, the first template
(SHA256 `577DA4AD...`) passed its guard bytes. It then refused with "render data pointer
moved" before attaching any hook or making any GPU call, and wrote no manifest. The game was
rendering in third person. The cause was this note's earlier assumption that `0x3330188`
holds `0x3331f40`. IDA shows its two writers store renderer+0x10. The same mistake would have
shifted the RTV identification scan by 0x10. Both are corrected against the flush formula and
the guard still requires the exact pointer. Preflight refusals now write a manifest with the
actual and expected values before throwing. No draw evidence came from this run.

## Live capture, PID41756

Run `.local/skyrim-research/fg-frida-20261115-220603/native-draws/`, REA evidence
`ev_ac85c355caebd6d28414611118c0b03bc839d0af18380ef370a7c77e7721b775`, template SHA256
`F848F2F0...`. The game was in QASmoke, foregrounded, third person after one POV key press,
with reduced SR active. Warmup saw native frames 670 to 756 and 46,403 native draws, all on
one thread, 36496. The probe completed with no errors, 12 records, four frames (757..760),
73 census keys, 32 bone uploads and 324,778 readback bytes. Analysis:
`analyze-native-draws.py` wrote `analysis.json` beside the manifest.

Every frame reports output 1920x1080, ratios 0.6667 and lock 0, with a changing jitter. All
12 records have viewport 1280x720, matching the reduced render rectangle, and a VB0 stride
equal to the native vertex description.

Census of frames 757 and 758: 1,080 native draws, 496 with motion target 7 in the native RT
cache.

| Native RT ids / depth | Draws | Shaders (exact inventory join) | Motion slot state from records |
| --- | --- | --- | --- |
| 1,7,4(or 5),112 / main depth, test only | 462 | Lighting VS 1, 513, 16777728, 16777729; PS 1, 513, 16777729 | slot1 R16G16_FLOAT, write mask 15, blend off; PS declares SV_Target0..2; VS exports POSITION2 |
| 1,7 / native depth 7, LESS_EQUAL, no write | 10 | Lighting VS 514 and 83886599 (skinned), 16777728; PS 513, 83887623, 16777729 | same as above |
| 4(or 5),7 / main depth | 2 | Water VS/PS 20480 | slot1 write mask 7, blend off; PS SV_Target0..1; VS exports POSITION2 |
| 0,7 / main depth (native cache only) | 22 | not native helpers; see below | not recorded |

Skinned history at the actual final draw: three skins with 40, 15 and 5 matrices were drawn
first by depth-only Utility VS 8196 into main depth. They were then drawn by Lighting VS 514
or 83886599 into colour 1 and motion 7, against native depth slot 7 (the post-prepass copy).
For all five skinned records, bound VS b9 equals the CPU previous array and b10 the CPU
current array, byte for byte. Previous differs from current: maximum element deltas are
0.114, 0.0116 and 0.028. Each palette is the identity map for its partition. The decoded
weighted bone indices stay within it (maximum 39, 14 and 4), with no non-finite weights.
Six of 468 vertices in one skin have half-precision weight sums more than 1% from one; that
is native asset data. The draws are DrawIndexed from native helper `0x100bdad`, inside the
primary World output 0 established by backtrace.

Conclusion for this scene: every observed native draw that binds the motion target uses a VS
exporting previous position and a PS declaring the motion output. The motion slot is written
without blending and with RG enabled. Skinned draws receive real previous and current bones
that cover their indexed vertices. No missing vector, buffer, flag or timer defect is shown,
so no shader replacement is justified by this capture, and none was made.

Coverage limits that remain:

- No Effect, Particle, RunGrass, DistantTree or Sky draw reached the motion target in these
  frames, and the census has no shadow or cube pass. The Effect literal (1,0) path and the
  other families are still unobserved live. A scene containing them is needed before any
  claim either way.
- 22 draws per two frames come from `0x118b595` (DrawIndexed) and `0x118b8ba` (Draw) inside
  virtual method `0x118b320` and its callee `0x118b790`. That renderer batches vertices with
  memcpy into its own buffers and calls a context pointer it stores at +0x498. It does not
  use the native state flush, so the native cache values recorded for these draws
  (RT 0,7, VS/PS 0) do not prove the actual binding. They ran outside the World wrapper
  (render flag 0), with no detailed record. RTTI for its vtable near `0x1ab7858` was not
  resolved. Whether it writes target 7, and whether that matters after the pre-HDR SR/FG
  input lease, is unverified.
- Counted but not recorded: 27 Draw calls on another context, 97 raster/dispatch calls from
  ReScaleFrame.Runtime.dll, one from d3d11.dll, and 20 native Dispatch calls at `0x15651af`.
- One interior scene and four frames; no moving-scene quality or FG/latency acceptance follows.

## Riverwood exterior capture, PID46148

Run `.local/skyrim-research/exterior-draws-20261115-221646/native-draws`, REA evidence
`ev_60a4eabafdac67926f4b5788e408a7fbdd5075995b3f2d8c487afc203469d296`, unchanged template
`F848F2F0...`. The starting cell was Riverwood (CELL form 0x9732), third person, reduced SR
active. Warmup advanced 67 frames on owner thread 30496. Phase complete, no errors, 1,083
bone uploads, 16 records.

The 16 records filled within native frame 1220, so the census is a partial frame (4,419
draws, 647 on motion target 7, 67 of them skinned). Sky, grass, water, particles and later
passes may fall after the cutoff, so their absence is not evidence. Every motion-target draw
used the native RT layout `[1,7,5,112]`, main depth test-only, native blend index 1.

- One Effect draw with the motion bit (VS 67108931, PS 67633219) writes slot 1 with write
  mask 15, and its VS exports POSITION2 from `cb2[3..5]`. No non-motion Effect PS was bound
  to target 7 in the counted draws.
- The six skinned Lighting records (VS 2, PS 24577; 4-bone partitions) again have b9 equal
  to CPU previous and b10 equal to CPU current, previous different from current, and weights
  inside the partition. VS 2 places POSITION1 in `o9` (from cb10) and POSITION2 in `o10`
  (from cb9); `o11` is COLOR0, so its literal `(1,1,1,1)` is not history.
- In every Lighting motion record in both scenes, PS `cb2[7].z` (SSRParams.z) is 0, so the
  literal `(1,0)` branch is not taken.
- Rigid records 4, 5, 6, 13, 14 and 15 have `cb2` current equal to previous to the bit. They
  are static architecture under a stationary camera, which is expected and proves nothing
  about moving props.
- Records 0 and 1 are a 512x512 auxiliary view (depth slot 6, cube 0) and do not bind
  target 7. Records 2 and 3 are 2048x2048 shadow-depth Utility draws.

## Rigid and attached prop history (carried-log report)

The user reported heavy ghosting on logs carried by an NPC. Static facts from the matched
IDA database:

- Producer `RSF_Research_NiAVObject_UpdateWorldAndHistory` (`0xeebf40`). Once per value of
  render counter `0x33363f4` (stamp at `+0x104`), or when update flag `2` is set, it copies
  world (`+0x7c`, 52 bytes) to previous (`+0xb0`). It then rebuilds world from the parent
  (`+0x30`) and local transform, or from the collision object (`+0x40`). It resets previous
  to world when `0xef5c60(previous)` is true or reset flag `+0x109 & 0x40` is set.
- Binding: Lighting `SetupGeometry` `0x1549550` (vtable `0x1b47420` slot 6) writes VS `cb2`
  current world from `+0x7c` and previous world from `+0xb0`, each camera-relative against
  the current and previous eye position (`0x20cff5c` and `0x20cff68`). If `renderFlags & 0x10`,
  previous is taken from `+0x7c`, the current world, which leaves camera-only motion. LOD
  techniques 9, 13, 15 and 18 always take current, which suits static LOD.
- `RenderPassImmediately` `0x1560340(pass, technique, alphaTest, renderFlags)` forwards
  `renderFlags` unchanged through `0x1562270` to SetupGeometry. Batch loop `0x155f800` and
  accumulator `0x1521190`/`0x151ef40` also pass it through. The setter of bit `0x10` was not
  located statically.
- Community Shaders (pinned and latest `038a0a04`) hooks SetupGeometry and
  RenderPassImmediately only to select permutations. It passes `renderFlags` through and
  never edits `previousWorld`. Its upscaler therefore consumes the same native object history.

Neither capture isolates a moving rigid prop. `probe-native-draws-v4.js` (SHA256
`23C3549A4B835ABA4882268DBAD661127141225ED10F4CD17CBDA36127C07A5F`) is v3 plus one
guarded, memory-only hook on `0x1560340`. For every pass in two frames it records a
histogram of `renderFlags` values. For each pass whose geometry has world different from
previous, or `renderFlags & 0x10`, it records the geometry, technique, flags, both
transforms, stamp, reset flags, collision and five parent names, plus both eye positions.
Records prefer rigid motion draws whose pass geometry moved, so their `cb2` can be compared
with the CPU transforms. `prepare-native-draw-capture.ps1` now defaults to v4 (`-Template`
selects v3). Scene: Riverwood exterior in third person with a visibly moving rigid
attachment, for example the player walking with an equipped weapon, or an NPC carrying a
log. Rejection: if moving rigid geometry has `cb2` previous equal to world-minus-previous-eye
while `+0xb0` differs from `+0x7c`, binding dropped history (bit `0x10` or another branch).
If `+0xb0` equals `+0x7c` while the object moves, the producer did not retain history.

## Exhaustive native shader census and Community Shaders comparison

All 16,044 archive containers are now classified offline. Lighting and the unclassified tail
were added by `audit-lighting-tail-inputs.py` (manifest `lighting-tail-inputs/manifest.json`).
Other families come from the earlier manifests.

| Family | VS / PS | Motion interface (static) | Live evidence | Open |
| --- | --- | --- | --- | --- |
| Lighting | 127 / 6,924 | All VS export POSITION2: 78 rigid from previous-world `cb2[3..5]`, 49 skinned from previous bones `cb9`. Every PS writes target1 as `cb2[7].z > 1e-5 ? (1,0) : computed` | Main MRT `[1,7,4/5,112]` and the `[1,7]` skinned pass on depth copy 7; `cb2[7].z = 0` in all records; bones exact | Moving rigid `cb2` previous (v4); SSR-auxiliary passes with `cb2[7].z > 0` not observed |
| Effect | 606 / 3,216 | 264 VS POSITION2 (bit 26); 1,543 bit-26 PS compute; 1,673 non-bit-26 PS write literal `(1,0)` unconditionally | One bit-26 draw on target 7 (Riverwood); no non-bit-26 PS on target 7 seen | Particles and spells on target 7 in a full frame |
| Water | 2,172 / 2,172 | Only 20480..20483 (technique 10, stencil) export POSITION1/2 and target1 | 20480 on `[4/5,7]`, write mask 7 (QASmoke) | Animated water and reflection ownership |
| RunGrass | 18 / 18 | All VS POSITION2 with separate current/previous wind timers | Not observed | Grass in a full frame |
| DistantTree | 4 / 4 | 2 VS POSITION2, 2 PS target1 | Seen only in depth prepass and auxiliary view (cube) | Main colour pass |
| Sky | 9 / 9 | All VS POSITION2, all PS target1 | Not observed | Exterior full frame |
| Particle | 6 / 6 | No POSITION2; target1 fixed XYZ (normal-like) | Not observed | Whether target 7 is ever bound while particles draw |
| Utility | 339 / 177 | No POSITION2, no target1 | Depth prepass and shadow depth only | None for motion |
| BloodSplatter | 2 / 2 | No POSITION2, target0 only | Not observed | Overlay order |
| Unclassified tail | 107 VS / 107 PS / 19 CS | No VS POSITION2; 4 PS write a second target from full-screen-style code; 19 compute | No image-space draw bound target 7 | Exact identity (likely image space and compute) |

Comparison with the working Community Shaders upscaler (`src/Features/Upscaling.cpp`
1440..1512 and `EncodeTexturesCS.hlsl`, latest `038a0a04`). Before DLSS or FSR it derives:

1. Reactive (`BiasCurrentColorHint` for DLSS) = `taaMask.x * 0.1 + taaMask.y` from
   `kTEMPORAL_AA_MASK`. In this build native TAA (`0x1544da0`) binds target 85 as `t4`,
   alongside motion target 7 (`t2`) and accumulation 81/82, so the vanilla mask exists and
   native TAA consumes it. Its main-frame producer was not found statically (the only direct
   bind of 85 is in secondary camera renderer `0x1409899c0`), so its vanilla contents are
   unverified. RSF supplies no reactive input for Skyrim.
2. Transparency = normals target z. Vanilla Lighting PS 513, 24577 and 25089 write
   `o2 = (normal.xy, 0, ssrMask)`, the Effect motion PS writes `o2.zw = 0`, and the Water
   motion PS does not write `o2`. Community Shaders' own Lighting writes glossiness there. On
   vanilla data this input would be constant zero, so it does not apply.
3. DLSS only: 5x5 depth-aware dilation that takes the longest nearer neighbour vector and
   fades to the pixel's own vector near the camera. This is generic conditioning of real
   vectors rather than a producer repair, and RSF's motion resolve has none.

Already equivalent in RSF (earlier entries): camera-inclusive vectors, unjittered, `mvecScale`
(1,1) after the direct-DLSS correction, jitter sign, non-inverted depth, valid zero motion
preserved.

Conclusion: no missing interface, buffer, bone history or SSR-gate misuse is proven in any
family or observed draw. No shader replacement is justified. The carried-log ghosting is
still unexplained. Candidates: (a) native rigid history dropped by bit `0x10` or by the
producer, decided by v4; (b) the absent reactive input, once target 85 contents are shown;
(c) the absent dilation, a conditioning choice for the coordinator.

## Steady-carry baseline capture (v4), PID43160

Run `.local/skyrim-research/prop-draws-20261115-223539/native-draws`, REA evidence
`ev_dd079f728d8495424222d3ca3f49e57a1002c509e9c9cbd1e798402e8d44ace2`, template v4
`23C3549A...`. Riverwood, third person, W held during the probe, four complete frames (no
early stop), no errors, 15 records. An earlier attempt, `prop-draws-20261115-223128`, failed
in preparation (a PowerShell parameter validation bug in the helper) before any hook ran. It
provides no evidence.

- The `renderFlags` histogram over 2 frames shows no bit `0x10`: 0x8:509, 0x500:374,
  0x400:3604, 0x404:16, 0x21:2538, 0x3:8, 0x0:116, 0x41:2916, 0x45:514, 0x43:8.
- Carried firewood in steady carry: three NPC carry objects (`AnimObjectCarryFirewood01/02/03`
  under the hand and MagicNode bones), plus bread, a forge hammer and metal scrap, have stamp
  = frame−1, reset flags 0 and real per-frame motion (about 1.7 units). Record 9
  (`AnimObjectCarryFirewood02`, VS 513, PS 25089) binds VS `cb2` current = world − eye and
  previous = previousWorld − previous eye to within 3e-8. It differs from world − previous
  eye by 1.65, so the binding passes the real previous transform. The dagger on an NPC
  (2.6 units), the lumber-mill water wheel (1.34) and the metal scrap (0.05) agree the same
  way. All use `renderFlags` 0x41 and PS `cb2[7].z` = 0.
- The two-frame census of target 7 resolves to Lighting (conditional SSR gate), Water 20480
  (computed), Effect bit-26 pairs (computed) and two non-motion Effect pairs (`vs65603/ps1638467`,
  5 draws per frame; `vs1091/ps1091`, 1 draw per frame, Effect or Water by descriptor). Effect
  PS without bit 26 write literal (1,0) XY to their own pixels. That is the native design of
  those permutations, not proof of a defect. Pairs such as `vs0/ps0`, `vs2/ps2`, `vs4/ps4`
  and `vs8/ps8` match several families by descriptor alone, so their family needs a VS hash.

The user then narrowed the symptom: ghosting appears only during the log pickup animation,
and steady carry is clean. This capture contains no pickup. It establishes the clean
steady-carry path and does not reproduce or exclude the reported bug.

Native facts relevant to a pickup transition: `0xef5c60(previous)` only detects an identity
previous transform, so the producer resets history for never-initialised objects, which gives
zero object motion on their first frame. It has no gap check: `stamp < frame` treats an object
last updated many frames ago the same as one updated last frame. An attach, reparent, clone
or unhide that keeps an old `+0xb0` would produce frames of wrong object motion. This is a
hypothesis until a pickup frame is captured. Community Shaders (latest) has no object-history
or AnimObject transition handling, so it cannot serve as a behavioural reference for this branch.

Prepared capture: `probe-native-draws-v5.js` (SHA256
`902EB1A73CF6BF62A290D0C8B74967F0E368B2BBAF17FA64F6B1CAC52A8B864B`) is v4 with a 50 s watch.
Across the whole watch, every pass checks its geometry's world-to-previous jump and whether
the geometry was absent in the previous frame. It records transitions (an AnimObject drawn
first or after absence, or any jump above 25 units) with transforms, stamp, reset flags,
names, gap and eyes. It takes up to 10 `transition-motion` draw records with `cb2` for the
CPU/GPU comparison. Baseline records stay in the first four frames.
`prepare-native-draw-capture.ps1 -Template probe-native-draws-v5.js` writes REA
`duration_ms` 55000. Scene: Riverwood lumber mill or woodpile, third person, camera still
and looking at an NPC who will pick up firewood within about 50 s.

## Pickup watch (v5), PID49684

Run `.local/skyrim-research/pickup-watch-20261115-224232/native-draws`, REA evidence
`ev_e4cb39502d7718b24769e3c86399297a9cf6d6ac7b9bdb0c9cbf27f04cd5ebb3`, template v5
`902EB1A7...`. Native frames 1145..1631, 11 records, no errors. The user said "that's the
broken one" during this run. No game action followed; the game had already exited normally
with verified restoration.

What the artifacts retain:

- No world geometry passed through `RenderPassImmediately` in frames 1146..1461. Warmup
  frames 1060..1144 averaged only about 13 draws per frame, so the world was not being
  rendered (a menu or other non-world state).
- At frame 1462, the first world frame, the three NPC carry objects
  (`AnimObjectCarryFirewood01/02/03`) were drawn with history stamp `+0x104` = 859 and
  world−previous of 6.9..8.2 units. Steady carry in the v4 baseline has stamp = frame−1 and
  about 1.7 units. So the history producer had not run on them for about 600 render-counter
  frames, and this draw used motion from the last update before the gap. One also had reset
  flags `0x94` (bit `0x40` clear).
- The 512-entry transition list filled within frames 1462..1479 (5,538 dropped). The filler
  was 394 landscape LOD `Land` chunks with uninitialised previous transforms (zero
  translation, stamp 156; harmless because LOD techniques bind current), camera-following sky
  and cloud layers, and a bird of prey. This was a v5 filter flaw. Any pickup after frame 1479
  was not retained. The 10 transition draw records are sky and LOD water, not AnimObjects,
  because the transition was evaluated only at a geometry's first pass in the frame.

This run does not prove which frames the user called broken, and it contains no attributable
pickup frame. It does show one concrete native mechanism. The binding (Lighting SetupGeometry
`0x1549550`) uses `+0xb0` whenever `renderFlags & 0x10` is clear, even when stamp lags the
render counter by more than one. In that state the producer has not run since the stamp, so
the object has not moved, and the correct object motion is zero. The native binding instead
repeats the last update's motion. At frame 1462 that motion was 7..8 units.

RSF's render inputs mark a history reset when the native frame is not consecutive. Whether
the frame-1462 draw fell on such a reset, and whether stale lag persists for more frames
(paused world still rendered, animation update throttling, attach during pickup), is the
missing fact.

Corrected `probe-native-draws-v5.js` (SHA256
`C027F6A390F0AC5EB33175C5B7D0FD5147D71509EADB9FE45523B85BAB31A6BC`, node --check passed):
- Triggers only on stale history: lag `frame − stamp > 1` with world ≠ previous, excluding an
  uninitialised (zero-translation) previous.
- Flags the geometry for the whole frame, so its main-pass draw gets a `cb2` record.
- Records a compact per-frame series for every AnimObject geometry
  (`[frame, id, lag, delta, reset flags, renderFlags]`, up to 30,000 entries) with names, so
  a pickup phase cannot be crowded out.
- Prepare with `-Template probe-native-draws-v5.js` (REA 55 s). Capture only through a
  coordinated, user-run handoff.

Candidate repair, not implemented: in the Skyrim adapter, when Lighting SetupGeometry binds a
geometry whose stamp lags the render counter by more than one, select the native
current-as-previous path (the engine's own `0x10` branch) for that call. It is justified only
if the corrected capture shows a lagged draw outside an RSF history reset at the reported
pickup. Community Shaders has no equivalent handling.

## User F10 frame during a pickup pose (PID43160)

The user captured
`ReScaleFrame/captures/website/scene-20261115-223624-382-40500062-43160-1` in the same process
as the v4 baseline, later than its draw records: source and frame 1624, native frame 1836,
DLSS Quality 1280x720 to 1920x1080, reset 0, non-inverted depth. `clip_to_previous` is
identity within 3e-8, so the camera is stationary and any motion is object motion. The
originals were only read. Outputs (`analyze-f10-motion.py`, the
`f10-scene-20261115-223624-analysis/` JSON and crops) are ignored research material.

What this frame settles:

- The NPC at the log pile (output x280..310, y520..580) is crouched in a pickup pose, with a
  log in the left hand and a pole or axe to the right. Native motion covers the silhouette,
  the held log and the pole.
- There is no stale or invalid motion in the frame. Every active-rectangle pixel is finite:
  median 0, p99 0.12 px, maximum 0.67 px, none above 1 px. A stale 7..8-unit object jump at
  this depth would be about 3 px. The NPC clusters are ±0.3 px; the clouds, a mill part and
  a water-wheel part are 0.3..0.6 px.
- The motion SR received is consistent. The active 1280x720 native motion equals the SR game
  motion bit for bit, and the backend motion equals UV × (1280, 720) to 4e-4 px (the
  previous-minus-current pixel convention). The full-size target holds stale values outside
  the active rectangle (up to 0.066 UV), which SR does not read.
- DLSS output against the nearest-upscaled source on linear buffers, NPC motion mask
  (1,308 output pixels): mean luminance 0.414 against 0.411, while the brighter background
  is 0.44. Relative difference has a median of 2.5% and p90 of 11%, against 1.6% and 6.5% for
  static pixels in the same box. No ghost blend of the NPC with the background is visible in
  this frame. Dilation in the Community Shaders style would not change sub-pixel vectors like
  these.

Limits: this is one frame at one moment of the pose. It cannot show the attach or start
frames, a multi-frame trail, or whether this is the moment the user called broken. It does
not reproduce the reported ghosting, so it justifies no repair. It agrees with the v4 result
that steady attached objects receive correct history. The stale-history mechanism from the
pickup watch still needs a lagged draw outside an RSF reset before any change.

## Status and uncertainty

The analyzer's synthetic and skin-decoding checks were code checks. The live capture above is
game evidence for one interior third-person scene. The accurate backtracer did resolve
through the adapter's World detour, since motion records were attributed to primary output 0.
No shader repair, motion repair or broad coverage acceptance is claimed.
