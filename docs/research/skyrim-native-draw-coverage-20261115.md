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

## Status and uncertainty

The analyzer's synthetic and skin-decoding checks were code checks. The live capture above is
game evidence for one interior third-person scene. The accurate backtracer did resolve
through the adapter's World detour, since motion records were attributed to primary output 0.
No shader repair, motion repair or broad coverage acceptance is claimed.
