# Anomaly 1.5.3 DX11 AVX profile

## Scope and evidence

Question: why does the launcher's DX11 + AVX executable refuse entry, and can the existing adapter use its native boundaries safely?
The standard-only fingerprint was the cause of refusal. `build_profile.h` now selects an immutable native/geometry/SR/motion table by executable basename and full SHA-256 before preparing the observer or graphics services. Unknown names, mismatched name/hash pairs and unknown hashes refuse entry. Selection and policy remain in `games/anomaly`; no runtime game switch was added.

Inputs, read only on 2026-11-12:

| Executable | SHA-256 | SizeOfImage |
| --- | --- | --- |
| AnomalyDX11.exe | c857b6f49d32c62245611b26106862a773185533856e8b858a7d7e849ed2fb4e | 0xfca000 |
| AnomalyDX11AVX.exe | 2032acfc041e215508692ef4079ef504c7bbabf1793d5668084099f734b92f12 | 0xfb8000 |

The existing named standard analysis under `.local/anomaly/sr-stage0/ida` and the standard research provide function labels; the retained transfer artifacts use RTTI and normalized instruction/call-graph matching. Its retained matching inputs and derivations are under `.local/anomaly/avx-handoff/scratch/` (`need.pkl`, `map.pkl`, `rtti.json`, `gen.json`, `binlib.py`). Similarity selects candidates only. This continuation re-read executable bytes and decoded the relevant operands with Capstone, and verified the semantic tests below against both exact hashes. No game execution, injection, deployment or new analysis database was needed. Repository baseline: bb80598755689af52f6b7eb9568405e02e966b50 plus the shared uncommitted Anomaly work. The matching source reference is xray-monolith revision 028ccf5, as in the existing standard profile research.

## How each group was checked and used

* Observer: eight entry guards, image size and unique prefixes; FG checks End's swapchain read and sole Present tail jump, RTTI-derived RenderUI and RenderToTarget slots, scope/PDA call order in CLevel::OnRender, and the depth owner. These hooks bracket native rendering, reset and HUD transitions; source-frame identity does not prove simulation ownership.
* Geometry: lifecycle and visual entries matched to CRender slots 36-39, rmNear/rmNormal slots 55/57 and visual slot 7 through PE RTTI. The hierarchy whitelist is reconstructed from FHierrarhyVisual, CKinematics, CKinematicsAnimated and FLOD RTTI. World transforms are derived from set_W callers and the matrix-copy body; bone stride 0xa8 and render matrix +0x40 are decoded. constants.flush's 128 AVX callers and subsequent DrawIndexed calls establish the per-draw seam. Detail/tree bodies prove constant-table ownership, grass dump flushes and tree PM last_lod +0x108. These values are read under the existing geometry callbacks and leases, not by scanning during gameplay.
* SR: the original 15 function entries, five call instructions and six first-person projection/upload pairs checked against call targets and complete caller sets. The render target constructor identifies every one of the 26 ref_rt members by its name, object displacement and format argument. u_setrt, CRT::create, surface_set and Invalidate prove CRT/CTexture/RCache offsets; SetupStates and scene_begin prove sampler ownership. Nine world twins and four absent targets are checked against those slots. Half-depth option bits, SSAO dispatch and the static viewport guard/initializer are decoded. Loading-screen flags are independently found from the game_loaded message and PreCache writes. These tables serve Stages 1-5; Stage 4/5 GPU passes add no further executable sites.
* Motion: create_shader's error strings, device slots +0x60/+0x68/+0x78, D3DReflect and D3DGetInputSignatureBlob imports identify shader creation before reflection/signature extraction. Its complete caller set is exactly cache-hit and freshly compiled bytecode paths inside shader_compile. The caller stack is re-derived below. scene_begin/end callers all belong to CRender::Render. Hull/domain dispatch, wrapper device slots, shader-object store and constant destinations are independently decoded. The motion callback and shader loader borrow the selected profile and retain the existing frame/resource lifetimes.
* FG and overlay: FG adds no separate executable-site table or detour; it consumes the selected observer boundaries, camera/viewport/Device data and SR HW depth table. Present ownership, reset and the HUD-less seam are proved above. The overlay uses the existing generic Runtime services and adapter options, with no additional AVX addresses.

See [native hooks](anomaly-native-hooks.md), [geometry coverage](anomaly-geometry-coverage.md), [SR seam](anomaly-sr-seam.md) and [foliage motion](anomaly-foliage-motion.md) for the source/ownership rationale of the standard path. The evidence scripts provide executable assertions, not runtime acceptance.

## Entry and layout guards

All values below are RVAs from image base 0x140000000. Guards are exact hexadecimal bytes at the named RVA. The tables in the headers are authoritative; the following inventory records the AVX mapping alongside the standard RVA.

### Native sites

