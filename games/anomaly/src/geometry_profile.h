// SPDX-License-Identifier: GPL-3.0-only
#pragma once
#include "native_profile.h"
namespace rsf::anomaly::geometry {
// STANDARD executable only. Local handoff records decompilation and RTTI slot evidence.
// Indices 0..9 are the lifecycle and visual-entry detours. 10..15 are the draw-time call site and the
// view/projection context boundaries (see the "Context and draw-time sites" block below). 16 is the detail (grass)
// render window.
inline constexpr std::array<NativeSite, 17> geometry_sites{{
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
    // void CDetailManager::Render() (DetailManager.cpp:401): `if (0 == dtFS) return; if (!rsDetails) return;` then
    // hw_Render() (0x8aa320, the DX10 batch path). Every grass draw, in the G-buffer and in the shadow passes, happens
    // inside this call.
    {"CDetailManager::Render", 0x86a8c0, signature("40534883ec204883b95804000000488b")}
}};
inline constexpr std::size_t geometry_site_lifecycle_count = 4;
inline constexpr std::size_t geometry_site_visual_count = 6;
inline constexpr std::size_t geometry_site_flush = 10, geometry_site_render_hud = 11, geometry_site_render_sorted = 12,
    geometry_site_render_emissive = 13, geometry_site_rm_near = 14, geometry_site_rm_normal = 15,
    geometry_site_detail_render = 16;
// These are layout evidence guards, not detours. Refuse the native layout reader on mismatch.
inline constexpr std::array<NativeSite, 7> geometry_layout_sites{{
    {"R_xforms::set_W", 0x840960, signature("48895c240848896c2410488974241857")},
    {"CKinematics::LL_GetBoneInstance", 0x803d90, signature("0fb7c24869c0a800000048034150c3cc")},
    {"CKinematics::LL_GetTransform_R", 0x803da0, signature("4883ec28488b01ff50484883c0404883")},
    // Includes the rip-relative displacement to RImplementation.phase (`cmp dword [phase], PHASE_SMAP`).
    {"Fvisual::Render phase compare", 0x8cff83, signature("833d06504900010f850b020000813d0a")},
    // CRenderDevice::CSecondVPParams::IsSVPFrame: isActive && dwFrame % frameDelay == 0.
    {"CSecondVPParams::IsSVPFrame", 0x2d260, signature("80390074150fb6490133d28b050fa4d0")},
    // dx10ConstantBuffer::Flush: `if (m_bChanged) { UpdateSubresource(m_pBuffer, 0, 0, m_pBufferData, 0, 0); m_bChanged = 0; }`.
    // The body fixes m_pBuffer +0x50, m_pBufferData +0x60 and m_bChanged +0x68 (evidence script re-derives them).
    {"dx10ConstantBuffer::Flush", 0x83de40, signature("40534883ec4080796800488bd9743248")},
    // FTreeVisual_PM::Render reloads `last_lod` (+0x108) right after FTreeVisual::Render.
    {"FTreeVisual_PM last_lod load", 0x8d5a66, signature("448b8b080100000f57c00f2ff00f8291")}
}};
// Context and draw-time sites (standard 1.5.3 DX11).
// * CBackend::constants.flush runs once per indexed/non-indexed draw (and compute dispatch), after the SRV, RT/ZB,
//   vertex-layout and state-manager applies and immediately before DrawIndexed, from all 128 inlined
//   CBackend::Render sites. The inlined D3D call is not hookable, so this is the last engine-owned call before it.
// * r_dsgraph_render_hud draws mapHUD under the HUD projection (psHUD_FOV). r_dsgraph_render_sorted and
//   r_dsgraph_render_emissive draw their world half and then, after rmNear(), their HUD half. rmNear/rmNormal
//   are CRender vtable slots 55 and 57 (viewport depth 0..0.02 and 0..1).
inline constexpr std::uint32_t render_phase_rva = 0xd64f90;      // CRender::phase (u32); PHASE_NORMAL=0, PHASE_SMAP=1.
inline constexpr std::uint32_t phase_normal = 0, phase_smap = 1;
inline constexpr std::uint32_t second_viewport_rva = 0xd37b30;   // CSecondVPParams: bool isActive, u8 frameDelay, bool isCamReady.
inline constexpr std::uint32_t second_viewport_delay_offset = 1;
// R_xforms instance behind RCache.xforms; set_W copies the 16 floats to its first member (m_w).
// 32 of 44 set_W callers pass this address (tests/anomaly_geometry_evidence.py --verify).
inline constexpr std::uint32_t world_matrix_rva = 0xd63c18;
// Foliage constants. The engine stages every shader constant in a CPU copy (dx10ConstantBuffer::m_pBufferData) and
// uploads it inside CBackend::constants.flush, so after the flush hook's original returns the copy is exactly what the
// next draw reads. CBackend::m_aVertexConstants[14] (ref_cbuffer = one pointer to the dx10ConstantBuffer) is the first of
// the six per-stage tables the flush loop walks (VS, PS, GS, HS, DS, CS, 0x70 bytes apart; flush body `lea rbx` base
// minus 0x70). The table index is the shader reflection's constant-buffer index (R_constant_table::parse).
inline constexpr std::uint32_t vertex_constant_buffers_rva = 0xd63e78;
inline constexpr std::uint32_t constant_buffer_slots = 14;
inline constexpr std::uint32_t constant_buffer_size_offset = 0x58;   // u32 m_uiBufferSize (the reflection size)
inline constexpr std::uint32_t constant_buffer_data_offset = 0x60;   // void* m_pBufferData
// FTreeVisual_PM: FSlideWindowItem* pSWI at +0x100, u32 last_lod at +0x108 (the slide window the draw uses).
inline constexpr std::uint32_t tree_pm_last_lod_offset = 0x108;
// Detail rows live in `float4 array[61*4]`: four rows per instance, row 3 is (sun, sun, sun, hemi).
inline constexpr std::uint32_t detail_batch_instances = 61;
// Primary vtables of FHierrarhyVisual (engine spelling), CKinematics, CKinematicsAnimated and FLOD.
// All four share the FHierrarhyVisual child vector (+96/+104/+112) and BOOL bDontDelete (+120).
// RTTI tables are re-checked by tests/anomaly_geometry_evidence.py --verify.
inline constexpr std::array<std::uint32_t, 4> hierarchy_vtables{0xb67d20, 0xb64ce8, 0xb71548, 0xb6c460};
inline constexpr std::uint32_t skeleton_st_offset = 160;
inline constexpr std::uint32_t skeleton_pm_offset = 208;
inline constexpr std::uint32_t hierarchy_children_offset = 96;
inline constexpr std::uint32_t bone_instances_offset = 208; // CKinematics+128 IKinematics +0x50.
inline constexpr std::uint32_t bone_instance_stride = 168;
inline constexpr std::uint32_t render_transform_offset = 64;
inline constexpr std::uint64_t sbones_palette_layout = 0x53424f4e45530001ULL; // Verified three float4 columns per bone.
}
