// SPDX-License-Identifier: GPL-3.0-only
#pragma once
#include "native_profile.h"
namespace rsf::anomaly::sr {
// Render-resolution split (SR Stage 0, docs/research/anomaly-sr-seam.md). One table per exact executable (native_profile.h,
// build_profile.h); the AVX table is documented in docs/research/anomaly-avx-profile.md.
// Data only: nothing here installs a hook or changes rendering. Every value is re-checked against the installed
// executable by tests/anomaly_sr_evidence.py --verify [--exe avx] (decompile/disassembly evidence in that script and the docs).
// All functions are x64 __fastcall with `this` in rcx. Line numbers refer to xray-monolith 028ccf5.

// Detour and call-target candidates (sr_function_sites, in this order). expected = the first 16 bytes at the RVA (same
// convention as native_sites).
//   render_sun_cascades: CRender wrapper looping over every sun cascade (r2_R_sun.cpp:1113). Temporarily uses the
//     saved unjittered main matrices for fitting; the caller restores Device and RCache before world rendering resumes.
//   phase_scene_prepare: void CRenderTarget::phase_scene_prepare() (r4_rendertarget_phase_scene.cpp:4). H1.
//   phase_combine: void CRenderTarget::phase_combine() (r4_rendertarget_phase_combine.cpp:39). Hosts the S-B seam calls.
//   phase_sunshafts: void CRenderTarget::phase_sunshafts() (rendertarget_phase_sunshafts.cpp:3). H2a.
//   phase_blur: void CRenderTarget::phase_blur() (rendertarget_phase_blur.cpp:3). H2b. 16 bytes are not unique: see unique guards.
//   rmNear / rmFar: void CRender::rmNear() / rmFar() (r4.cpp:834,843), CRender vtable slots 55 and 56. rmNear is H4 and is the
//     same function geometry_profile.h already detours (geometry_site_rm_near): H4 must share that detour.
//   set_P: void R_xforms::set_P(const Fmatrix&) (R_Backend_xform.cpp:32), rdx = matrix; copies it to this+0xC0 (m_p).
//   set_V: void R_xforms::set_V(const Fmatrix&) (R_Backend_xform.cpp:19); copies to this+0x80 (m_v).
//   surface_set: void CTexture::surface_set(ID3DBaseTexture*) (dx10SH_Texture.cpp:55): AddRef new, Release pSurface and
//     m_pSRView, store, refresh desc, CreateShaderResourceView (R24G8_TYPELESS -> R24_UNORM_X8_TYPELESS).
//   SetMipLODBias: void dx10SamplerStateCache::SetMipLODBias(float) (dx10SamplerStateCache.cpp:201), bias in xmm1. Rebuilds
//     every sampler state when the value changes.
//   SetMaxAnisotropy: void dx10SamplerStateCache::SetMaxAnisotropy(u32) (dx10SamplerStateCache.cpp:172), edx = value.
//   Invalidate: void CBackend::Invalidate() (R_Backend_Runtime.cpp:63). Clears every RCache cache, including the texture slots.
//   u_setrt(W,H): void CRenderTarget::u_setrt(u32 W, u32 H, RTV0, RTV1, RTV2, DSV) (r4_rendertarget.cpp:111).
//   CParticleEffect::Render, accum_point: first-person projection rebuild sites outside geometry_profile.h (see
//     sr_first_person_sites).
inline constexpr std::size_t sr_function_site_count = 16;
inline constexpr std::size_t site_phase_scene_prepare = 0, site_phase_combine = 1, site_phase_sunshafts = 2,
    site_phase_blur = 3, site_rm_near = 4, site_rm_far = 5, site_set_p = 6, site_set_v = 7, site_surface_set = 8,
    site_set_mip_lod_bias = 9, site_set_max_anisotropy = 10, site_backend_invalidate = 11, site_u_setrt_wh = 12, site_sun_cascades = 15;

// Where 16 bytes are not unique in .text: the shortest prefix at the same RVA that is (sr_unique_guards). rmNear/rmFar/
// rmNormal differ only in their rip-relative MaxDepth displacement near the end of the guard.
// Real `call rel32` instructions (sr_call_sites, no LTCG inlining). expected = 16 bytes from the E8 opcode.
inline constexpr std::size_t sr_call_site_count = 6;

// Every site that rebuilds a first-person projection, uploads it with R_xforms::set_P and then calls rmNear().
// rm_near_call is the virtual `call [rax+0x1b8]` (slot 55) or a direct call to CRender::rmNear.
struct FirstPersonSite {
    const char* name;
    uint32_t function_rva;
    uint32_t set_p_call;
    uint32_t rm_near_call;
};
inline constexpr std::size_t sr_first_person_site_count = 6;
inline constexpr uint32_t rm_near_slot = 55, rm_far_slot = 56, rm_normal_slot = 57;

// Per-build addresses. Globals are RVAs from image base 0x140000000; Device.dwWidth/dwHeight are native_profile.h
// device_width/height_rva.
//   device_view/project/full_transform_rva: Device.mView, mProject, mFullTransform (adjacent Fmatrix).
//   Loading screen. CRenderDevice::PreCache registers load_screen_renderer (b_registered = 1, b_need_user_input = the next
//   byte) and CGamePersistent::OnFrame stops it only when dwPrecacheFrame is 0 and the "Press any key to continue" sequence
//   is done, so the flag stays set through the key wait, when the world still renders under the loading screen and
//   dwPrecacheFrame is 0. device_precache_frame_rva = Device.dwPrecacheFrame (device_width_rva + 8);
//   load_screen_registered_rva = load_screen_renderer.b_registered (bool).
//   hw_*: HW.pDevice, HW.pContext, HW.m_pSwapChain, HW.pBaseRT, HW.pBaseZB.
//   render_implementation_rva: RImplementation (CRender). render_options_word0/3_rva: CRender::o, first and fourth u32 of
//   the bitfield. sunshafts_mode_rva: ps_sunshafts_mode; 2/3 run phase_sunshafts. sampler_cache_rva: SSManager
//   (dx10SamplerStateCache). user_mip_bias_rva: ps_r__tf_Mipbias (float). user_anisotropy_rva: ps_r__tf_Anisotropic (u32).
//   backend_rva: RCache (CBackend).
//   SSAO viewport helper (r4_rendertarget_phase_ssao.cpp:3-10): `static D3D_VIEWPORT viewport[1] = {0, 0, w, h, 0, 1}`
//   is a function-local static, so its width and height are fixed by the first call for the rest of the process and
//   later calls only re-apply it. Stock calls it from phase_downsamp (with o.ssao_half_data) and phase_ssao (with
//   o.ssao_blur_on and no ssao_opt_data), always first with Device.dwWidth/2 x Device.dwHeight/2. In a Stage 2 world
//   window the static must hold render/2, and output/2 again afterwards. Binary (x64 magic static):
//   `cmp [guard], tls_epoch; jg init` then `lea r8,[viewport]; jmp [ctx->vtbl+0x160]` (RSSetViewports); the init path
//   stores only Width (+8) and Height (+0xC); X, Y, MinDepth 0 and MaxDepth 1.0 are in .data. The guard is in .bss:
//   0 = never initialised, -1 = being initialised, anything else = initialised. Four callers in phase_downsamp and
//   phase_ssao, no other reference. ssao_set_viewport_code is the function from its entry up to and including
//   `lea r8,[rip+viewport]` (it ends with that displacement); the guard and viewport displacements in it prove both data
//   addresses.
struct SrProfile {
    std::array<NativeSite, sr_function_site_count> sr_function_sites;
    std::span<const UniqueGuard> sr_unique_guards;
    std::array<NativeSite, sr_call_site_count> sr_call_sites;
    std::array<FirstPersonSite, sr_first_person_site_count> sr_first_person_sites;
    uint32_t crender_vtable_rva;
    uint32_t device_view_rva, device_project_rva, device_full_transform_rva;
    uint32_t device_precache_frame_rva, load_screen_registered_rva;
    uint32_t hw_device_rva, hw_context_rva, hw_swap_chain_rva, hw_base_rt_rva, hw_base_zb_rva;
    uint32_t render_implementation_rva;
    uint32_t render_options_word0_rva, render_options_word3_rva;
    uint32_t sunshafts_mode_rva;
    uint32_t sampler_cache_rva, user_mip_bias_rva, user_anisotropy_rva;
    uint32_t ssao_set_viewport_rva, ssao_viewport_rva, ssao_viewport_guard_rva;
    const char* ssao_set_viewport_code;
    uint32_t backend_rva;
    uint32_t sun_depth_near_scale_rva, sun_depth_far_scale_rva;
    std::span<const UniqueGuard> sun_depth_guards;
};

// ---- Layout shared by both builds (same source; the evidence script re-derives each value from each build's code) ----
inline constexpr uint32_t render_target_offset = 0x768;          // CRender::Target (CRenderTarget*)
inline constexpr uint32_t mrtmixdepth_bit = 1u << 26, fp16_blend_bit = 1u << 28;
inline constexpr uint32_t advancedpp_bit = 1u << 12, dx10_msaa_bit = 1u << 14;
inline constexpr uint32_t sampler_mip_lod_bias_offset = 0x19c;   // m_uiMipLODBias (float)

// CRenderTarget (from the constructor's ref_rt::create(name, w, h, fmt, samples) calls).
inline constexpr uint32_t target_width_offset = 0x8, target_height_offset = 0xc;  // set by every u_setrt
struct TargetSlot {
    const char* name;      // CResourceManager name passed to ref_rt::create
    uint32_t offset;       // ref_rt (CRT*) inside CRenderTarget
    uint32_t d3d_format;   // D3DFORMAT passed at creation: 21 A8R8G8B8, 113 A16B16G16R16F, 75 D24S8, 0 = not checked
};
inline constexpr std::array<TargetSlot, 26> target_slots{{
    {"$user$position", 0x378, 113}, {"$user$msaadepth", 0x358, 75}, {"$user$albedo", 0x380, 21},
    {"$user$accum", 0x388, 113}, {"$user$accum_temp", 0x390, 113}, {"$user$generic0", 0x3a8, 21},
    {"$user$generic1", 0x3b0, 21}, {"$user$generic", 0x370, 21}, {"$user$generic_temp", 0x3b8, 21},
    {"$user$viewport2", 0x3c0, 21}, {"$user$dof", 0x3c8, 21}, {"$user$ui", 0x3d0, 21},
    {"$user$sun_shafts0", 0x398, 21}, {"$user$sun_shafts1", 0x3a0, 21}, {"$user$blur_h_2", 0x3d8, 21},
    {"$user$blur_2", 0x3e0, 21}, {"$user$blur_h_4", 0x3e8, 21}, {"$user$blur_4", 0x3f0, 21},
    {"$user$blur_h_8", 0x3f8, 21}, {"$user$blur_8", 0x400, 21}, {"$user$pp_bloom", 0x408, 21},
    {"$user$generic0_r", 0x360, 21}, {"$user$generic1_r", 0x368, 21}, {"$user$generic2", 0x420, 113},
    {"$user$half_depth", 0x590, 0}, {"$user$ssao_temp", 0x588, 0},
}};
// CTexture* t_LUM_src / t_LUM_dest, re-pointed with surface_set at the start of every phase_combine.
inline constexpr uint32_t target_lum_src_texture_offset = 0x4c8, target_lum_dest_texture_offset = 0x4d0;

// SR Stage 2 world targets: every CRenderTarget target the world window (phase_scene_prepare .. S-B seam) renders
// into, in the order of sr::Slot. Each entry repeats its target_slots offset and format (static_assert in
// sr_swap.cpp, and the verifier). d3d_format 0 accepts R16F (111) or R32F (114): rt_half_depth is R32F on NVIDIA and
// R16F elsewhere (r4_rendertarget.cpp:717), rt_ssao_temp is R16F with a UAV (749). Optional targets exist only with
// their option (generic2: o.advancedpp; half_depth: o.ssao_opt_data; ssao_temp: o.ssao_hdao && o.ssao_ultra).
// half_allowed: the engine may create it at Device.dwWidth/2 x Device.dwHeight/2 (o.ssao_half_data, line 709).
struct WorldTarget {
    const char* name;
    uint32_t offset;
    uint32_t d3d_format;
    bool optional;
    bool half_allowed;
};
inline constexpr uint32_t d3dfmt_r16f = 111, d3dfmt_r32f = 114;
inline constexpr std::array<WorldTarget, 9> sr_world_targets{{
    {"$user$generic0", 0x3a8, 21, false, false},       // S-B colour; FP16 twin
    {"$user$generic_temp", 0x3b8, 21, false, false},   // CopyResource partner of generic0; FP16 twin
    {"$user$position", 0x378, 113, false, false},      // G-buffer position/normal; P reads it (blur, dof, combine_2)
    {"$user$albedo", 0x380, 21, false, false},         // G-buffer albedo, then hud_ui scratch
    {"$user$accum", 0x388, 113, false, false},         // light accumulation
    {"$user$generic1", 0x3b0, 21, false, false},       // bloom source, then distortion mask; combine_2 reads it
    {"$user$generic2", 0x420, 113, true, false},       // volumetric light accumulation (advancedpp)
    {"$user$half_depth", 0x590, 0, true, true},        // SSAO downsampled depth (ssao_opt_data)
    {"$user$ssao_temp", 0x588, 0, true, false},        // HDAO output, UAV (ssao_hdao && ssao_ultra)
}};
// Targets the world window would also write on paths Stage 2 does not twin. Each must be absent (null ref_rt):
// msaadepth, generic0_r and generic1_r exist only with MSAA (refused); accum_temp only without mrtmixdepth and
// fp16_blend (r4_rendertarget.cpp:426), which D3D10+ never selects (HWCaps.cpp:242).
inline constexpr std::array<uint32_t, 4> sr_world_absent_offsets{0x358, 0x360, 0x368, 0x390};

inline constexpr uint32_t ssao_viewport_width_offset = 0x8, ssao_viewport_height_offset = 0xc;
// The CRender::o word0 bits phase_combine tests before calling the SSAO passes (test al,0x20 / 0x40 -> phase_hdao;
// test al,4 -> phase_downsamp; test al,2 -> phase_ssao). Documentation and verifier only.
inline constexpr uint32_t ssao_blur_on_bit = 1u << 1, ssao_opt_data_bit = 1u << 2, ssao_half_data_bit = 1u << 3,
    ssao_hdao_bit = 1u << 5, ssao_ultra_bit = 1u << 6;

// CRT (SH_RT.h:6-38; 0x50 bytes, allocated by CResourceManager::_CreateRT).
inline constexpr uint32_t crt_size = 0x50, crt_reference_offset = 0x0, crt_surface_offset = 0x10, crt_rtv_offset = 0x18,
    crt_dsv_offset = 0x20, crt_uav_offset = 0x28, crt_texture_offset = 0x30, crt_width_offset = 0x38,
    crt_height_offset = 0x3c, crt_format_offset = 0x40, crt_order_offset = 0x48;
// CTexture (SH_Texture.h; polymorphic, reference count after the vtable).
inline constexpr uint32_t texture_reference_offset = 0x8, texture_surface_offset = 0x50, texture_desc_cache_offset = 0x70,
    texture_desc_offset = 0x78, texture_srv_offset = 0xa8;
// RCache (CBackend, R_Backend.h:117-190). Offsets from CBackend::Invalidate and OnFrameBegin.
inline constexpr uint32_t backend_xforms_offset = 0x58;           // R_xforms (geometry_profile world_matrix_rva)
inline constexpr uint32_t backend_rt_offset = 0x578, backend_zb_offset = 0x598;  // pRT[4], pZB
inline constexpr uint32_t backend_ctable_offset = 0x5c8;           // R_constant_table* ctable
inline constexpr uint32_t backend_texture_list_offset = 0x640;     // STextureList* T
inline constexpr uint32_t backend_textures_ps_offset = 0x658, backend_textures_ps_count = 16;
inline constexpr uint32_t backend_textures_vs_offset = 0x6d8, backend_textures_vs_count = 4;
inline constexpr uint32_t backend_textures_gs_offset = 0x6f8, backend_textures_hs_offset = 0x778,
    backend_textures_ds_offset = 0x7f8, backend_textures_cs_offset = 0x878, backend_textures_stage_count = 16;
inline constexpr uint32_t backend_rt_changed_offset = 0x980;       // m_bChangedRTorZB
// R_xforms (r_backend_xform.h): m_w, m_invw, m_v, m_p, then derived matrices.
inline constexpr uint32_t xforms_view_offset = 0x80, xforms_project_offset = 0xc0;

// ---- Standard 1.5.3 DX11 (AnomalyDX11.exe) -------------------------------------------------------------------------
inline constexpr std::array<UniqueGuard, 7> sr_standard_unique{{
    {"CRender::render_sun_cascades", 0x815a30, "48895c240848896c2410488974241848897c242041564883ec20488bd948"},
    {"CRenderTarget::phase_blur", 0x8f2590, "48895c242055565741564157488bec4881"},
    {"CRender::rmNear", 0x83a450, "40534883ec40488b01ff507848c744242000000000488bc8488bd8488b10ff52508bc80f57c0f3480f2ac1488bcbf30f11442428488b13ff5258488b0d47"},
    {"CRender::rmFar", 0x83a3c0, "40534883ec40488b01ff507848c744242000000000488bc8488bd8488b10ff52508bc80f57c0f3480f2ac1488bcbf30f11442428488b13ff5258488b0dd7"},
    {"R_xforms::set_P", 0x83ecd0, "48895c240848896c2410488974241848897c242041564883ec208b02"},
    {"R_xforms::set_V", 0x83f970, "48895c240848896c2410488974241857415641574883ec208b02"},
    {"CBackend::Invalidate", 0x842c50, "48895c240848896c24104889742418574883ec2033ed488bd94889a9"},
}};
inline constexpr std::array<UniqueGuard, 3> sun_depth_standard_guards{{
    {"sun depth near console registration", 0x7f9569, "488d15f0a43600488d0d19855600e8943287ff488d05a91d4f004c892506855600488d0d1f02250048890528855600c705268556000000003fc705208556000000c03f"},
    {"sun depth far console registration", 0x7f943d, "488d15eca53600488d0da5855600e8c03387ff488d05cd1e4f004c892592855600488d0dcb022500488905b4855600c705b28556000000003fc705ac8556000000c03f"},
    {"sun depth cascade selection", 0x85e69c, "4585f6750bf3440f100d82cc4800eb09f3440f100d6fcc4800f3440f114db0"},
}};
inline constexpr SrProfile sr_standard{
    .sr_function_sites = {{
        {"CRenderTarget::phase_scene_prepare", 0x8e4310, signature("40534883ec50f7058c0f480000400000")},
        {"CRenderTarget::phase_combine", 0x8bc980, signature("488bc45556415441554156488da888fe")},
        {"CRenderTarget::phase_sunshafts", 0x8f07c0, signature("4c8bdc554156498d6ba14881ece80000")},
        {"CRenderTarget::phase_blur", 0x8f2590, signature("48895c242055565741564157488bec48")},
        {"CRender::rmNear", 0x83a450, signature("40534883ec40488b01ff507848c74424")},
        {"CRender::rmFar", 0x83a3c0, signature("40534883ec40488b01ff507848c74424")},
        {"R_xforms::set_P", 0x83ecd0, signature("48895c240848896c2410488974241848")},
        {"R_xforms::set_V", 0x83f970, signature("48895c240848896c2410488974241857")},
        {"CTexture::surface_set", 0x87ac30, signature("48895c24185556574883ec40488bfa48")},
        {"dx10SamplerStateCache::SetMipLODBias", 0x7fe170, signature("40574881eca0000000f30f10819c0100")},
        {"dx10SamplerStateCache::SetMaxAnisotropy", 0x7fe060, signature("40574881eca0000000488bf983fa0173")},
        {"CBackend::Invalidate", 0x842c50, signature("48895c240848896c2410488974241857")},
        {"CRenderTarget::u_setrt(W,H)", 0x859cb0, signature("4883ec288951084489410c4c3b0d76a4")},
        {"PS::CParticleEffect::Render", 0x8d8420, signature("40534881ec80010000488bd9e87fa40f")},
        {"CRenderTarget::accum_point", 0x90dca0, signature("488bc45553488da8d8feffff4881ec18")},
        {"CRender::render_sun_cascades", 0x815a30, signature("48895c240848896c2410488974241848")},
    }},
    .sr_unique_guards = sr_standard_unique,
    .sr_call_sites = {{
        {"CRender::Render -> phase_scene_prepare (z-prepass, r4_R_render.cpp:278)", 0x897d2c, signature("e8dfc5040033d2488d0d76414500e861")},
        {"CRender::Render -> phase_scene_prepare (r4_R_render.cpp:285)", 0x897d66, signature("e8a5c50400488b1d76fd4900833db77b")},
        {"CRender::Render -> phase_combine (r4_R_render.cpp:538)", 0x8985bf, signature("e8bc430200eb39488b05233345004889")},
        {"phase_combine -> phase_sunshafts (phase_combine.cpp:374)", 0x8bf273, signature("e848150300498bcee810330300498bce")},
        {"phase_combine -> phase_blur (phase_combine.cpp:378)", 0x8bf27b, signature("e810330300498bcee858550300f70502")},
        {"CRender::Render -> render_sun_cascades (r4_R_render.cpp:492)", 0x8984a8, signature("e883d5f7ffeb15e8ecd6f7ff488bcee8")},
    }},
    .sr_first_person_sites = {{
        {"r_dsgraph_render_hud (r__dsgraph_render.cpp:502-520)", 0x84ffa0, 0x850410, 0x85041b},
        {"r_dsgraph_render_hud_ui (r__dsgraph_render.cpp:538-583)", 0x8505a0, 0x8509b5, 0x850ab4},
        {"r_dsgraph_render_sorted (r__dsgraph_render.cpp:595-613)", 0x850cc0, 0x851135, 0x851140},
        {"r_dsgraph_render_emissive (r__dsgraph_render.cpp:626-645)", 0x84ec80, 0x84f0f5, 0x84f100},
        {"CParticleEffect::Render HUD mode (ParticleEffect.cpp:599-611)", 0x8d8420, 0x8d8a8e, 0x8d8a9a},
        {"CRenderTarget::accum_point HUD light (r4_rendertarget_accum_point.cpp:19-31)", 0x90dca0, 0x90e16c, 0x90e178},
    }},
    .crender_vtable_rva = 0xb669f8,
    .device_view_rva = 0xd376c8,
    .device_project_rva = 0xd37708,
    .device_full_transform_rva = 0xd37748,
    .device_precache_frame_rva = 0xd37670,
    .load_screen_registered_rva = 0xce41b0,
    .hw_device_rva = 0xceb8d0,
    .hw_context_rva = 0xceb8d8,
    .hw_swap_chain_rva = 0xceb8e0,
    .hw_base_rt_rva = 0xceb8e8,
    .hw_base_zb_rva = 0xceb8f0,
    .render_implementation_rva = 0xd64e40,
    .render_options_word0_rva = 0xd652a0,
    .render_options_word3_rva = 0xd652ac,
    .sunshafts_mode_rva = 0xceb36c,
    .sampler_cache_rva = 0xd63a20,
    .user_mip_bias_rva = 0xd607a0,
    .user_anisotropy_rva = 0xceb878,
    .ssao_set_viewport_rva = 0x8f95f0,
    .ssao_viewport_rva = 0xceebd0,
    .ssao_viewport_guard_rva = 0xd79284,
    .ssao_set_viewport_code =
        "40534883ec408b0514704600488bd965488b1425580000000f297424300f28f2b9100002000f297c24200f28f9488b14c28b041139055afc47007f28488b034c8d059a553f00",
    .backend_rva = 0xd63bc0,
    .sun_depth_near_scale_rva = 0xceb32c,
    .sun_depth_far_scale_rva = 0xceb324,
    .sun_depth_guards = sun_depth_standard_guards,
};

// ---- AVX 1.5.3 DX11 (AnomalyDX11AVX.exe) ---------------------------------------------------------------------------
// rmFar's 16 bytes are unique in this build, so it has no extended guard. The SSAO helper is re-encoded with VEX moves
// (0x4a bytes up to the viewport lea instead of 0x46).
inline constexpr std::array<UniqueGuard, 6> sr_avx_unique{{
    {"CRender::render_sun_cascades", 0x80dfd0, "48895c240848896c2410488974241848897c242041564883ec20488bd948"},
    {"CRenderTarget::phase_blur", 0x8e6e40, "488bc44889582055565741564157488d68a14881eca0"},
    {"CRender::rmNear", 0x831700, "40534883ec50488b01c5f829742440ff5078c5c857f6c5fa11742420c5fa11742424488b10488bc8488bd8ff52508bc8c5f857c0c4e1fa2ac1c5fa11442428488b13488bcbff5258488b0d89"},
    {"R_xforms::set_P", 0x835fc0, "48895c240848896c2410488974241848897c242041564883ec208b02"},
    {"R_xforms::set_V", 0x836c60, "48895c240848896c2410488974241857415641574883ec208b02"},
    {"CBackend::Invalidate", 0x839f80, "48895c240848896c24104889742418574883ec2033ed488bd94889a9"},
}};
inline constexpr std::array<UniqueGuard, 5> sun_depth_avx_guards{{
    {"sun depth near console registration", 0x7f2f47, "488d1502dc3500488d0d3bcb5500e8368687ff488d05cb634e004c892d28cb5500488d0d313a24004889054acb5500c57a110d4acb5500c5fa113546cb5500"},
    {"sun depth far console registration", 0x7f2e11, "488d1508dd3500488d0dd1cb5500e86c8787ffc5fa103590dd2400488d05f1644e004c892db6cb5500488d0ddf3a2400488905d8cb5500c57a110dd8cb5500c5fa1135d4cb5500"},
    {"sun depth cascade selection", 0x85494b, "4585f6750ac57a1035d4494800eb08c57a1035c2494800c57a1175c0"},
    {"sun depth console minimum", 0x7f2dc7, "c57a100dcd922400488b0d7230530048"},
    {"sun depth console maximum", 0x7f2e6b, "c5fa103549dd2400488b0dce2f530048"},
}};
inline constexpr SrProfile sr_avx{
    .sr_function_sites = {{
        {"CRenderTarget::phase_scene_prepare", 0x8d9160, signature("40534883ec50f7053ca1470000400000")},
        {"CRenderTarget::phase_combine", 0x8b2950, signature("488bc45556415441554156488da888fe")},
        {"CRenderTarget::phase_sunshafts", 0x8e5090, signature("4c8bdc554156498d6ba14881ecf80000")},
        {"CRenderTarget::phase_blur", 0x8e6e40, signature("488bc44889582055565741564157488d")},
        {"CRender::rmNear", 0x831700, signature("40534883ec50488b01c5f829742440ff")},
        {"CRender::rmFar", 0x831670, signature("40534883ec40488b01ff5078c5f857c0")},
        {"R_xforms::set_P", 0x835fc0, signature("48895c240848896c2410488974241848")},
        {"R_xforms::set_V", 0x836c60, signature("48895c240848896c2410488974241857")},
        {"CTexture::surface_set", 0x870870, signature("48895c24185556574883ec40488bfa48")},
        {"dx10SamplerStateCache::SetMipLODBias", 0x7f7b80, signature("40574881eca0000000c5fa10819c0100")},
        {"dx10SamplerStateCache::SetMaxAnisotropy", 0x7f7a70, signature("40574881eca0000000488bf983fa0173")},
        {"CBackend::Invalidate", 0x839f80, signature("48895c240848896c2410488974241857")},
        {"CRenderTarget::u_setrt(W,H)", 0x8502c0, signature("4883ec288951084489410c4c3b0d661e")},
        {"PS::CParticleEffect::Render", 0x8cd840, signature("40534881ec80010000488bd9e87f470f")},
        {"CRenderTarget::accum_point", 0x9021d0, signature("488bc45553488da8c8feffff4881ec28")},
        {"CRender::render_sun_cascades", 0x80dfd0, signature("48895c240848896c2410488974241848")},
    }},
    .sr_unique_guards = sr_avx_unique,
    .sr_call_sites = {{
        {"CRender::Render -> phase_scene_prepare (z-prepass, r4_R_render.cpp:278)", 0x88d957, signature("e804b8040033d2488d0d4bc54400e866")},
        {"CRender::Render -> phase_scene_prepare (r4_R_render.cpp:285)", 0x88d991, signature("e8cab70400488b1d4b814900833d8cff")},
        {"CRender::Render -> phase_combine (r4_R_render.cpp:538)", 0x88e1ed, signature("e85e470200eb39488b05f5b644004889")},
        {"phase_combine -> phase_sunshafts (phase_combine.cpp:374)", 0x8b5100, signature("e88bff0200498bcee8331d0300498bce")},
        {"phase_combine -> phase_blur (phase_combine.cpp:378)", 0x8b5108, signature("e8331d0300498bcee8ab3f0300f70575")},
        {"CRender::Render -> render_sun_cascades (r4_R_render.cpp:492)", 0x88e0d6, signature("e8f5fef7ffeb15e85e00f8ff488bcee8")},
    }},
    .sr_first_person_sites = {{
        {"r_dsgraph_render_hud (r__dsgraph_render.cpp:502-520)", 0x846aa0, 0x846e95, 0x846ea0},
        {"r_dsgraph_render_hud_ui (r__dsgraph_render.cpp:538-583)", 0x847000, 0x8473ac, 0x8474ab},
        {"r_dsgraph_render_sorted (r__dsgraph_render.cpp:595-613)", 0x8476a0, 0x847ab3, 0x847abe},
        {"r_dsgraph_render_emissive (r__dsgraph_render.cpp:626-645)", 0x8457e0, 0x845bf3, 0x845bfe},
        {"CParticleEffect::Render HUD mode (ParticleEffect.cpp:599-611)", 0x8cd840, 0x8cddcb, 0x8cddd7},
        {"CRenderTarget::accum_point HUD light (r4_rendertarget_accum_point.cpp:19-31)", 0x9021d0, 0x90260d, 0x902619},
    }},
    .crender_vtable_rva = 0xb53b68,
    .device_view_rva = 0xd256c8,
    .device_project_rva = 0xd25708,
    .device_full_transform_rva = 0xd25748,
    .device_precache_frame_rva = 0xd25670,
    .load_screen_registered_rva = 0xcd21b0,
    .hw_device_rva = 0xcd98d0,
    .hw_context_rva = 0xcd98d8,
    .hw_swap_chain_rva = 0xcd98e0,
    .hw_base_rt_rva = 0xcd98e8,
    .hw_base_zb_rva = 0xcd98f0,
    .render_implementation_rva = 0xd52e40,
    .render_options_word0_rva = 0xd532a0,
    .render_options_word3_rva = 0xd532ac,
    .sunshafts_mode_rva = 0xcd936c,
    .sampler_cache_rva = 0xd51a20,
    .user_mip_bias_rva = 0xd4e7a0,
    .user_anisotropy_rva = 0xcd9878,
    .ssao_set_viewport_rva = 0x8eded0,
    .ssao_viewport_rva = 0xcdcbd0,
    .ssao_viewport_guard_rva = 0xd67284,
    .ssao_set_viewport_code =
        "40534883ec408b0534074600488bd965488b142558000000c5f829742430b910000200c5f8297c2420488b14c2c5f828f2c5f828f98b04113905769347007f2a488b034c8d05b6ec3e00",
    .backend_rva = 0xd51bc0,
    .sun_depth_near_scale_rva = 0xcd932c,
    .sun_depth_far_scale_rva = 0xcd9324,
    .sun_depth_guards = sun_depth_avx_guards,
};
}