| Site | Standard RVA | AVX RVA | AVX expected16 |
| --- | --- | --- | --- |
| IRenderDeviceRender::Begin | 0x822cb0 | 0x81a930 | `40534883ec20488bd9488d0d80725300` |
| IRenderDeviceRender::End | 0x822eb0 | 0x81ab40 | `4883ec28833db9ed4b00007406488b01` |
| IRenderDeviceRender::SetCacheXform | 0x823250 | 0x81aee0 | `40534883ec20488d0d2b6d5300498bd8` |
| CRender::Render | 0x8977c0 | 0x88d420 | `488bc448894808535657415441554156` |
| CHUDManager::RenderUI | 0x0f1e60 | 0x0f0810 | `40534883ec40f7056819be0000100000` |
| CRender::RenderToTarget | 0x898640 | 0x88e290 | `40534883ec30448bc24183e801744d41` |
| CRender::reset_begin | 0x83a020 | 0x8312d0 | `48894c2408565741564883ec3048c744` |
| CRender::reset_end | 0x83a1c0 | 0x831470 | `40574883ec5048895c2468488bf94889` |

Extended AVX guards, the shortest unique prefix in `.text`:

| Site | RVA | Exact bytes |
| --- | --- | --- |
| CRender::Render | 0x88d420 | `488bc44889480853565741544155415641574881ec50` |
| CRender::reset_begin | 0x8312d0 | `48894c2408565741564883ec3048c7442420feffffff48895c2460488bf9` |

Per-build scalar addresses (the paired values are read independently from code, not calculated by a relocation delta):

| Field | Standard | AVX |
| --- | --- | --- |
| image_size | 0xfca000 | 0xfb8000 |
| device_width_rva | 0xd37668 | 0xd25668 |
| device_height_rva | 0xd3766c | 0xd2566c |
| device_frame_rva | 0xd37680 | 0xd25680 |

### Geometry sites

| Site | Standard RVA | AVX RVA | AVX expected16 |
| --- | --- | --- | --- |
| CRender::model_Create | 0x839940 | 0x830bf0 | `488b8958070000e944ce0400cccccccc` |
| CRender::model_CreateChild | 0x839950 | 0x830c00 | `488b8958070000e9a4d00400cccccccc` |
| CRender::model_Duplicate | 0x839bd0 | 0x830e80 | `488b8958070000e904d70400cccccccc` |
| CRender::model_Delete | 0x839b40 | 0x830df0 | `40534883ec20488b02488bda488b8958` |
| Fvisual::Render | 0x8cff60 | 0x8c5970 | `48895c2410555657415641574883ec30` |
| FProgressive::Render | 0x8d0800 | 0x8c6210 | `48895c241048896c2420565741544156` |
| CSkeletonX_ST::Render | 0x8d1610 | 0x8c7010 | `4883ec384c8bc1488d51684881c1a000` |
| CSkeletonX_PM::Render | 0x8d1520 | 0x8c6f20 | `4883ec38448b89c80000004c8bd9c5f8` |
| FTreeVisual_ST::Render | 0x8d5d20 | 0x8cb5e0 | `48895c2410555657415641574883ec30` |
| FTreeVisual_PM::Render | 0x8d5a40 | 0x8cb2f0 | `48895c241848896c2420565741544156` |
| CBackend::constants.flush | 0x84b930 | 0x842490 | `48895c2408574883ec20488d1d47fa50` |
| R_dsgraph_structure::r_dsgraph_render_hud | 0x84ffa0 | 0x846aa0 | `488bc448897010574881ec80010000c5` |
| R_dsgraph_structure::r_dsgraph_render_sorted | 0x850cc0 | 0x8476a0 | `488bc44889580848897010574881ec40` |
| R_dsgraph_structure::r_dsgraph_render_emissive | 0x84ec80 | 0x8457e0 | `488bc44889580848897010574881ec40` |
| CRender::rmNear | 0x83a450 | 0x831700 | `40534883ec50488b01c5f829742440ff` |
| CRender::rmNormal | 0x83a4d0 | 0x8317a0 | `40534883ec50488b01c5f829742440ff` |
| CDetailManager::Render | 0x86a8c0 | 0x860170 | `40534883ec204883b95804000000488b` |
| R_xforms::set_W | 0x840960 | 0x837c50 | `48895c240848896c2410488974241857` |
| CKinematics::LL_GetBoneInstance | 0x803d90 | 0x7fd6a0 | `0fb7c24869c0a800000048034150c3cc` |
| CKinematics::LL_GetTransform_R | 0x803da0 | 0x7fd6b0 | `4883ec28488b01ff50484883c0404883` |
| Fvisual::Render phase compare | 0x8cff83 | 0x8c5993 | `833df6d54800010f850b020000813dfa` |
| CSecondVPParams::IsSVPFrame | 0x2d260 | 0x2d180 | `80390074150fb6490133d28b05ef84cf` |
| dx10ConstantBuffer::Flush | 0x83de40 | 0x835130 | `40534883ec4080796800488bd9743248` |
| FTreeVisual_PM last_lod load | 0x8d5a66 | 0x8cb318 | `448b8b08010000c5f857c0c5f82ff00f` |

Extended AVX guards, the shortest unique prefix in `.text`:

