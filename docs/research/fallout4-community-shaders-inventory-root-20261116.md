# Fallout 4 Community Shaders inventory and RSF comparison

## Scope and provenance

This root inventory asks what the complete reference implementation contains,
which mechanisms support its temporal rendering, and what RSF has not reproduced
or independently verified. It is source analysis, not a CS or RSF runtime test.
The independent [Claude inventory](fallout4-community-shaders-inventory-claude-20261116.md)
was reviewed after the root source map and temporal-path inspection. Differences
and agreement are reconciled below rather than treating two reports as runtime proof.

The fetched upstream main and local HEAD both equal
`e0e711594291c1a8e447ac758099492514810fbb`, dated 14 November 2026. Source is
`https://github.com/northaxosky/fallout4-community-shaders`. Four pinned submodules:

| Dependency | Revision | Consumed role |
| --- | --- | --- |
| CommonLibF4 | `7f8deca16dc7dd245c9c52151ec9ef27edd9ad60` | Native types, IDs, F4SE entry and build/package rules |
| community-shaders-shared | `6e67083a54090d10806484549d07f75b7fbc8dc2` | Explicit shader/source manifest in `xmake/shared.lua`, not the whole Skyrim plugin |
| Streamline fork | `68bad8fe3ad61d25b1f94a17939c34ab3ce17d88` | Headers; runtime DLLs are separately staged |
| bethesda-devbench | `1d804c5241019ecc456a4c7d385109c90f56ce1f` | Remote-control client API; no CS-owned server |

Root initialized the latter two missing submodules for this inventory. This did
not stage CS, F4SE or vendor runtimes in the game. Unused nested Skyrim/CommonLib,
Streamline/FidelityFX and DevBench application code are not required by the
FO4 build manifest and are outside the consumed implementation reviewed here.

The parent has 644 tracked entries: 640 regular files and four gitlinks. Root's
ignored `community-shaders-full-inventory-manifest-20261116.json` records every
regular file's SHA256/size; `community-shaders-source-index-root-20261116.json`
adds line counts, includes, shader register declarations and symbol candidates.
Regex fields are navigation aids, not semantic or native proof. Claude's 644-row
manifest includes gitlinks, explaining its different count without a source mismatch.

| Parent bucket | Regular files | Inventory coverage |
| --- | ---: | --- |
| `src` | 251 | All subsystem ownership mapped; temporal/camera/hook/compatibility paths inspected in depth |
| `features` | 194 | All 27 feature directories and callback/shader contributions accounted for below |
| `package` | 86 | 79 shader files plus seven assets/configuration entries; binaries counted, not reverse engineered here |
| `tests` | 18 | Build registration, identity/ABI/cache/configuration coverage and limits |
| `docs` | 31 | Feature/deviation coverage; source overrides unverified documentation claims |
| `scripts`, `xmake`, `.github` | 20, 7, 7 | Dependency fetch, shader staging, formatting, isolated deployment, package and CI roles |
| `extern` parent files | 14 | Vendored interfaces/notices; submodule contents counted separately |
| Other root files | 12 | Build, metadata, licensing, README and contribution contracts |

This is a complete inventory of ownership and mechanisms, not a claim to have
audited every arithmetic expression or external SDK implementation. Detailed
active shader/buffer coverage remains a separate required audit. Binary assets,
unused dependency code and exhaustive test-data-row review are explicit exclusions.

## Core subsystem map

All source anchors in this note use paths relative to the pinned CS checkout.
Unqualified temporal filenames are under `src/Render`; `DynamicResolution.cpp`
and `ShaderDefines.h` are under `features/Upscaling/src`, and
`MotionVectorFixes.cpp` is under `features/MotionVectorFixes/src`.

