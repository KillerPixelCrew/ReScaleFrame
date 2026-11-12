# Anomaly: world-space UI with super resolution and frame generation

Status: primary-source documentation review and local source inspection only. No shader replacement, hook, game run, SR output, or FG output has been validated. Delivery must use a DLL shim, never replace the game executable. `D:/xray-monolith` is a source reference, not a rebuilt-engine delivery path.

## Question and method

How should the physical PDA screen be handled when the world renders below output resolution and presentation includes generated frames?

Read AMD upscaling/frame-generation documentation, NVIDIA's DLSS programming guide and world-attached UI discussion, current Streamline guidance, and the current Intel XeSS-FG guide. Separate these vendor recommendations from the proposed Anomaly implementation. These online guides do not establish which features the locally installed RSF SDK versions support.

## Findings from vendor sources

- Moving geometry needs matching depth and dense motion vectors that account for camera and independent object motion. This is a necessary temporal input contract, not a special exception for text.[5]
- Animated material content is a different problem from geometric motion. AMD explicitly identifies in-world video screens as potential ghosting cases and recommends material-local reactive treatment in its UE integration.[2]
- For the documented FSR 3.1 upscaler, the transparency/composition mask affects history protection, while the reactive mask changes history accumulation. These are related but not interchangeable controls. Animated textures are an explicit T&C use case.[1]
- Do not blindly translate those masks into DLSS. NVIDIA's DLSS programming guide dated 31 March 2026, section 3.15, warns that BiasCurrentColor should not be used with the latest models. The legacy description later in that section is not permission to ignore that warning.[7]
- AMD prefers rendering screen-space UI through a presentation callback. A separate UI texture is another option; HUD-less/backbuffer extraction is a compatibility path. Presentation callbacks can run independently of the next engine frame, so arbitrary engine UI calls are not automatically safe inside one.[3]
- Current Streamline guidance requests HUD-less and UI inputs; UI alpha is preferred where available. Its optional UI recomposition interpolates HUD and scene separately before composition. This is not equivalent to repeating the latest UI texture over every generated frame.[5]
- Intel's current GitHub XeSS-FG guide recommends trying UI interpolation first, with composition alternatives when quality is inadequate. Composition can make world-attached markers update at render rate. This differs from older Intel website guidance that discouraged the no-composition mode; do not assume one universal UI policy across providers.[6]
- NVIDIA explicitly discusses UI attached to world objects, including weapon displays, and explains why redrawing it correctly at presentation rate is not a trivial overlay operation.[4]

## Verified local source seam

Reference source revision: `D:/xray-monolith`, commit `028ccf5`, described by its commit message as the Anomaly 1.5.3 engine. Source correspondence to the shipping machine code has not been verified.

- `src/xrGame/Level.cpp:714-760`: with `AF_3D_PDA` active, draws the PDA window and cursor, then calls `RenderToTarget(rtPDA)` before the inherited level render at line 770.
- `src/Layers/xrRenderPC_R4/r4_R_render.cpp:567-587`: `RenderToTarget(rtPDA)` copies swapchain buffer 0 into `Target->rt_ui_pda`. It does not itself draw UI directly to a standalone offscreen target.
- `src/Layers/xrRenderPC_R4/r4_rendertarget.cpp:399-444`: the world buffers and PDA texture use the same device dimensions. The PDA texture is named `$user$ui` by `r2_types.h:70`.
- `src/xrGame/Level.cpp:779` and `HUDManager.cpp:265-295`: normal gameplay UI is drawn later through `HUD().RenderUI()`.
- `src/Layers/xrRenderPC_R4/r4_rendertarget_phase_combine.cpp:303,354`: menu/postprocess UI also has earlier entry points. The normal HUD hook is not a complete census.

The exact material consumer, its depth/blend state, first-person projection, and live resource lifetimes still need to be verified. A source label containing "UI" is not proof that its final pixels belong in the screen-space HUD channel.

## Additional local integration constraints

Source review completed on 11 November 2026. RSF revision: `a6cfa3e63a467b32a7d6268c22a888ea307028e0`. X-Ray revision: `028ccf5f5189b393396480076b11e40852ba3ecd`. The following are source facts, not verified binary hook sites or game acceptance:

- The physical PDA producer has a once-per-engine-frame guard in `src/xrGame/ui/UIPdaWnd.cpp:365-370`. The cursor also suppresses repeated draws and performs an update inside rendering, in `src/xrGame/UICursor.cpp:57-87`. Merely calling them again from an FG presentation callback is not a valid redraw implementation.
- Item-attached UI can use lit 3D coordinates and an item/bone transform: `src/xrGame/HudItem.cpp:597-627`. This is separate from the PDA texture producer and from ordinary screen-space HUD. Do not merge these categories based on function names.
- Scope/secondary-view engine frames do not necessarily Present: `src/Layers/xrRender/dxRenderDeviceRender.cpp:383-385` suppresses Present for secondary viewport and camera-ready states. Maintain main-view and scope histories separately; a previous engine frame is not necessarily the previous presented main view.
- A ready-made linear HDR scene-color seam has not been demonstrated. `r4_rendertarget.cpp:431-444` allocates generic scene and PDA targets as 8-bit LDR surfaces. `blender_combine.cpp:18-45` already binds a tonemap texture in the first scene combine. Final-combine naming and a tonemap parameter upload do not establish that all preceding color is linear HDR. Inspect packed vanilla shaders before choosing an SR seam. Converting an already quantized image to FP16 cannot restore lost precision.
- The exported RSF `sr_bridge.h` contract already supports D3D11 inputs and ordered same-adapter processing/copyback. Its current implementation, `runtime/orchestrator/src/sr_bridge.cpp:215-243`, requires FP16 RGBA color/output, R32 float depth and RG16 float dense motion, with optional masks validated separately. Stock X-Ray targets cannot be submitted unchanged merely because the API is D3D11.
- Current `runtime/orchestrator/src/native_fg.cpp:842-855` populates backbuffer, depth, motion and HUD-less inputs, not the separate FG UI resource. Vendor SDK capabilities documented above are not proof that RSF's existing D3D11 path already transports those UI inputs.
- `loader/proxy/CMakeLists.txt:2-34` currently links the combined carrier with the orchestrator core and AC7 components. A new Anomaly DLL path must not reuse that target unchanged or create a second runtime owner. Reuse forwarding mechanisms through a thin carrier, with Anomaly hooks/policy in its adapter and only reusable services in the runtime.

