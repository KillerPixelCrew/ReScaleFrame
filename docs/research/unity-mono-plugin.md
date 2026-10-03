# Shared Unity Mono plugin

Direction agreed on 3 October 2026: use a shared Unity Mono plugin, reflection for discovery,
and Harmony for managed method patches. Drag'n Wash is the first validation case. The native
game-plugin scaffold and contract fixture now exist; managed bootstrap, Harmony and rendering
remain unimplemented and untested in a game.

## Question and evidence

Can ReScaleFrame integrate at Unity's engine-owned rendering boundaries without duplicating a
native renderer plugin for each Mono game? The shipped Drag'n Wash assemblies expose camera
setup, RenderGraph resource owners, temporal reconstruction and final UI drawing as managed
methods. Its Mono DLL exports assembly loading, reflection access and invocation APIs. Those
facts support a shared managed adapter rather than executable-specific render offsets.

[Drag'n Wash research](drag-n-wash-renderer.md) records the exact build, complete assembly
decompiles and limitations. Its Unity player and executable PDBs match their PE CodeView GUIDs
and ages, but no native symbol import or hook experiment was performed.

## Ownership and loading

The plugin lives under `games/unity-mono/`. Its native scaffold implements the existing
game-plugin C ABI. The completed plugin will own Unity discovery, managed bootstrap, Harmony
patches, pipeline adapters, frame/view data and output reinsertion. It will load one first-party
managed helper into the game's existing Mono scripting domain. It never initializes a second
Mono/CLR runtime or another orchestrator.
Vendor SDKs, graphics interoperability, settings and presentation remain in `runtime/`.

`ReScaleFrame.Game.UnityMono.dll` currently recognizes only the exact researched Drag'n Wash
x64 name and SHA256. It reports rendering readiness false, refuses `prepare` and `start` with
`RSF_ERROR_NOT_READY`, retains no host services and allows repeated cleanup. This exact allowlist
is deliberate: executable metadata alone cannot prove another Unity game's adapter compatibility.
The synthetic contract fixture validates C ABI guards and lifecycle refusal. It does not load
Mono, install Harmony patches or touch a graphics device.

The Release scaffold was built with MSVC through the VS 2026 preset. The full repository gate
passed with six expected environment/vendor skips, then Rust formatting and Clippy. The Unity
contract fixture passed. These checks establish the DLL contract only.

Native bootstrap must wait for the player, Mono and scripting domain to be ready, outside
`DllMain`. Resolve exports from the already loaded runtime and attach only threads that need
Mono API access. Assembly loading does not establish a safe thread for Unity object access;
handoff to a validated Unity main-thread boundary before discovery or patch activation.
The scripting domain used by the player must be established live. A root-domain export is a
lead, not proof that loading the helper into that domain reaches the game's assemblies.

Managed/native communication remains a bounded, size/version-checked C ABI. Do not pass
`UnityEngine.Object`, managed strings, GC handles with ambiguous ownership, exceptions or
RenderGraph value types across the public plugin ABI. Keep an internal opaque view identifier
and copy matrices, dimensions, jitter, reset state and graphics handles into owned packets.

The current `rsf_game_probe` carries executable basename/hash/architecture only. That cannot
prove generic Unity Mono compatibility. In-process recognition must verify the loaded player,
Mono runtime and required exports. Preparation then checks the scripting domain and selected
pipeline capabilities. A launcher-side file inventory can select a candidate plugin but cannot
declare readiness. Existing explicit plugin-path loading can be reused; automatic selection and
startup retry timing still need implementation. No SDK change was made for this research.

## Adapter selection and Harmony guards

Choose an adapter from the actual loaded assemblies and active pipeline instance. Separate
URP RenderGraph, older URP, HDRP and built-in renderer adapters. The first adapter targets the
shipped Unity 6000.3 URP method/resource contract. IL2CPP needs another bootstrap and hook path.
Do not advertise support for all Unity Mono games from this one static inspection.

Use reflection to resolve exact declaring types, overload signatures, fields and capabilities.
Record engine version, assembly identity/hash/MVID, selected adapter, graphics API and methods.
Cache resolved members and delegates during preparation. Avoid per-frame broad reflection scans
or repeated `MethodInfo.Invoke` at the rendering boundary where a typed delegate is available.
Pipeline recreation or a changed assembly invalidates the adapter and its cached state.

Harmony prefixes/postfixes suit camera policy and pass setup. A transpiler is appropriate where
the reconstruction call sits inside a larger post-processing recorder. Match resolved methods
and field accesses, branch destinations and stack shape, not fixed instruction offsets. Require
the expected number of semantic matches and preserve labels/exception blocks. A missing or
ambiguous match refuses activation and logs the reason. Do not force an unknown IL layout.

Use a unique patch owner and remove only this plugin's patches. Check other patch owners and
validate the resulting path. Harmony may rebuild transpilers when another mod changes a method.
Inlining, generic methods and external/native methods require separate handling. Successful
`Patch()` return does not prove execution. Verify bounded hit counts at the intended camera/pass.
These constraints follow the primary [Harmony patching guide](https://harmony.pardeike.net/v2/articles/patching.html),
[transpiler guide](https://harmony.pardeike.net/v2/articles/patching-transpiler.html) and
[edge cases](https://harmony.pardeike.net/v2/articles/patching-edgecases.html).

Harmony is selected conceptually. No package was downloaded, pinned, integrated or run here.
Before implementation, select a release compatible with the shipped Mono runtime, lock its
dependencies and record its bundled license/notices in `docs/dependencies.md`. The reference
[Harmony repository](https://github.com/pardeike/Harmony) describes its merged package option.

## First URP RenderGraph adapter

The following are inspected method names in the shipped assembly, not installed patches:

| Boundary | Proposed responsibility |
| --- | --- |
| `UniversalRenderPipeline.InitializeStackedCameraData` | Render scale, source/target descriptors and MSAA policy before dependent allocations |
| `UniversalRenderPipeline.InitializeAdditionalCameraData` | Camera-specific AA/history/jitter setup before projection and downstream culling/render use |
| `TemporalAA.CalculateJitterMatrix` | Read actual frame-indexed jitter and its render-pixel units; avoid independent camera jitter |
| `UniversalRenderer.GetRenderPassInputs` | Request depth/motion from their producers for the selected reconstruction path |
| `PostProcessPassRenderGraph.RenderTemporalAA` / `RenderSTP` | Candidate replacement recorder for SR before bloom and native tonemapping |
| `UniversalRenderer.OnAfterRendering` | Final scene colour and SDR overlay ordering for HUD-less capture |
| `DrawScreenSpaceUIPass.RenderOverlay` | Output-resolution UI boundary and native reinsertion |

Do not merely set a game's camera to TAA once. Drag'n Wash's settings listener rewrites its AA
policy on enable/settings changes. Own the effective engine camera setup for opted-in views and
restore the game's current policy when disabling. Do not blanket-enable TAA on UI, reflections,
offscreen cameras, overlay stacks or cameras whose post-processing is disabled.

The shipped TAA gate requires single-sample targets, post-processing, allocated history, a base
camera without a camera stack, no hardware dynamic resolution and motion-vector support.
Changing its result alone would bypass resource setup. Prepare the producer path before invoking
SR. A managed fallback that falsely reports temporal eligibility is not a valid adapter.

STP demonstrates output-size allocation and `UpdateCameraResolution` before downstream passes.
A replacement SR recorder must preserve that dimensional handoff, colour space and fallback.
Keep bloom, colour grading, exposure and tonemapping with the engine. Inspect and validate each
quality transition rather than replacing final display colour at Present.

## Rendering execution and resource lifetime

The inspected Raster, Compute and Unsafe command-buffer wrappers expose
`IssuePluginEventAndData`. Record a graph pass with declared input/output dependencies and a
native render-thread callback. Prevent culling of required side effects and treat external GPU
work as ordered graph work. Resolve actual resources inside pass execution; recording is earlier
than physical allocation/execution and a `TextureHandle` is graph-scoped.

The managed `Texture.GetNativeTexturePtr` wrapper calls Unity's native InternalCall binding.
Harmony access to that wrapper does not replace native resource ownership. Use it only on a
validated materialized texture/RTHandle. Unity documents API-specific pointer
types, pointer changes and render-thread synchronization costs in
[the native texture API](https://docs.unity3d.com/6000.3/Documentation/ScriptReference/Texture.GetNativeTexturePtr.html).
Reflection and Harmony do not supply D3D12 resource-state ownership, queues or fences. Determine
Unity's supported native graphics interface loading path before implementation; an arbitrary
`LoadLibrary` does not promise Unity plugin registration. Inspect the actual API/device and
command context before passing resources to the orchestrator. D3D11 and D3D12 need distinct
execution adapters. Do not apply the AC7 D3D11 bridge to an unmeasured D3D12 player.

Declare reads/writes and use supported graph APIs for externally scheduled work. An unsafe pass
has explicit ordering/state obligations; see Unity's
[unsafe pass guidance](https://docs.unity3d.com/6000.3/Documentation/Manual/urp/render-graph-unsafe-pass.html).
Retain packet memory, callbacks and resources until their queued work completes. GC pinning
does not retain Unity GPU contents. On stop, prevent new work, drain queued callbacks/GPU work,
restore owned camera state, remove this plugin's patches and release managed/native resources.
Keep the native DLL loaded while callbacks can still target it. Managed code may need to remain
loaded until the scripting domain ends; stop must leave it inert rather than claiming assembly unload.

Carry session, engine frame, camera instance, view/eye and history-generation identity together.
`Time.frameCount` alone is insufficient for multiple cameras, repeated rendering or stereo.
Cuts, camera replacement, pipeline recreation, scale/API changes and resize reset history.

## Validation sequence

The subsequent [DX12 runtime increment](unity-dx12-runtime-20261004.md) records implementation,
synthetic GPU checks, live activation and shared overlay controls separately from this plan.

1. Load an inert managed helper into the real player domain and report bounded discovery results.
2. Install diagnostic Harmony hooks and prove the selected camera and recorder execute. Unpatch
   and drain repeatedly without changing the rendered image.
3. Add a graph pass that copies through native execution, retaining native fallback. Verify
   resource formats, rectangles, thread/API identity, ordering and resize/scene transitions.
4. Capture colour/depth/motion plus jitter and independent object motion, including custom
   fluid/decal/deformation paths. Verify shader conventions and previous-state coverage.
5. Implement SR and dimensional reinsertion. Compare matched scenes at full/reduced scale,
   settings changes, cuts, pause/resume, disable/reenable and teardown.
6. Establish the completed HUD-less image and output-resolution UI reinsertion, then integrate
   FG and latency independently with the shared orchestrator.

Reflection makes managed discovery and adaptation easier. Working rendering still requires
those execution, lifetime, convention and image checks. No item above has been runtime-tested
by this investigation.