| Owner/files | Behavior and important boundary |
| --- | --- |
| `src/XSEPlugin.cpp:26-136` | F4SE Query/Version/Load, Address Library and layout admission, settings and compatibility first, shader/creation routing and feature activation, then frame/render hooks. PostPostLoad freezes temporal requests; GameDataReady publishes loaded-world state. |
| `src/Feature.*`, `FeatureState.h`, `FeatureCategories.h` | Registration order, opt-in activation, settings persistence and runtime quarantine. `Feature.cpp:48` observes actual LoadingMenu close for history reset. |
| `src/Settings/*`, `Menu/*`, `Host/*` | Typed TOML schemas and registry, live edit transactions, presets/weather overrides, delta persistence and DearModdingUI host pages. Feature loading is startup-only; loaded feature settings can change live. Headless use remains possible. |
| `src/Compat/*` | Scans plugin metadata and applies feature-specific conflict rules. `Rules.cpp:37` disables all features for ENB. This restriction is not RSF's requested compatibility policy. |
| `Render/SwapChainHook.*`, `D3D11Bootstrap.*` | Native D3D11 creation interception and renderer bootstrap. The separate creation router admits requested SR/FG/HDR topology. |
| `Render/DX12SwapChain.*`, `DXGISwapChainProxy.*`, facade contract | Application-facing D3D11 chain over a private D3D12 presentation path; shared resources/fences, two frame slots, provider preparation, real Present and resize/drain. |
| `Render/TemporalPipeline*`, provider/presentation contracts, `LatencyRouter.*` | Requested/effective configuration, providers, frame/slot ownership, reset epochs, input preparation, latency markers and SDK failure disposition. This is a reference architecture, not code to move into an RSF game adapter. |
| `Render/TemporalRenderer*`, `TemporalResolve.*`, `TemporalRenderHooks.*`, `TemporalFrameGenerationInputs.*` | Native frame preparation, size/jitter publication, proxy scopes, resolve and auxiliary publication, HUD-less and FG capture. Detailed order follows below. |
| `Render/Engine.h`, `RendererContext.*`, `EngineCallSite.*`, `RenderTargetFormats.*` | Logical-to-platform resolution, native state access, expected call-target validation, scoped target/geometry/unscaled operations. Engine identity is explicit rather than inferred from a texture's allocation order. |
| `Render/FrameBuffer.*`, `CanonicalDepth.*`, `SharedData*`, bindings/layouts | Keyed world-camera capture at prepass, shared shader constants and history reset, canonical scene/output depth for owned consumers. The canonical product is not the SDK depth product. |
| `Render/FrameBoundary.*`, `FrameDispatcher.*`, `FrameGenerationCpuTiming.h` | Ordered pre/post Swap callbacks, render-thread work and bounded timing statistics. |
| `Render/ShaderInjection*`, `ShaderSubclassHooks.*`, variant/compile/observer/family files | Native shader-family/descriptor normalization, feature define contributions, replacement ownership, cache and compilation gates. Identical family names are not evidence of identical native shader behavior. |
| `Render/RenderHooks.*`, `HookRegistry.h`, `ForwardDraw.*`, `LightingRenderPasses.*` | Callback registries around engine phases, selected draw/shader routes and feature-owned native material work. |
| `Render/Occluder*`, `PrecipitationOcclusion.*`, `SkyReflectionCube.*`, shadow/light/terrain/object files | Feature-owned geometry gathering, shadow/cubemap/depth/light passes, LOD and water consumers. These can introduce non-world views that temporal ownership must distinguish. |
| `Render/HdrCompose.*`, `InterfaceLayer.*` | HDR output composition and a separate interface layer with translated blend behavior. SDR FG instead uses HUD-less plus final colour. |
| `Render/Annotation.*`, `FrameAnnotations.*`, `FrameProfiler.*`, `RgbaPreview.*`, `src/Capture`, `Telemetry` | Capture/debug contracts, markers, timings, preview and status. None supplies missing temporal geometry history. |
| `src/Utils/*` | Hook ownership, detours, buffer helpers, hashing, paths and shader compile/cache/dependency machinery. |
| `src/World/*` | Sky, weather/time, sun/moon, climate and water state consumed by rendering features and editor tools. |