| Site | RVA | Exact bytes |
| --- | --- | --- |
| Fvisual::Render | 0x8c5970 | `48895c2410555657415641574883ec30488bb9` |
| FProgressive::Render | 0x8c6210 | `48895c241048896c242056574154415641574883ec30` |
| FTreeVisual_ST::Render | 0x8cb5e0 | `48895c2410555657415641574883ec30488bd9` |
| FTreeVisual_PM::Render | 0x8cb2f0 | `48895c241848896c242056574154415641574883ec40` |
| R_dsgraph_structure::r_dsgraph_render_sorted | 0x8476a0 | `488bc44889580848897010574881ec4001000083b9e8` |
| R_dsgraph_structure::r_dsgraph_render_emissive | 0x8457e0 | `488bc44889580848897010574881ec4001000083b958` |
| CRender::rmNear | 0x831700 | `40534883ec50488b01c5f829742440ff5078c5c857f6c5fa11742420c5fa11742424488b10488bc8488bd8ff52508bc8c5f857c0c4e1fa2ac1c5fa11442428488b13488bcbff5258488b0d89` |
| CRender::rmNormal | 0x8317a0 | `40534883ec50488b01c5f829742440ff5078c5c857f6c5fa11742420c5fa11742424488b10488bc8488bd8ff52508bc8c5f857c0c4e1fa2ac1c5fa11442428488b13488bcbff5258488b0de9` |
| R_xforms::set_W | 0x837c50 | `48895c240848896c24104889742418574883ec208b0248` |

Per-build scalar addresses (the paired values are read independently from code, not calculated by a relocation delta):

| Field | Standard | AVX |
| --- | --- | --- |
| render_phase_rva | 0xd64f90 | 0xd52f90 |
| second_viewport_rva | 0xd37b30 | 0xd25b30 |
| world_matrix_rva | 0xd63c18 | 0xd51c18 |
| vertex_constant_buffers_rva | 0xd63e78 | 0xd51e78 |

### Sr sites

| Site | Standard RVA | AVX RVA | AVX expected16 |
| --- | --- | --- | --- |
| CRenderTarget::phase_scene_prepare | 0x8e4310 | 0x8d9160 | `40534883ec50f7053ca1470000400000` |
| CRenderTarget::phase_combine | 0x8bc980 | 0x8b2950 | `488bc45556415441554156488da888fe` |
| CRenderTarget::phase_sunshafts | 0x8f07c0 | 0x8e5090 | `4c8bdc554156498d6ba14881ecf80000` |
| CRenderTarget::phase_blur | 0x8f2590 | 0x8e6e40 | `488bc44889582055565741564157488d` |
| CRender::rmNear | 0x83a450 | 0x831700 | `40534883ec50488b01c5f829742440ff` |
| CRender::rmFar | 0x83a3c0 | 0x831670 | `40534883ec40488b01ff5078c5f857c0` |
| R_xforms::set_P | 0x83ecd0 | 0x835fc0 | `48895c240848896c2410488974241848` |
| R_xforms::set_V | 0x83f970 | 0x836c60 | `48895c240848896c2410488974241857` |
| CTexture::surface_set | 0x87ac30 | 0x870870 | `48895c24185556574883ec40488bfa48` |
| dx10SamplerStateCache::SetMipLODBias | 0x7fe170 | 0x7f7b80 | `40574881eca0000000c5fa10819c0100` |
| dx10SamplerStateCache::SetMaxAnisotropy | 0x7fe060 | 0x7f7a70 | `40574881eca0000000488bf983fa0173` |
| CBackend::Invalidate | 0x842c50 | 0x839f80 | `48895c240848896c2410488974241857` |
| CRenderTarget::u_setrt(W,H) | 0x859cb0 | 0x8502c0 | `4883ec288951084489410c4c3b0d661e` |
| PS::CParticleEffect::Render | 0x8d8420 | 0x8cd840 | `40534881ec80010000488bd9e87f470f` |
| CRenderTarget::accum_point | 0x90dca0 | 0x9021d0 | `488bc45553488da8c8feffff4881ec28` |
| CRender::render_sun_cascades | 0x815a30 | 0x80dfd0 | `48895c240848896c2410488974241848` |
| cl_pos_decompress_params::setup | 0x834f70 | 0x82c1c0 | `40574883ec30c5fa100586964f00c5fa` |
| CRender::Render -> phase_scene_prepare (z-prepass, r4_R_render.cpp:278) | 0x897d2c | 0x88d957 | `e804b8040033d2488d0d4bc54400e866` |
| CRender::Render -> phase_scene_prepare (r4_R_render.cpp:285) | 0x897d66 | 0x88d991 | `e8cab70400488b1d4b814900833d8cff` |
| CRender::Render -> phase_combine (r4_R_render.cpp:538) | 0x8985bf | 0x88e1ed | `e85e470200eb39488b05f5b644004889` |
| phase_combine -> phase_sunshafts (phase_combine.cpp:374) | 0x8bf273 | 0x8b5100 | `e88bff0200498bcee8331d0300498bce` |
| phase_combine -> phase_blur (phase_combine.cpp:378) | 0x8bf27b | 0x8b5108 | `e8331d0300498bcee8ab3f0300f70575` |

Extended AVX guards, the shortest unique prefix in `.text`:

| Site | RVA | Exact bytes |
| --- | --- | --- |
| CRenderTarget::phase_blur | 0x8e6e40 | `488bc44889582055565741564157488d68a14881eca0` |
| CRender::rmNear | 0x831700 | `40534883ec50488b01c5f829742440ff5078c5c857f6c5fa11742420c5fa11742424488b10488bc8488bd8ff52508bc8c5f857c0c4e1fa2ac1c5fa11442428488b13488bcbff5258488b0d89` |
| R_xforms::set_P | 0x835fc0 | `48895c240848896c2410488974241848897c242041564883ec208b02` |
| R_xforms::set_V | 0x836c60 | `48895c240848896c2410488974241857415641574883ec208b02` |
| CBackend::Invalidate | 0x839f80 | `48895c240848896c24104889742418574883ec2033ed488bd94889a9` |

