// SPDX-License-Identifier: GPL-3.0-only
#pragma once
#include "native_profile.h"
namespace rsf::anomaly::geometry {
// One table per exact executable (native_profile.h); build_profile.h selects it. Local handoff records decompilation and
// RTTI slot evidence; tests/anomaly_geometry_evidence.py --verify [--exe avx] re-derives every value per build.
// Indices 0..9 are the lifecycle and visual-entry detours. 10..15 are the draw-time call site and the
// view/projection context boundaries (see the "Context and draw-time sites" notes below). 16 is the detail (grass)
// render window.
//   CDetailManager::Render (DetailManager.cpp:401): `if (0 == dtFS) return; if (!rsDetails) return;` then hw_Render()
//   (the DX10 batch path). Every grass draw, in the G-buffer and in the shadow passes, happens inside this call.
inline constexpr std::size_t geometry_site_count = 17, geometry_layout_site_count = 7;
inline constexpr std::size_t geometry_site_lifecycle_count = 4;
inline constexpr std::size_t geometry_site_visual_count = 6;
inline constexpr std::size_t geometry_site_flush = 10, geometry_site_render_hud = 11, geometry_site_render_sorted = 12,
    geometry_site_render_emissive = 13, geometry_site_rm_near = 14, geometry_site_rm_normal = 15,
    geometry_site_detail_render = 16;
// Layout sites are evidence guards, not detours. Refuse the native layout reader on mismatch:
//   R_xforms::set_W, CKinematics::LL_GetBoneInstance / LL_GetTransform_R (bone stride and render transform),
//   Fvisual::Render phase compare (includes the rip-relative displacement to RImplementation.phase,
//   `cmp dword [phase], PHASE_SMAP`), CSecondVPParams::IsSVPFrame (isActive && dwFrame % frameDelay == 0),
//   dx10ConstantBuffer::Flush (`if (m_bChanged) { UpdateSubresource(m_pBuffer, 0, 0, m_pBufferData, 0, 0); m_bChanged = 0; }`:
//   m_pBuffer +0x50, m_pBufferData +0x60, m_bChanged +0x68), FTreeVisual_PM last_lod load (`last_lod` +0x108 reloaded right
//   after FTreeVisual::Render).
// Context and draw-time sites:
// * CBackend::constants.flush runs once per indexed/non-indexed draw (and compute dispatch), after the SRV, RT/ZB,
//   vertex-layout and state-manager applies and immediately before DrawIndexed, from every inlined
//   CBackend::Render site. The inlined D3D call is not hookable, so this is the last engine-owned call before it.
// * r_dsgraph_render_hud draws mapHUD under the HUD projection (psHUD_FOV). r_dsgraph_render_sorted and
//   r_dsgraph_render_emissive draw their world half and then, after rmNear(), their HUD half. rmNear/rmNormal
//   are CRender vtable slots 55 and 57 (viewport depth 0..0.02 and 0..1).
// Globals:
// * render_phase_rva: CRender::phase (u32); PHASE_NORMAL=0, PHASE_SMAP=1.
// * second_viewport_rva: CSecondVPParams: bool isActive, u8 frameDelay, bool isCamReady.
// * world_matrix_rva: R_xforms instance behind RCache.xforms; set_W copies the 16 floats to its first member (m_w). The
//   address most set_W callers pass (the evidence script counts them).
// * vertex_constant_buffers_rva: foliage constants. The engine stages every shader constant in a CPU copy
//   (dx10ConstantBuffer::m_pBufferData) and uploads it inside CBackend::constants.flush, so after the flush hook's original
//   returns the copy is exactly what the next draw reads. CBackend::m_aVertexConstants[14] (ref_cbuffer = one pointer to the
//   dx10ConstantBuffer) is the first of the six per-stage tables the flush loop walks (VS, PS, GS, HS, DS, CS, 0x70 bytes
//   apart; flush body `lea rbx` base minus 0x70). The table index is the shader reflection's constant-buffer index.
// * hierarchy_vtables: primary vtables of FHierrarhyVisual (engine spelling), CKinematics, CKinematicsAnimated and FLOD.
//   All four share the FHierrarhyVisual child vector (+96/+104/+112) and BOOL bDontDelete (+120). RTTI re-checked.
struct GeometryProfile {
    std::array<NativeSite, geometry_site_count> geometry_sites;
    std::array<NativeSite, geometry_layout_site_count> geometry_layout_sites;
    std::span<const UniqueGuard> geometry_unique_guards;
    uint32_t render_phase_rva;
    uint32_t second_viewport_rva;
    uint32_t world_matrix_rva;
    uint32_t vertex_constant_buffers_rva;
    std::array<uint32_t, 4> hierarchy_vtables;
};

// Layout shared by both builds (same source; each value is re-derived from each build's code by the evidence script).
inline constexpr std::uint32_t phase_normal = 0, phase_smap = 1;
inline constexpr std::uint32_t second_viewport_delay_offset = 1;
inline constexpr std::uint32_t constant_buffer_slots = 14;
inline constexpr std::uint32_t constant_buffer_size_offset = 0x58;   // u32 m_uiBufferSize (the reflection size)
inline constexpr std::uint32_t constant_buffer_data_offset = 0x60;   // void* m_pBufferData
// FTreeVisual_PM: FSlideWindowItem* pSWI at +0x100, u32 last_lod at +0x108 (the slide window the draw uses).
inline constexpr std::uint32_t tree_pm_last_lod_offset = 0x108;
// Detail rows live in `float4 array[61*4]`: four rows per instance, row 3 is (sun, sun, sun, hemi).
inline constexpr std::uint32_t detail_batch_instances = 61;
inline constexpr std::uint32_t skeleton_st_offset = 160;
inline constexpr std::uint32_t skeleton_pm_offset = 208;
inline constexpr std::uint32_t hierarchy_children_offset = 96;
inline constexpr std::uint32_t bone_instances_offset = 208; // CKinematics+128 IKinematics +0x50.
inline constexpr std::uint32_t bone_instance_stride = 168;
inline constexpr std::uint32_t render_transform_offset = 64;
inline constexpr std::uint64_t sbones_palette_layout = 0x53424f4e45530001ULL; // Verified three float4 columns per bone.

// ---- Standard 1.5.3 DX11 (AnomalyDX11.exe) -------------------------------------------------------------------------
inline constexpr std::array<UniqueGuard, 9> geometry_standard_unique{{
    {"Fvisual::Render", 0x8cff60, "48895c2410555657415641574883ec30488bb9"},
    {"FProgressive::Render", 0x8d0800, "48895c241048896c242056574154415641574883ec30"},
    {"FTreeVisual_ST::Render", 0x8d5d20, "48895c2410555657415641574883ec30488bd9"},
    {"FTreeVisual_PM::Render", 0x8d5a40, "48895c241848896c242056574154415641574883ec40"},
    {"R_dsgraph_structure::r_dsgraph_render_sorted", 0x850cc0, "488bc44889580848897010574881ec1001000083b9e8"},
    {"R_dsgraph_structure::r_dsgraph_render_emissive", 0x84ec80, "488bc44889580848897010574881ec1001000083b958"},
    {"CRender::rmNear", 0x83a450, "40534883ec40488b01ff507848c744242000000000488bc8488bd8488b10ff52508bc80f57c0f3480f2ac1488bcbf30f11442428488b13ff5258488b0d47"},
    {"CRender::rmNormal", 0x83a4d0, "40534883ec40488b01ff507848c744242000000000488bc8488bd8488b10ff52508bc80f57c0f3480f2ac1488bcbf30f11442428488b13ff5258488b0dc7"},
    {"R_xforms::set_W", 0x840960, "48895c240848896c24104889742418574883ec208b0248"},
}};
inline constexpr GeometryProfile geometry_standard{
    .geometry_sites = {{
        {"CRender::model_Create", 0x839940, signature("488b8958070000e914e20400cccccccc")},
        {"CRender::model_CreateChild", 0x839950, signature("488b8958070000e974e40400cccccccc")},
        {"CRender::model_Duplicate", 0x839bd0, signature("488b8958070000e9d4ea0400cccccccc")},
        {"CRender::model_Delete", 0x839b40, signature("40534883ec20488b02488bda488b8958")},
        {"Fvisual::Render", 0x8cff60, signature("48895c2410555657415641574883ec30")},
        {"FProgressive::Render", 0x8d0800, signature("48895c241048896c2420565741544156")},
        {"CSkeletonX_ST::Render", 0x8d1610, signature("4883ec384c8bc1488d51684881c1a000")},
        {"CSkeletonX_PM::Render", 0x8d1520, signature("4883ec38448b89c80000000f57c00f2f")},
        {"FTreeVisual_ST::Render", 0x8d5d20, signature("48895c2410555657415641574883ec30")},
        {"FTreeVisual_PM::Render", 0x8d5a40, signature("48895c241848896c2420565741544156")},
        {"CBackend::constants.flush", 0x84b930, signature("48895c2408574883ec20488d1da78551")},
        {"R_dsgraph_structure::r_dsgraph_render_hud", 0x84ffa0, signature("488bc448897010574881ec500100000f")},
        {"R_dsgraph_structure::r_dsgraph_render_sorted", 0x850cc0, signature("488bc44889580848897010574881ec10")},
        {"R_dsgraph_structure::r_dsgraph_render_emissive", 0x84ec80, signature("488bc44889580848897010574881ec10")},
        {"CRender::rmNear", 0x83a450, signature("40534883ec40488b01ff507848c74424")},
        {"CRender::rmNormal", 0x83a4d0, signature("40534883ec40488b01ff507848c74424")},
        {"CDetailManager::Render", 0x86a8c0, signature("40534883ec204883b95804000000488b")},
    }},
    .geometry_layout_sites = {{
        {"R_xforms::set_W", 0x840960, signature("48895c240848896c2410488974241857")},
        {"CKinematics::LL_GetBoneInstance", 0x803d90, signature("0fb7c24869c0a800000048034150c3cc")},
        {"CKinematics::LL_GetTransform_R", 0x803da0, signature("4883ec28488b01ff50484883c0404883")},
        {"Fvisual::Render phase compare", 0x8cff83, signature("833d06504900010f850b020000813d0a")},
        {"CSecondVPParams::IsSVPFrame", 0x2d260, signature("80390074150fb6490133d28b050fa4d0")},
        {"dx10ConstantBuffer::Flush", 0x83de40, signature("40534883ec4080796800488bd9743248")},
        {"FTreeVisual_PM last_lod load", 0x8d5a66, signature("448b8b080100000f57c00f2ff00f8291")},
    }},
    .geometry_unique_guards = geometry_standard_unique,
    .render_phase_rva = 0xd64f90,
    .second_viewport_rva = 0xd37b30,
    .world_matrix_rva = 0xd63c18,
    .vertex_constant_buffers_rva = 0xd63e78,
    .hierarchy_vtables = {0xb67d20, 0xb64ce8, 0xb71548, 0xb6c460},
};

// ---- AVX 1.5.3 DX11 (AnomalyDX11AVX.exe) ---------------------------------------------------------------------------
// CSkeletonX_PM::Render, render_hud/sorted/emissive, rmNear/rmNormal and the FTreeVisual_PM last_lod load contain VEX
// instructions in their guard bytes; every detour's first instructions (the bytes MinHook relocates) are integer code.
inline constexpr std::array<UniqueGuard, 9> geometry_avx_unique{{
    {"Fvisual::Render", 0x8c5970, "48895c2410555657415641574883ec30488bb9"},
    {"FProgressive::Render", 0x8c6210, "48895c241048896c242056574154415641574883ec30"},
    {"FTreeVisual_ST::Render", 0x8cb5e0, "48895c2410555657415641574883ec30488bd9"},
    {"FTreeVisual_PM::Render", 0x8cb2f0, "48895c241848896c242056574154415641574883ec40"},
    {"R_dsgraph_structure::r_dsgraph_render_sorted", 0x8476a0, "488bc44889580848897010574881ec4001000083b9e8"},
    {"R_dsgraph_structure::r_dsgraph_render_emissive", 0x8457e0, "488bc44889580848897010574881ec4001000083b958"},
    {"CRender::rmNear", 0x831700, "40534883ec50488b01c5f829742440ff5078c5c857f6c5fa11742420c5fa11742424488b10488bc8488bd8ff52508bc8c5f857c0c4e1fa2ac1c5fa11442428488b13488bcbff5258488b0d89"},
    {"CRender::rmNormal", 0x8317a0, "40534883ec50488b01c5f829742440ff5078c5c857f6c5fa11742420c5fa11742424488b10488bc8488bd8ff52508bc8c5f857c0c4e1fa2ac1c5fa11442428488b13488bcbff5258488b0de9"},
    {"R_xforms::set_W", 0x837c50, "48895c240848896c24104889742418574883ec208b0248"},
}};
inline constexpr GeometryProfile geometry_avx{
    .geometry_sites = {{
        {"CRender::model_Create", 0x830bf0, signature("488b8958070000e944ce0400cccccccc")},
        {"CRender::model_CreateChild", 0x830c00, signature("488b8958070000e9a4d00400cccccccc")},
        {"CRender::model_Duplicate", 0x830e80, signature("488b8958070000e904d70400cccccccc")},
        {"CRender::model_Delete", 0x830df0, signature("40534883ec20488b02488bda488b8958")},
        {"Fvisual::Render", 0x8c5970, signature("48895c2410555657415641574883ec30")},
        {"FProgressive::Render", 0x8c6210, signature("48895c241048896c2420565741544156")},
        {"CSkeletonX_ST::Render", 0x8c7010, signature("4883ec384c8bc1488d51684881c1a000")},
        {"CSkeletonX_PM::Render", 0x8c6f20, signature("4883ec38448b89c80000004c8bd9c5f8")},
        {"FTreeVisual_ST::Render", 0x8cb5e0, signature("48895c2410555657415641574883ec30")},
        {"FTreeVisual_PM::Render", 0x8cb2f0, signature("48895c241848896c2420565741544156")},
        {"CBackend::constants.flush", 0x842490, signature("48895c2408574883ec20488d1d47fa50")},
        {"R_dsgraph_structure::r_dsgraph_render_hud", 0x846aa0, signature("488bc448897010574881ec80010000c5")},
        {"R_dsgraph_structure::r_dsgraph_render_sorted", 0x8476a0, signature("488bc44889580848897010574881ec40")},
        {"R_dsgraph_structure::r_dsgraph_render_emissive", 0x8457e0, signature("488bc44889580848897010574881ec40")},
        {"CRender::rmNear", 0x831700, signature("40534883ec50488b01c5f829742440ff")},
        {"CRender::rmNormal", 0x8317a0, signature("40534883ec50488b01c5f829742440ff")},
        {"CDetailManager::Render", 0x860170, signature("40534883ec204883b95804000000488b")},
    }},
    .geometry_layout_sites = {{
        {"R_xforms::set_W", 0x837c50, signature("48895c240848896c2410488974241857")},
        {"CKinematics::LL_GetBoneInstance", 0x7fd6a0, signature("0fb7c24869c0a800000048034150c3cc")},
        {"CKinematics::LL_GetTransform_R", 0x7fd6b0, signature("4883ec28488b01ff50484883c0404883")},
        {"Fvisual::Render phase compare", 0x8c5993, signature("833df6d54800010f850b020000813dfa")},
        {"CSecondVPParams::IsSVPFrame", 0x2d180, signature("80390074150fb6490133d28b05ef84cf")},
        {"dx10ConstantBuffer::Flush", 0x835130, signature("40534883ec4080796800488bd9743248")},
        {"FTreeVisual_PM last_lod load", 0x8cb318, signature("448b8b08010000c5f857c0c5f82ff00f")},
    }},
    .geometry_unique_guards = geometry_avx_unique,
    .render_phase_rva = 0xd52f90,
    .second_viewport_rva = 0xd25b30,
    .world_matrix_rva = 0xd51c18,
    .vertex_constant_buffers_rva = 0xd51e78,
    .hierarchy_vtables = {0xb54e90, 0xb51dd8, 0xb5e708, 0xb595d0},
};
}