## All feature owners

Paths are `features/<directory>/src`; shader counts cover files owned by that
feature, excluding explicitly imported shared shaders and replaced package shaders.
Callback names come from the feature interfaces; final draws are in their
implementations and shader contributions rather than implied by a feature name.

| Directory | Files / feature shader files | Purpose and connection |
| --- | --- | --- |
| CloudShadows | 5 / 1 | Prepass cloud-shadow state and shared-data/shader contributions to lighting/water |
| CSEditor | 6 / 0 | Data-loaded/prepass weather/time controls and per-weather feature overrides |
| DynamicCubemaps | 8 / 3 | Prepass scene reflection capture and filtering; shared material data; prerequisite for IBL |
| ExtendedMaterials | 4 / 0 | Material/terrain parallax and shared feature constants; owned shader prerequisites |
| FrameGeneration | 7 / 1 | Provider selection/configuration, shared capture contracts, presentation and generation lifecycle |
| HDRDisplay | 9 / 1 | HDR output settings/shared-frame data, interface composition and display transfer |
| IBL | 4 / 0 | ImageBasedLighting registration, cubemap-dependent prepass lighting and weather overrides |
| LightLimitFix | 24 / 1 | Clustered/local-light data, shadow copies, material shader variants and data-loaded light definitions |
| LODBlending | 4 / 0 | LOD brightness/gamma/material constants and shader defines |
| MotionVectorFixes | 2 / 0 | Player/sequence transform history and frozen/LOD pass corrections, with LoadingMenu state |
| PerformanceOverlay | 3 / 0 | Overlay graph/profiling and telemetry, including runtime quarantine handling |
| RemoteControl | 12 / 0 | PostPostLoad DevBench tool registration; shared NvPerf adapters; no independent server |
| RenderDoc | 5 / 0 | Device-ready capture integration, overlay control and restart-sensitive settings |
| Screenshot | 5 / 0 | Asynchronous image output, HDR/crop/clipboard controls |
| ScreenSpaceGI | 6 / 0 | Prepass AO/indirect lighting, shared compute shaders and shared feature state |
| ScreenSpaceShadows | 6 / 0 | Prepass raymarch/contact-shadow data and shared Bend shader/CPU support |
| Skylighting | 5 / 1 | Sky-visibility probes/prepass and lighting shader data |
| SkySync | 3 / 0 | Native sky/sun/moon/godray alignment hooks and data-loaded state |
| SubsurfaceScattering | 10 / 2 | Skin scattering variants, resource and data-loaded lifecycle |
| TerrainBlending | 3 / 0 | Terrain/object seam shader contributions and device resources |
| TerrainShadows | 5 / 1 | Data-loaded heightmaps, prepass long-range shadow updates and terrain shader data |
| TerrainVariation | 4 / 0 | Stochastic material/terrain sampling through shader and shared-data contributions |
| UnifiedWater | 6 / 0 | Loaded-world/native water changes, shader data and feature overlay |
| Upscaling | 29 / 8 | SR providers, render-size query, DR proxies, latency providers, Agility bootstrap, RCAS and ReShade integration |
| VanillaPBR | 10 / 3 | Prepass/material GGX/reflection data and owned shader variants |
| VolumetricShadows | 4 / 1 | Device-ready shadow filtering and shared feature constants |
| WaterEffects | 5 / 2 | Prepass caustic/parallax/debug products and water shader data |

`src/Settings/FeatureKeys.h:21` orders 27 registered keys, including
ImageBasedLighting for the IBL directory, and statically orders its dependency
after DynamicCubemaps. Counting directories without that key mapping would
miss a real activation constraint.

## Shader infrastructure and buffer boundaries