Per-build scalar addresses (the paired values are read independently from code, not calculated by a relocation delta):

| Field | Standard | AVX |
| --- | --- | --- |
| crender_vtable_rva | 0xb669f8 | 0xb53b68 |
| device_view_rva | 0xd376c8 | 0xd256c8 |
| device_project_rva | 0xd37708 | 0xd25708 |
| device_full_transform_rva | 0xd37748 | 0xd25748 |
| device_precache_frame_rva | 0xd37670 | 0xd25670 |
| load_screen_registered_rva | 0xce41b0 | 0xcd21b0 |
| hw_device_rva | 0xceb8d0 | 0xcd98d0 |
| hw_context_rva | 0xceb8d8 | 0xcd98d8 |
| hw_swap_chain_rva | 0xceb8e0 | 0xcd98e0 |
| hw_base_rt_rva | 0xceb8e8 | 0xcd98e8 |
| hw_base_zb_rva | 0xceb8f0 | 0xcd98f0 |
| render_implementation_rva | 0xd64e40 | 0xd52e40 |
| render_options_word0_rva | 0xd652a0 | 0xd532a0 |
| render_options_word3_rva | 0xd652ac | 0xd532ac |
| sunshafts_mode_rva | 0xceb36c | 0xcd936c |
| sampler_cache_rva | 0xd63a20 | 0xd51a20 |
| user_mip_bias_rva | 0xd607a0 | 0xd4e7a0 |
| user_anisotropy_rva | 0xceb878 | 0xcd9878 |
| ssao_set_viewport_rva | 0x8f95f0 | 0x8eded0 |
| ssao_viewport_rva | 0xceebd0 | 0xcdcbd0 |
| ssao_viewport_guard_rva | 0xd79284 | 0xd67284 |
| backend_rva | 0xd63bc0 | 0xd51bc0 |

### Motion sites

| Site | Standard RVA | AVX RVA | AVX expected16 |
| --- | --- | --- | --- |
| create_shader | 0x838de0 | 0x830080 | `48895c2410488974241848897c242055` |
| CRenderTarget::phase_scene_begin | 0x8e3d20 | 0x8d8b90 | `40534883ec508b15dc0c4000488bd948` |
| CRenderTarget::phase_scene_end | 0x8e3df0 | 0x8d8c60 | `405557488d6c24b14881ecb800000048` |
| shader_compile -> create_shader (cache file, r4.cpp:1867) | 0x83b9b1 | 0x832cb1 | `e8cad3ffff89442468488bcee8fee086` |
| shader_compile -> create_shader (D3DCompile output, r4.cpp:1903) | 0x83bb93 | 0x832e93 | `e8e8d1ffffe9ea00000048837b50104c` |

Extended AVX guards, the shortest unique prefix in `.text`:

| Site | RVA | Exact bytes |
| --- | --- | --- |
| create_shader | 0x830080 | `48895c2410488974241848897c2420554154415541564157488dac24b0` |

Per-build scalar addresses (the paired values are read independently from code, not calculated by a relocation delta):

| Field | Standard | AVX |
| --- | --- | --- |
| shader_compile_rva | 0x83a610 | 0x831900 |
| source_offset | 0x14f0 | 0x14f0 |
| length_offset | 0x14f8 | 0x14f8 |
| entry_offset | 0x1500 | 0x1500 |
| flags_offset | 0x1510 | 0x1510 |
| defines_offset | 0x250 | 0x250 |
| d3dcompile_setup_rva | 0x83b9ce | 0x832cce |
| source_name_rva | 0xa4f210 | 0xa3c240 |
| tess_dispatch_rva | 0x839123 | 0x8303c3 |

## Code generation and stack proof

There is no constant code relocation delta. AVX changes scalar/SIMD encodings, register saves and stack allocations. For example rmNear uses a 0x50 frame and VEX moves instead of the standard 0x40 frame. SSAO set_viewport's guarded prefix through `lea r8,[viewport]` is 0x4a bytes rather than 0x46. Its initializer uses vmovss rather than movss; both still store only Width/Height in the static viewport and use the same TLS epoch guard protocol. The SR verifier now disassembles RIP operands, including VEX, instead of treating an SSE byte scanner as a decoder.

`verify_detour_prefix` decodes complete instructions through MinHook's five-byte relative relay patch for every native/geometry/SR/motion detour candidate. Every copied prefix is legacy integer code; no VEX/EVEX instruction is crossed. Guard bytes may extend past the relocated prefix and may include VEX. RIP memory targets in copied instructions are decoded for review; MinHook retains relocation ownership. Hook fixtures test installation and callback behavior with synthetic code and mapped profiles, not execution of AVX game instructions.