Read-only PE inspection identified these installed x64 builds. Fingerprints authorize identification only, not hooks:

| Executable | SHA-256 |
| --- | --- |
| `AnomalyDX11.exe` | `c857b6f49d32c62245611b26106862a773185533856e8b858a7d7e849ed2fb4e` |
| `AnomalyDX11AVX.exe` | `2032acfc041e215508692ef4079ef504c7bbabf1793d5668084099f734b92f12` |

Both import `DINPUT8.dll!DirectInput8Create`, making a DirectInput carrier an evidenced entry candidate. Startup ordering before graphics creation and source-to-binary hook matches remain unverified. The TAA archive was inspected without installation; it is a reference for temporal inputs, not an executable replacement or a source of shader code to redistribute.

Suggested first implementation gate: DLL-only spatial world scaling with temporal SR and FG disabled, native-size UI preserved, paired scene/final captures, correct PDA/scope handling and safe reset/fallback behavior. This isolates the resolution/composition surgery before adding new temporal producers. This milestone has not been implemented or accepted.

## Recommended starting architecture for Anomaly

The following is an engineering recommendation derived from the sources above, not a vendor guarantee or an already working path.

1. Keep the physical PDA and its projected screen in the scene initially. Provide the item's actual temporal transforms, including its first-person view/projection and skinning where applicable. Do not substitute the main world camera if the item uses a distinct projection.
2. Render the PDA's content into an independently sized, unjittered UI texture. Apply normal scene jitter when projecting that texture on the physical screen. Track the content texture's resolution separately from scene render and presentation dimensions. A larger source texture alone does not restore detail lost when the screen is rasterized into a low-resolution world image.
3. Distinguish surface movement from content changes. Scrolling text and a moving cursor can change while the PDA surface is stationary. Geometric vectors should remain truthful; use supported, localized temporal hints where available rather than fabricating velocity or resetting all scene history on every UI update.
4. Treat normal 2D HUD/crosshair/menu composition separately. Supply output-aligned UI coverage and scene data to RSF; keep the actual interpolation/composition policy provider-specific. The PDA's UV-space source texture is not itself an output-space FG UI texture.
5. Keep the physical display temporally attached to the item on generated frames. Repeating its latest projected pixels over an independently interpolated hand/PDA could cause slipping, double edges, or incorrect occlusion. An output-space split would need correct coverage, depth ordering and provider validation, not a rectangular screen-space cutout.
6. If text quality remains inadequate, test a selective output-resolution screen-surface pass after SR but before FG. This is an optional experiment, not a general best practice proven by these sources. It needs correct geometry, occlusion by the hand/bezel, consistent depth/motion, and a deliberate treatment of lighting, glow and post-processing. Late rendering alone does not solve FG.
7. Do not assume a presentation callback may tick game UI or the simulation again. A true per-present redraw of world-attached UI would require a compatible intermediate pose/view and safe render-only engine access. Test a conservative provider path before designing that larger feature.

The stock PDA backbuffer-copy producer is a particular hazard for dual-resolution rendering and presentation interception. A shim must preserve that copy's descriptors and ordering or redirect the producer to a compatible dedicated target; globally shrinking device dimensions or replacing all backbuffer accesses is not an adequate design.

## Bounded acceptance tests

Compare SR off/on and FG off/on independently. Test a stationary readable page; PDA sway against a stationary background; camera rotation; scrolling text and cursor movement; fingers/bezel occlusion; opening/closing the device; and switching between physical and flat PDA modes. Capture source color, depth, motion, PDA material coverage, content texture and final UI/scene inputs with the same frame/view identity.

Reject changes that sharpen text at the cost of detached screen motion, holes at occluders, double UI, lost bloom/lighting, or incorrect main/secondary view history. Passing numeric fixtures or a DLL load is not PDA image-quality acceptance.

## Sources

[1] https://gpuopen.com/manuals/fidelityfx_sdk/techniques/super-resolution-upscaler — amd-upscaler
[2] https://gpuopen.com/learn/ue-fsr — amd-inworld
[3] https://gpuopen.com/manuals/fsr_sdk/techniques/frame-interpolation-api — amd-fg
[4] https://research.nvidia.com/labs/adlr/DLSS4 — nv-world-ui
[5] https://raw.githubusercontent.com/NVIDIA-RTX/Streamline/main/docs/ProgrammingGuideDLSS_G.md — nv-fg
[6] https://raw.githubusercontent.com/intel/xess/main/doc/xess_fg_developer_guide_english.md — intel-fg
[7] https://raw.githubusercontent.com/NVIDIA/DLSS/main/doc/DLSS_Programming_Guide_Release.pdf — NVIDIA DLSS Programming Guide