`Render/ShaderInjectionTargets.h:13` enumerates 13 injection families:
deferred prepass, utility, particle, effect, blood splatter, distant tree,
imagespace, sky, water, lighting, deferred light, deferred composite and tiled
lighting. Most admit VS/PS; imagespace additionally admits CS and tiled lighting
is CS. This is the replacement registry's stage coverage, not evidence that
native HS/DS/GS stages are absent or supported by replacement code.

The 79 package shader files contain reconstructed world/deferred/image-space
families plus FO4 substrate/constant/depth includes. Feature shader files add
compute or fullscreen products. `xmake/shared.lua:1-121` explicitly imports
shared common colour/math/G-buffer/frame data, lighting, shadows, cubemaps,
GI, materials, terrain, water and RCAS shaders, plus profiler/NvPerf/math/hook
utilities. It checks source existence and rejects duplicate/unsafe manifest
entries. It does not compile the entire Skyrim source tree into FO4.

`ShaderFamilyDescriptor.cpp:37-145` normalizes prepass descriptor bits and adds
MOTION_VECTORS to the reconstructed prepass recipe. Native output/binding
coverage still requires checking native bytecode and real draws. Shader ABI
tests and the AE/OG identity TSVs establish specific compiled correspondence,
not full moving-scene coverage. Their scope is separate from RSF's existing
3,939-record offline shader inventory and the newly required full buffer audit.

World-camera capture (`FrameBuffer.cpp:22-84`) uses the world reference camera,
jitter key and native frame, not an arbitrary inherited camera buffer. The
canonical-depth path (`CanonicalDepth.cpp:75-154`) uses the near inverse and
world projection and publishes scene/output products separately. The SDK
encode path still receives partitioned native depth; the deviation document
explicitly marks world-projection SDK depth pending. It is not a precedent
for passing mixed depth without verifying semantics.

## Temporal frame order and necessary supporting mechanisms

1. Creation routing freezes requested SR/FG/HDR topology before device creation;
   the private presentation path requires windowed operation. Device/resource
   setup is deferred when native resize has not yet repopulated RT0.
2. At native temporal update, the renderer finalizes earlier failures, configures
   native TAA and queries provider render size. `TemporalRenderer.cpp:155-223`
   preflights resolve/recovery resources before committing size, jitter or mip bias.
3. After native UpdateDynamicResolution, `PublishDynamicResolution:226-261`
   builds proxies, publishes ratios and computes offsets with the native camera
   builder's division by ratio accounted for. Halton phase count depends on
   input/output resolution. Owned prepass/forward shaders see scoped mip bias.
4. Composite and SSLR are not simply left with smaller viewports.
   `TemporalRenderHooks.cpp:298-369` wraps them in proxy passes; pooled BlurH
   is handled within acquire/release. `DynamicResolution.cpp:364-546` also
   adjusts native descriptor dimensions and already-bound SRVs, handles depth,
   and restores/copied results at the matching boundary.
5. Lens-flare visibility uses a scoped depth override. Interface3D Pip-Boy and
   companion local map callbacks use unscaled scopes before the SR seam.
6. At Render_UI's effect-range call, the renderer snapshots pre-tonemap Main,
   captures FG inputs before mutation, evaluates SR and publishes full Main.
   `TemporalResolve.cpp:272-397` distinguishes vendor publication, spatial
   recovery, skipped frames and unsafe submission. Recovery resolves committed
   reduced colour instead of assuming native TAA will expand it correctly.
7. Full-extent depth/refraction/motion/Pip-Boy/VATS inputs are supplied to the
   remaining chain. Raw chain velocity is distinct from provider-dilated motion.
   VATS uses a separate full copy and scoped target swap, not a lasting replacement.
8. Ratios become1 and offsets0, with a unit triangle for output work. HUD-less
   capture occurs after imagespace and before interface drawing. EndRenderUI
   resets ratios to1 and restores triangle/target swaps.
9. Actual Present consumes frozen slot-specific constants and shared inputs;
   D3D11/D3D12 queue signals/waits order GPU copies and provider work. Resize
   quiesces and drains before replacing resources.