Both shader_compile prologues store r8 at entry rsp+0x18 and r9d at entry rsp+0x20, push seven registers, set rbp to entry rsp-0x13d8 and allocate 0x14a0. Consequently body rsp = entry rsp-0x14d8 and rbp = body rsp+0x100. At both guarded create_shader calls the source/length are body rsp+0x14f0/+0x14f8, entry name +0x1500, flags +0x1510, and the 128-entry macro array starts at +0x250. D3DCompile setup independently loads those fields and uses `lea r9,[rbp+0x150]`; no body instruction rewrites the protected argument slots. The AVX calls are 0x832cb1 and 0x832e93, with shader_compile at 0x831900. Exact prologue/setup bytes are in motion_profile.h and checked byte for byte.

The AVX tessellation dispatch at 0x8303c3 compares al with 'h', calls create_shader<SHS> at 0x82d200 or create_shader<SDS> at 0x82d150. Both store the D3D11 object at +0x10 before reflection, use RC_dest_hull/domain 0x10/0x20, and invoke wrappers with ID3D11Device slots +0x80/+0x88. Exact dispatch, create and wrapper byte strings are in motion_profile.h and verified through the final object store/device call. No frame offsets were inherited on similarity alone.

## Validation and limits

`py -3 -I tests/anomaly_profile_evidence.py` checks both installed hashes with all four evidence scripts, then mutates a guard, RVA and layout field for every script/build combination in temporary header copies: 8 positive and 24 negative checks. Native, geometry, motion and SR guards include minimal-prefix uniqueness checks. The suite never writes either executable.

Profile-selection tests cover mixed hashes/names, unknown renderer names, case folding and wide filenames. Geometry/SR hook fixtures run once per build; motion caller-frame fixtures cover both builds. The plugin/bootstrap contracts exercise exact fingerprints while readiness remains zero. No engine.json exists for this adapter.

No required profile site remains unmapped. Static checks do not establish AVX in-game acceptance, SR convergence, tessellated motion quality, FG pacing or latency; those remain game/device acceptance questions. This task does not resolve existing Stage 3 jitter-measurement misses or expand the standard path's acceptance scope.

First AVX game run: verify the exact AVX-profile selection message and successful native/geometry/motion/SR hook installation with no entry-refused status=2 or byte/layout refusal. Check observer Begin/End, main/scope/HUD/reset counts; geometry draw/flush and skinned/detail/tree coverage; motion cache-hit/recompile and hull/domain replacement/link counts; SR world enter/restore, jitter/restore/mip counts, evaluate/refusal/reset counters and output size; FG source/presented/unpresented, tagged/fed, provider Present and generated counters, with scope and loading frames held. Open the overlay and exercise saved live settings, backend switching and teardown. Confirm visual correctness separately for world, weapon, HUD, scope and foliage, then compare pacing/latency on the NVIDIA adapter; higher presentation counts alone do not establish lower latency.

## Relocated AVX prefixes

MinHook source check: `src/trampoline.c` copies until `oldPos >= sizeof(JMP_REL)` and `src/hook.c` uses that five-byte patch. The following complete-instruction lengths reach that boundary; a dash means no RIP memory operand in the copied instructions. This inventory is also retained with the standard comparison in `.local/anomaly/avx-handoff/detour-prefixes.json`.

| Entry | RVA | Instruction lengths | RIP targets |
| --- | --- | --- | --- |
| IRenderDeviceRender::Begin | 0x81a930 | 2+4 | - |
| IRenderDeviceRender::End | 0x81ab40 | 4+7 | 0xcd9904 |
| IRenderDeviceRender::SetCacheXform | 0x81aee0 | 2+4 | - |
| CRender::Render | 0x88d420 | 3+4 | - |
| CHUDManager::RenderUI | 0xf0810 | 2+4 | - |
| CRender::RenderToTarget | 0x88e290 | 2+4 | - |
| CRender::reset_begin | 0x8312d0 | 5 | - |
| CRender::reset_end | 0x831470 | 2+4 | - |
| CRender::model_Create | 0x830bf0 | 7 | - |
| CRender::model_CreateChild | 0x830c00 | 7 | - |
| CRender::model_Duplicate | 0x830e80 | 7 | - |
| CRender::model_Delete | 0x830df0 | 2+4 | - |
| Fvisual::Render | 0x8c5970 | 5 | - |
| FProgressive::Render | 0x8c6210 | 5 | - |
| CSkeletonX_ST::Render | 0x8c7010 | 4+3 | - |
| CSkeletonX_PM::Render | 0x8c6f20 | 4+7 | - |
| FTreeVisual_ST::Render | 0x8cb5e0 | 5 | - |
| FTreeVisual_PM::Render | 0x8cb2f0 | 5 | - |
| CBackend::constants.flush | 0x842490 | 5 | - |
| R_dsgraph_structure::r_dsgraph_render_hud | 0x846aa0 | 3+4 | - |
| R_dsgraph_structure::r_dsgraph_render_sorted | 0x8476a0 | 3+4 | - |
| R_dsgraph_structure::r_dsgraph_render_emissive | 0x8457e0 | 3+4 | - |
| CRender::rmNear | 0x831700 | 2+4 | - |
| CRender::rmNormal | 0x8317a0 | 2+4 | - |
| CDetailManager::Render | 0x860170 | 2+4 | - |
| CRenderTarget::phase_scene_prepare | 0x8d9160 | 2+4 | - |
| CRenderTarget::phase_combine | 0x8b2950 | 3+1+1 | - |
| CRenderTarget::phase_sunshafts | 0x8e5090 | 3+1+2 | - |
| CRenderTarget::phase_blur | 0x8e6e40 | 3+4 | - |
| CRender::rmFar | 0x831670 | 2+4 | - |
| R_xforms::set_P | 0x835fc0 | 5 | - |
| R_xforms::set_V | 0x836c60 | 5 | - |
| CTexture::surface_set | 0x870870 | 5 | - |
| dx10SamplerStateCache::SetMipLODBias | 0x7f7b80 | 2+7 | - |
| dx10SamplerStateCache::SetMaxAnisotropy | 0x7f7a70 | 2+7 | - |
| CBackend::Invalidate | 0x839f80 | 5 | - |
| CRenderTarget::u_setrt(W,H) | 0x8502c0 | 4+3 | - |
| PS::CParticleEffect::Render | 0x8cd840 | 2+7 | - |
| CRenderTarget::accum_point | 0x9021d0 | 3+1+1 | - |
| create_shader | 0x830080 | 5 | - |
| CRenderTarget::phase_scene_begin | 0x8d8b90 | 2+4 | - |
| CRenderTarget::phase_scene_end | 0x8d8c60 | 2+1+5 | - |

