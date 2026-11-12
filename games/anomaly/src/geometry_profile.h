// SPDX-License-Identifier: GPL-3.0-only
#pragma once
#include "native_profile.h"
namespace rsf::anomaly::geometry {
// STANDARD executable only. Local handoff records decompilation and RTTI slot evidence.
inline constexpr std::array<NativeSite, 10> geometry_sites{{
    {"CRender::model_Create", 0x839940, signature("488b8958070000e914e20400cccccccc")},
    {"CRender::model_CreateChild", 0x839950, signature("488b8958070000e974e40400cccccccc")},
    {"CRender::model_Duplicate", 0x839bd0, signature("488b8958070000e9d4ea0400cccccccc")},
    {"CRender::model_Delete", 0x839b40, signature("40534883ec20488b02488bda488b8958")},
    {"Fvisual::Render", 0x8cff60, signature("48895c2410555657415641574883ec30")},
    {"FProgressive::Render", 0x8d0800, signature("48895c241048896c2420565741544156")},
    {"CSkeletonX_ST::Render", 0x8d1610, signature("4883ec384c8bc1488d51684881c1a000")},
    {"CSkeletonX_PM::Render", 0x8d1520, signature("4883ec38448b89c80000000f57c00f2f")},
    {"FTreeVisual_ST::Render", 0x8d5d20, signature("48895c2410555657415641574883ec30")},
    {"FTreeVisual_PM::Render", 0x8d5a40, signature("48895c241848896c2420565741544156")}
}};
// These are layout evidence guards, not detours. Refuse the native layout reader on mismatch.
inline constexpr std::array<NativeSite, 3> geometry_layout_sites{{
    {"R_xforms::set_W", 0x840960, signature("48895c240848896c2410488974241857")},
    {"CKinematics::LL_GetBoneInstance", 0x803d90, signature("0fb7c24869c0a800000048034150c3cc")},
    {"CKinematics::LL_GetTransform_R", 0x803da0, signature("4883ec28488b01ff50484883c0404883")}
}};
// R_xforms instance behind RCache.xforms; set_W copies the 16 floats to its first member (m_w).
// 32 of 44 set_W callers pass this address (tests/anomaly_geometry_evidence.py --verify).
inline constexpr std::uint32_t world_matrix_rva = 0xd63c18;
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