Relevant anchors are `TemporalRenderHooks.cpp:17-88,129-184,200-298`,
`TemporalResolve.cpp:272-475,475-710`, `DynamicResolution.cpp:23-46,364-695`,
and `TemporalFrameGenerationInputs.cpp:230-447`. Native verification must
still establish matching RVAs, call order and field ownership in the installed
AE executable; source names alone do not do that.

The imagespace VS (`package/Shaders/FO4/ImageSpaceVS.hlsli:46-57`) restores
fp32 scale within one fp16 ulp because the native dynamic triangle bakes ratio
into half UVs. `Upscaling/src/ShaderDefines.h` limits that contribution to owned
imagespace VS. It is a shader repair in addition to engine ratio writes.

MotionVectorFixes caches player world transforms around UpdatePlayer, repairs
sequence-position history and corrects frozen/LOD draws outside LoadingMenu
(`MotionVectorFixes.cpp:34-183`). FG's alpha conditioning is a colour-difference
heuristic, not geometry-derived independent transparent motion. The reference
documents BLEND history and reactive/transparency production as incomplete.

Latency runs through CS's own temporal/provider architecture: main-loop message
boundary, simulation/render markers and private Present. These reference
substitutes do not prove RSF's consumed worker-source association and must not
be transplanted into its adapter as vendor timing policy.

## Build, settings, compatibility and test scope

The build is xmake/MSVC/C++latest, version0.5.1, with vcpkg baseline
`9eec002df1cd109f8f104cd966a763daf9751489`; it stages shader sources and runtime
DLLs separately. CommonLib owns plugin packaging. The documented development
build can install automatically, so it was not run here. See `CONTRIBUTING.md`
and `xmake.lua:1-152,228-342`.

Tests cover feature configuration/defaults, Agility bootstrap, shader ABI and
stock identity, runtime shader compilation and cache behavior. CI runs unit
and shader groups, formatting and package assembly. These tests are not an
executed game test of proxy scopes, failed SR recovery, FG output or latency.
No reference build/test was performed for this inventory.

ENB disables all features in source, although the current RSF goal requires
later ENB coexistence. F4SE and Address Library are entry requirements for CS,
not unavoidable requirements for the native RSF implementation. ReShade/HDR
routes and hardware/provider constraints must be considered separately from
SR data correctness. Feature-load toggles need restart, unlike live settings
for loaded features. RSF must retain its own shared-overlay option contract.

## Reconciliation with Claude and current RSF work

Both inventories identify composite/SSLR proxy scopes, lens-flare depth,
unscaled offscreen/menu draws, fp16 UV correction, mip bias, transform-history
correction and committed-frame recovery as mechanisms missing from or
unverified in RSF. They are an actionable work list, not proof that every CS
implementation must be copied. RSF's generic services and guarded native
contracts must express the required behavior at the correct owners.

Claude identified restored reduced ratios after Render_UI as a concrete source
difference. The native instruction audit in the reduced-output scope note found
no established late copy in LateRenderEnd/list cleanup. Therefore that difference is retained,
but a longer output scope is not justified as a repair without a real consumer.
Likewise logical15's menu-background reuse merits identity/lifetime checks;
it does not establish aliasing with Main in the rejected frame.

Native-only PID66836 with c32d8f7/Entry72B274B9... visibly reproduced the
Quality1 rectangle with F4SE/ENB absent. This disproves ENB necessity, not all
possible ENB interactions. Its partial capture source16183/native16705 has
only native inputs at full rect after reload and cannot localize that screenshot's
first bad product. The unbuilt generic plan-only capture correction and bounded
reduced-scope diagnostics address this evidence gap before the next experiment.