## Recorded check results

* `cmake --preset windows-x64-vs18 -B build/wk-avx`: passed.
* `cmake --build build/wk-avx --config Release`: the Anomaly and requested test targets built; the full build reported an unrelated LNK2019/LNK1120 in rsf_native_split_stub_test for rsf_fsr4vk_retire_commands, during concurrent Vulkan work. The coordinator explicitly excluded that target from this task's gate.
* `cmake --build build/wk-avx --config Release --target rsf_game_anomaly`: passed.
* `ctest --test-dir build/wk-avx -C Release -R "anomaly|thin|motion" --output-on-failure`: 92/92 passed, no skips, 96.36 seconds. These include synthetic profile/hook contracts and device fixtures, not a game run.
* `py -3 -I tests/anomaly_profile_evidence.py`: 32/32 passed, including all four scripts against each executable and 24 negative profile mutations.

Local logs: `.local/anomaly/avx-handoff/{configure,build,build-adapter,ctest,evidence-suite}.log`. `eng/verify.ps1` hardcodes the shared default build directory, so it was not run under the worker's explicit assigned-directory restriction; the assigned MSVC configure/build/tests above are the native evidence. No Rust files or versions changed.

The initial continuation runs exposed evidence-tool assumptions, not observed game failures: unselected aggregate parsing, SSE-only RIP/float matching, and standard-only helper lengths missed valid AVX instructions. Selecting one header section and decoding SSE/VEX operands fixed those checks. Both builds and representative negative mutations were rerun after the corrections. The older standard script remains the same semantic check for both inputs rather than accepting byte similarity as proof.

## Sun cascade jitter correction

The SR profile now appends render_sun_cascades as function index 15 and its sole CRender::Render call as call index 5,
keeping all earlier indices stable: initially 16 function guards and six call-site guards per build. The later
position binder below makes 17 function guards. The new AVX wrapper is
0x80dfd0, called at 0x88e0d6, looping over cascade fit 0x80be60. Standard equivalents are 0x815a30, 0x8984a8 and
0x813170. Both expected16 entries are `48895c240848896c2410488974241848`, extended to the same unique 30 bytes:
`48895c240848896c2410488974241848897c242041564883ec20488bd948`. Call expected16 is
`e8f5fef7ffeb15e85e00f8ff488bcee8` on AVX and `e883d5f7ffeb15e8ecd6f7ff488bcee8` on standard.