The root REA/IDA analysis of matched Fallout4.exe RenderEffect2185610 now
provides fresh pseudocode/assembly evidence, parent
`ev_6e33f5289a857ff63f543b39a06e3f455326618712185ca24c9e2ef71730c6fc`.
It independently agrees with the selector: manager+48 selects partial geometry;
otherwise effect+A8 and RTM+FE4 select dynamic+38, else unit+28. This proves
that branch's semantics, not which geometry the failed frame used. Prior stale
IDA failure remains recorded; it is not promoted to successful decompilation.

## Fresh matched-executable REA checks

Root used REA's IDA provider against Fallout4.exe SHA256
`fdcef37ac1230af6d0b0050eb2142b139ef3a867b37b9211fb6edfcc646072f8`,
with static image base `0x140000000`. These checks answer where the dynamic UVs
are produced and which native composition branches surround the reference
proxy seams. They do not identify the failed screenshot's active draw.

| Native function / retained parent evidence | Result |
| --- | --- |
| RenderEffect RVA2185610, `ev_6e33f5289a857ff63f543b39a06e3f455326618712185ca24c9e2ef71730c6fc` | Partial manager+40 overrides; otherwise effect+A8 and RTM+FE4 choose dynamic+38, else unit+28. |
| Dynamic triangle RVA2186110, `ev_69fe402137058b3c0941e1197afa21aefd712f58a9689600a89c8e137e4f35fb` | Assembly loads FBC/FB8, multiplies each by the same constant, calls770110 and writes 16-bit fields+6/+8 in the mapped dynamic geometry buffer, then commits. The decompiler omitted XMM call arguments, so assembly is necessary. |
| Float conversion RVA770110, `ev_9f9a52d2856c13a1a3a9391f19bd2b72d9d42987f61d29fb77f150535ab29039` | Sign/exponent conversion and shift13 with bias4095 plus tie bit establish binary16 conversion, including the normal-value nearest-even rule. |
| UpdateDR RVA1839ED0, `ev_c9e2622a120daf18b6b67d49eb816adf6a8ab02539a34648cb20262a5835b9ea` | Ratio updates and eight-pixel dimension quantization/bias, ending in FE5 activation. Its 194 instructions contain no calls or triangle-buffer write. |
| PreUI RVA21F9C90, `ev_fb6c2a1eba02cef37235f82b50c50ef26a932d0c2263268d59775980529e3d2e` | Actual call21F9F2F invokes UpdateDR, then21F9F3B invokes the dynamic triangle producer, before prepass21F9F73. RSF's existing post-UpdateDR hook therefore publishes its ratios before this native bake. |
| Composite RVA21F82A0, `ev_03761a3a35d6ad9c0d96826a5b63f39f98db53e043a3018ab1f99a38d6015105` | First selected render-pass call21F8495, conditional reflection/blur work, Main selection at21F8979 or21F89D7, second render-pass21F8BB5 and optional21F8C7E. These branches confirm distinct composition stages; their active shader sampling and failed-frame contents remain unproved. |

The earlier independent PE/disassembly receipt identifies the multiplier at
RVA2929498 as float2. REA `read_bytes` is unavailable for this IDA provider;
that failed operation is retained rather than described as a successful byte
read. The assembly and converter corroborate the half-UV producer independently
of the reference comment. Changing ratios after this bake cannot alter stored
UVs. This supports checking producer/consumer geometry and precision; it does
not justify replacing every triangle or attributing the rectangle to half rounding.

The ignored `rea-cs-native-reference-root-20261116.json` bundle retains the
21 function/search dossier and view records. The owned analysis was closed
after export, with a separate21-record snapshot. The DynamicResolution string search
returned11 provider rows, which are discovery leads, not verified call sites.
The provider does not expose typed caller edges, CFG or value-flow facets;
empty unsupported fields are unknown. A stale-session failure preceded the
successful reopened analysis. No game or reference code was executed here.

Remaining requirements are a matched failed reduced evaluation/capture,
consumer/buffer/history coverage and necessary repairs, moving-game quality,
actual consumed-input/FG/latency admission, startup/resize/close and subsequent
mod compatibility. The reference inventory does not complete those requirements.