The five-byte relocated entry instruction is `mov [rsp+8],rbx` on both builds, with no RIP operand or AVX decoder issue.
The wrapper's source loop and reset-chain control identify it; the fit body reads all 64 projection bytes (two AVX ymm
loads versus four standard xmm loads) and restores RCache through set_V/set_P at its tail. Neither fit body directly reads
Device.mFullTransform. The adapter temporarily supplies the unjittered Device camera across the loop, then restores the
jittered pair and calls set_P to repair the cache. Full rationale, source revision, decoded evidence, counters and acceptance
limits are in the [sun cascade correction](anomaly-sr-seam.md#sun-cascade-camera-correction-12-november-2026).

The updated evidence suite covers eight originals and 28 negative mutations (36 checks); the additional mutations alter
the new extended guard and the call bytes in each build. The new test anomaly_sr_sun_cascades_contract covers the
matrix/cache lifetime, pass-through, rollback and quiesce paths. The previous profile bring-up results above are retained
as history, rather than presented as acceptance of this later correction.


Validation for this correction: MSVC Release configure and the full build in `build/wk-avx` passed; the requested
`ctest --test-dir build/wk-avx -C Release -R "anomaly|thin|motion" --output-on-failure` passed 93/93 without skips
(104.53 seconds). `py -3 -I tests/anomaly_profile_evidence.py` passed 36/36, including all four evidence scripts for
both executables and 28 negative mutations. `git diff --check` passed for the affected files. Logs and bounded binary
analysis are under `.local/anomaly/sun-cascade/` (`configure.log`, `build-all.log`, `ctest.log`, `evidence-suite.log`,
`sites.json`, `disassembly.txt`). The assigned-directory restriction prevents using the hardcoded shared build directory
in `eng/verify.ps1`; the explicit configure/full build/requested tests above are the native checks for this task.
No Rust behavior or public ABI changed. Visual acceptance remains pending.

## Deferred position binder

13 November 2026. The user rejected the preceding sun receiver-depth adjustment: it did not fix the flicker and distorted
character shadows. Its globals/registration/reader guards and option have been removed. The separately accepted
render_sun_cascades unjitter wrapper stays. The current SR function table appends cl_pos_decompress_params::setup at
index 16, preserving all existing indices: 17 functions and six call guards per executable.

The binder was found independently in each binary from the pos_decompression_params registration, its static binder
object's vptr, RTTI class identity and primary vtable slot 0. Reading the body confirms the FOV-derived xy, render-size zw,
then the six per-stage RCache.set_c expansions. AVX changes FOV arithmetic to VEX instructions, not the float4 ABI or
stage/constant-buffer layouts. The first relocated entry instructions are push rdi (2 bytes) and sub rsp,0x30 (4 bytes)
on both builds. The five-byte detour therefore relocates six integer bytes with no RIP operand and never reaches the
following SSE/AVX arithmetic instruction. No shader-hook stack offset or frame layout was changed.

| Exact-build site | Standard RVA | AVX RVA | Guard |
| --- | --- | --- | --- |
| cl_pos_decompress_params::setup | 0x834f70 | 0x82c1c0 | expected16 below, plus complete 0x2dc / 0x2d9-byte body |
| RTTI primary vtable, slot 0 | 0xb665f8 | 0xb53768 | File slot points to setup; registered object vptr independently matches |
| R_constants::GetCBuffer | 0x84b820 | 0x842380 | Complete successful stage branches, 0xd2 bytes per build |
| pos_decompression_params registration | 0x838b4f | 0x82fdf0 | Complete 22-byte argument/call sequence below |
| Reflected buffer-size constructor fragment | 0x83db90 | 0x834e80 | 0x78 bytes through GetDesc and Size store |

Setup expected16 is `40574883ec30f30f1005d6285000488b` on standard and
`40574883ec30c5fa100586964f00c5fa` on AVX. Registration bytes are
`4c8d05ca324b00488bcb488d15c0e43200e83bedffff` and
`4c8d0529a04a00488bcb488d158f433200e8eaecffff` respectively. All four full byte sequences per build are
literal reconstruction_guards in [sr_profile.h](../../games/anomaly/src/sr_profile.h), checked byte-for-byte at temporal
stage install and checked unique in .text by the evidence script. They are not wildcards or scan signatures.

The binder uses destination +0x10; stage load pairs (u16 byte offset, u16 class) at +0x14+4*stage; float4 class 1.
PS/VS/GS/HS/DS/CS stage bits are 1/2/8/16/32/64. The original GetCBuffer receives backend+0x5c0, reads destination
nibbles at shifts 16/12/8/20/24/28 and uses backend arrays +0x328/+0x2b8/+0x398/+0x408/+0x478/+0x4e8. The constructor
fragment proves reflected byte size +0x58 from D3D11_SHADER_BUFFER_DESC.Size, and every binder stage proves CPU data
+0x60 / dirty byte +0x68. Existing geometry evidence proves Flush uploads the same data and clears dirty (standard
0x83de40, AVX 0x835130). Each of these layouts is read from code in each executable, not inferred from source similarity.

The original binder runs first. While saved raster jitter is active, the adapter corrects each enabled stage's staging
float4 with xy += jitter_pixels * zw, preserves zw and marks the buffer dirty. It preflights every destination and handles
aliased storage once. Scope, SR-off, jitter-off and post-window calls retain the original stock result. Sun accumulation
inside the temporary unjittered shadow-fit scope still requires the saved raster jitter. See the
[derivation, lifetime and consumer audit](anomaly-sr-seam.md#jittered-deferred-position-reconstruction-13-november-2026).
The SR report adds position reconstruction corrected/stock/refused counters; corrected must rise in jittered world frames
and refused must remain zero. Binder calls are counted, not stages or frames.

No required owner for this correction is unmapped. The source/corpus consumer table explicitly lists optional effects
and post-seam/culling approximations that lack new game acceptance; they are not extra native mappings. Both profile
hook fixtures reject altered reconstruction guards; the static evidence suite also mutates both owner RVAs and all four
new code guards. Numeric and staging fixtures cover projection round trip, six stages, aliasing, rejection and rollback.
Bounded analysis and logs are in .local/anomaly/position-reconstruction. No game launch or install write was performed.
Visual acceptance of the corrected binder remains pending; the first run must check hook installation without refusal,
position corrected rising/refused zero and balanced sun unjittered/restored counters, with the console experiment's sun
scales restored to normal by the tester.


Validation (13 November 2026): MSVC Release configure and full build in `build/wk-avx` passed; the requested
`ctest --test-dir build/wk-avx -C Release -R "anomaly|thin|motion|shader_patch" --output-on-failure` passed 96/96,
without skips, in 92.78 seconds. `py -3 -I tests/anomaly_profile_evidence.py` passed 48/48: all four evidence scripts
against both hashes, with 40 negative mutations rejected. Package unittests passed 10/10 and `git diff --check` passed.
The first CTest run passed 94/96 but failed FSR3 activation in two FG fixtures: its cached vendor root held AMD DLL
SHA-256 `02297bee...`, while the current SDK/header pin uses `e6843252...`. Pointing only the assigned build's
`RSF_ANOMALY_SR_VENDOR_ROOT` at local fixture junctions (unchanged Streamline/XeSS and the current pinned FidelityFX
signedbin) fixed both failures; the entire requested selection then passed. This is a test-input correction, not a game
or Runtime change. Reproduce with `-DRSF_ANOMALY_SR_VENDOR_ROOT=D:/Coding/ReScaleFrame/.local/anomaly/position-reconstruction/vendors`.
Logs: `.local/anomaly/position-reconstruction/{configure,build-full,configure-pinned-vendors,build-final,ctest,
ctest-stale-vendors,evidence-suite,package-tests,fg-pinned-probe}.log`. The explicit assigned-directory build/tests
replace `eng/verify.ps1`, which hardcodes the prohibited shared build directory. No Rust behavior or public ABI changed.

## Input facts and latency boundary

13 November 2026, source revision 028ccf5f5189b393396480076b11e40852ba3ecd, implementation baseline 43619d9.
CInput was located through its RTTI primary pureFrame table, then every instruction in OnFrame was read on each build.
The callback tests pKeyboard at +0x28, calls KeyUpdate, then tests pMouse at +0x20 and calls MouseUpdate; its other three
direct calls are statistics/timing. The two update bodies issue GetDeviceData through DirectInput slot 10 (+0x50), with
24-byte DIDEVICEOBJECTDATA and an initial 64-entry buffer, including reacquire/retry. No XInput polling is present in this
stock callback; the matched source implementation is commented out. Third-party controller mods are outside this mapping.

| Site | Standard RVA / expected16 | AVX RVA / expected16 |
| --- | --- | --- |
| CInput::OnFrame | 0x67810 / 48895c2408574883ec20833d0f81cd00 | 0x66540 / 48895c2408574883ec20833ddf73cc00 |
| CInput::KeyUpdate | 0x66db0 / 4c8bdc534881ec40060000833d7a10cd | 0x65ae0 / 4c8bdc534881ec40060000833d4a03cc |
| CInput::MouseUpdate | 0x67020 / 4c8bdc534881ec80060000488bd941c7 | 0x65d50 / 4c8bdc534881ec80060000488bd941c7 |
| CInput primary vtable, slot 0 | 0xa55a20 -> 0x67810 | 0xa42a60 -> 0x66540 |

All three expected16 guards are unique in each executable's .text. input_profile.h also carries the complete unique
0xc3-byte OnFrame guard, checked before replacing the slot. The pointer must still hold the exact profiled function.
Both actual executables were mapped read-only with DONT_RESOLVE_DLL_REFERENCES; the production installer swapped,
restored, re-enabled and restored their slots successfully without executing game code. No new detour decoder or stack
layout assumption is required. Synthetic install fixtures reject all three altered entries, a changed final body byte
and an altered slot target on both profiles. FG evidence independently reconstructs RTTI, field offsets, call order and
buffered API arguments; its negative suite also alters all new entries, full body, table RVA and both field offsets.

Constructor references establish callback registration rather than relying only on RTTI similarity. Standard
CInput::CInput at 0x668c0 loads its primary table at 0x668df, then registers primary this with Device.seqFrame at
0xd379c8: mov r8d,0x33333333 at 0x66a99, mov rdx,rbx at 0x66a9f, call CRegistrator::Add at 0x66aa2 -> 0x2c2d0.
AVX constructor 0x655f0 loads the table at 0x6560f and registers with seqFrame 0xd259c8 using the same high priority
at 0x657cd, primary this at 0x657d3, and Add at 0x657d6 -> 0x2ca40. Both seqFrame globals are Device.dwFrame +0x348.
The matched source registers CInput at REG_PRIORITY_HIGH, before the ordinary frame update callbacks. The wrapper
calls the core synchronously before invoking the original OnFrame, so pacing returns before either DirectInput read.

The engine callback reports facts; the core owns pacing, marker mapping, registered-ping routing, tokens and per-source
deduplication. See [the corrected timeline](anomaly-fg.md#frame-timeline-pacing-and-latency-markers) and
[the event contract](input-keyed-latency.md). Counters to check in game are Latency window inputs_keyboard/mouse,
pcl_ping with a tool, zero marker_order_violations, and the Reflex driver report's matching input counters. Anti-Lag 2
is a shared Runtime mechanism with stub-driver coverage; AMD hardware and game acceptance remain unverified.
No required native input owner is unmapped for these stock executables. No game process or analysis database was opened.

Validation: both real-image slot probes passed, all 62 evidence checks passed (8 positives, 54 rejected mutations),
and the requested Release native selection passed 115 tests with five skips. Explicit plain/switch/borderless D3D11
and Reflex/PCL SDK fixtures passed; AMD hardware and Anomaly game acceptance remain pending. See the latency note
and .local/anomaly/input-latency/report.md for exact counts, skipped capabilities and game checks.
