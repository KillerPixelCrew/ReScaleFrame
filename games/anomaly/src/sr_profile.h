// SPDX-License-Identifier: GPL-3.0-only
#pragma once
#include "native_profile.h"
namespace rsf::anomaly::sr {
// Render-resolution split (SR Stage 0, docs/research/anomaly-sr-seam.md). STANDARD 1.5.3 DX11 executable only.
// Data only: nothing here installs a hook or changes rendering. Every value is re-checked against the installed
// executable by tests/anomaly_sr_evidence.py --verify (decompile/disassembly evidence in that script and the doc).
// All functions are x64 __fastcall with `this` in rcx. Line numbers refer to xray-monolith 028ccf5.

// Detour and call-target candidates. expected = the first 16 bytes at the RVA (same convention as native_sites).
inline constexpr std::array<NativeSite, 15> sr_function_sites{{
    // void CRenderTarget::phase_scene_prepare() (r4_rendertarget_phase_scene.cpp:4). H1.
    {"CRenderTarget::phase_scene_prepare", 0x8e4310, signature("40534883ec50f7058c0f480000400000")},
    // void CRenderTarget::phase_combine() (r4_rendertarget_phase_combine.cpp:39). Hosts the S-B seam calls.
    {"CRenderTarget::phase_combine", 0x8bc980, signature("488bc45556415441554156488da888fe")},
    // void CRenderTarget::phase_sunshafts() (rendertarget_phase_sunshafts.cpp:3). H2a.
    {"CRenderTarget::phase_sunshafts", 0x8f07c0, signature("4c8bdc554156498d6ba14881ece80000")},
    // void CRenderTarget::phase_blur() (rendertarget_phase_blur.cpp:3). H2b. 16 bytes are not unique: see unique guards.
    {"CRenderTarget::phase_blur", 0x8f2590, signature("48895c242055565741564157488bec48")},
    // void CRender::rmNear() / rmFar() (r4.cpp:834,843), CRender vtable slots 55 and 56. rmNear is H4 and is the
    // same function geometry_profile.h already detours (geometry_site_rm_near): H4 must share that detour.
    {"CRender::rmNear", 0x83a450, signature("40534883ec40488b01ff507848c74424")},
    {"CRender::rmFar", 0x83a3c0, signature("40534883ec40488b01ff507848c74424")},
    // void R_xforms::set_P(const Fmatrix&) (R_Backend_xform.cpp:32), rdx = matrix; copies it to this+0xC0 (m_p).
    {"R_xforms::set_P", 0x83ecd0, signature("48895c240848896c2410488974241848")},
    // void R_xforms::set_V(const Fmatrix&) (R_Backend_xform.cpp:19); copies to this+0x80 (m_v).
    {"R_xforms::set_V", 0x83f970, signature("48895c240848896c2410488974241857")},
    // void CTexture::surface_set(ID3DBaseTexture*) (dx10SH_Texture.cpp:55): AddRef new, Release pSurface and
    // m_pSRView, store, refresh desc, CreateShaderResourceView (R24G8_TYPELESS -> R24_UNORM_X8_TYPELESS).
    {"CTexture::surface_set", 0x87ac30, signature("48895c24185556574883ec40488bfa48")},
    // void dx10SamplerStateCache::SetMipLODBias(float) (dx10SamplerStateCache.cpp:201), bias in xmm1. Rebuilds every
    // sampler state when the value changes.
    {"dx10SamplerStateCache::SetMipLODBias", 0x7fe170, signature("40574881eca0000000f30f10819c0100")},
    // void dx10SamplerStateCache::SetMaxAnisotropy(u32) (dx10SamplerStateCache.cpp:172), edx = value.
    {"dx10SamplerStateCache::SetMaxAnisotropy", 0x7fe060, signature("40574881eca0000000488bf983fa0173")},
    // void CBackend::Invalidate() (R_Backend_Runtime.cpp:63). Clears every RCache cache, including the texture slots.
    {"CBackend::Invalidate", 0x842c50, signature("48895c240848896c2410488974241857")},
    // void CRenderTarget::u_setrt(u32 W, u32 H, RTV0, RTV1, RTV2, DSV) (r4_rendertarget.cpp:111).
    {"CRenderTarget::u_setrt(W,H)", 0x859cb0, signature("4883ec288951084489410c4c3b0d76a4")},
    // First-person projection rebuild sites outside geometry_profile.h (see sr_first_person_sites).
    {"PS::CParticleEffect::Render", 0x8d8420, signature("40534881ec80010000488bd9e87fa40f")},
    {"CRenderTarget::accum_point", 0x90dca0, signature("488bc45553488da8d8feffff4881ec18")},
}};
inline constexpr std::size_t site_phase_scene_prepare = 0, site_phase_combine = 1, site_phase_sunshafts = 2,
    site_phase_blur = 3, site_rm_near = 4, site_rm_far = 5, site_set_p = 6, site_set_v = 7, site_surface_set = 8,
    site_set_mip_lod_bias = 9, site_set_max_anisotropy = 10, site_backend_invalidate = 11, site_u_setrt_wh = 12;

// Where 16 bytes are not unique in .text: the shortest prefix at the same RVA that is (rmNear/rmFar/rmNormal differ
// only in their rip-relative MaxDepth displacement at byte 61).
struct UniqueGuard {
    const char* name;
    uint32_t rva;
    const char* expected_hex;
};
inline constexpr std::array<UniqueGuard, 6> sr_unique_guards{{
    {"CRenderTarget::phase_blur", 0x8f2590, "48895c242055565741564157488bec4881"},
    {"CRender::rmNear", 0x83a450, "40534883ec40488b01ff507848c744242000000000488bc8488bd8488b10ff52508bc80f57c0f3480f2ac1488bcbf30f11442428488b13ff5258488b0d47"},
    {"CRender::rmFar", 0x83a3c0, "40534883ec40488b01ff507848c744242000000000488bc8488bd8488b10ff52508bc80f57c0f3480f2ac1488bcbf30f11442428488b13ff5258488b0dd7"},
    {"R_xforms::set_P", 0x83ecd0, "48895c240848896c2410488974241848897c242041564883ec208b02"},
    {"R_xforms::set_V", 0x83f970, "48895c240848896c2410488974241857415641574883ec208b02"},
    {"CBackend::Invalidate", 0x842c50, "48895c240848896c24104889742418574883ec2033ed488bd94889a9"},
}};

// Real `call rel32` instructions (no LTCG inlining). expected = 16 bytes from the E8 opcode.
inline constexpr std::array<NativeSite, 5> sr_call_sites{{
    {"CRender::Render -> phase_scene_prepare (z-prepass, r4_R_render.cpp:278)", 0x897d2c, signature("e8dfc5040033d2488d0d76414500e861")},
    {"CRender::Render -> phase_scene_prepare (r4_R_render.cpp:285)", 0x897d66, signature("e8a5c50400488b1d76fd4900833db77b")},
    {"CRender::Render -> phase_combine (r4_R_render.cpp:538)", 0x8985bf, signature("e8bc430200eb39488b05233345004889")},
    {"phase_combine -> phase_sunshafts (phase_combine.cpp:374)", 0x8bf273, signature("e848150300498bcee810330300498bce")},
    {"phase_combine -> phase_blur (phase_combine.cpp:378)", 0x8bf27b, signature("e810330300498bcee858550300f70502")},
}};

// Every site that rebuilds a first-person projection, uploads it with R_xforms::set_P and then calls rmNear().
// rm_near_call is the virtual `call [rax+0x1b8]` (slot 55) or a direct call to CRender::rmNear.
struct FirstPersonSite {
    const char* name;
    uint32_t function_rva;
    uint32_t set_p_call;
    uint32_t rm_near_call;
};
inline constexpr std::array<FirstPersonSite, 6> sr_first_person_sites{{
    {"r_dsgraph_render_hud (r__dsgraph_render.cpp:502-520)", 0x84ffa0, 0x850410, 0x85041b},
    {"r_dsgraph_render_hud_ui (r__dsgraph_render.cpp:538-583)", 0x8505a0, 0x8509b5, 0x850ab4},
    {"r_dsgraph_render_sorted (r__dsgraph_render.cpp:595-613)", 0x850cc0, 0x851135, 0x851140},
    {"r_dsgraph_render_emissive (r__dsgraph_render.cpp:626-645)", 0x84ec80, 0x84f0f5, 0x84f100},
    {"CParticleEffect::Render HUD mode (ParticleEffect.cpp:599-611)", 0x8d8420, 0x8d8a8e, 0x8d8a9a},
    {"CRenderTarget::accum_point HUD light (r4_rendertarget_accum_point.cpp:19-31)", 0x90dca0, 0x90e16c, 0x90e178},
}};
inline constexpr uint32_t crender_vtable_rva = 0xb669f8;
inline constexpr uint32_t rm_near_slot = 55, rm_far_slot = 56, rm_normal_slot = 57;

// Globals (RVA from image base 0x140000000). Device.dwWidth/dwHeight are native_profile.h device_width/height_rva.
inline constexpr uint32_t device_view_rva = 0xd376c8;            // Device.mView
inline constexpr uint32_t device_project_rva = 0xd37708;         // Device.mProject
inline constexpr uint32_t device_full_transform_rva = 0xd37748;  // Device.mFullTransform
// Loading screen. CRenderDevice::PreCache registers load_screen_renderer (b_registered = 1, b_need_user_input = the next byte)
// and CGamePersistent::OnFrame stops it only when dwPrecacheFrame is 0 and the "Press any key to continue" sequence is done,
// so the flag stays set through the key wait, when the world still renders under the loading screen and dwPrecacheFrame is 0.
inline constexpr uint32_t device_precache_frame_rva = 0xd37670;  // Device.dwPrecacheFrame (device_width_rva + 8)
inline constexpr uint32_t load_screen_registered_rva = 0xce41b0; // load_screen_renderer.b_registered (bool)
inline constexpr uint32_t hw_device_rva = 0xceb8d0, hw_context_rva = 0xceb8d8, hw_swap_chain_rva = 0xceb8e0;
inline constexpr uint32_t hw_base_rt_rva = 0xceb8e8, hw_base_zb_rva = 0xceb8f0;  // HW.pBaseRT, HW.pBaseZB
inline constexpr uint32_t render_implementation_rva = 0xd64e40;  // RImplementation (CRender)
inline constexpr uint32_t render_target_offset = 0x768;          // CRender::Target (CRenderTarget*)
inline constexpr uint32_t render_options_word0_rva = 0xd652a0;   // CRender::o, first u32 of the bitfield
inline constexpr uint32_t mrtmixdepth_bit = 1u << 26, fp16_blend_bit = 1u << 28;
inline constexpr uint32_t render_options_word3_rva = 0xd652ac;
inline constexpr uint32_t advancedpp_bit = 1u << 12, dx10_msaa_bit = 1u << 14;
inline constexpr uint32_t sunshafts_mode_rva = 0xceb36c;         // ps_sunshafts_mode; 2/3 run phase_sunshafts
inline constexpr uint32_t sampler_cache_rva = 0xd63a20;          // SSManager (dx10SamplerStateCache)
inline constexpr uint32_t sampler_mip_lod_bias_offset = 0x19c;   // m_uiMipLODBias (float)
inline constexpr uint32_t user_mip_bias_rva = 0xd607a0;          // ps_r__tf_Mipbias (float)
inline constexpr uint32_t user_anisotropy_rva = 0xceb878;        // ps_r__tf_Anisotropic (u32)

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

// SSAO viewport helper (r4_rendertarget_phase_ssao.cpp:3-10): `static D3D_VIEWPORT viewport[1] = {0, 0, w, h, 0, 1}`
// is a function-local static, so its width and height are fixed by the first call for the rest of the process and
// later calls only re-apply it. Stock calls it from phase_downsamp (with o.ssao_half_data) and phase_ssao (with
// o.ssao_blur_on and no ssao_opt_data), always first with Device.dwWidth/2 x Device.dwHeight/2. In a Stage 2 world
// window the static must hold render/2, and output/2 again afterwards. Binary (0x8f95f0, x64 magic static):
// `cmp [guard], tls_epoch; jg init` then `lea r8,[viewport]; jmp [ctx->vtbl+0x160]` (RSSetViewports); the init path
// stores only Width (+8) and Height (+0xC); X, Y, MinDepth 0 and MaxDepth 1.0 are in .data. The guard is in .bss:
// 0 = never initialised, -1 = being initialised, anything else = initialised. Callers: 0x8f84ff and 0x8f89a7
// (phase_downsamp 0x8f8400), 0x8f8b90 and 0x8f957f (phase_ssao 0x8f89d0); no other reference.
inline constexpr uint32_t ssao_set_viewport_rva = 0x8f95f0;
inline constexpr uint32_t ssao_viewport_rva = 0xceebd0;          // D3D11_VIEWPORT {X, Y, Width, Height, MinDepth, MaxDepth}
inline constexpr uint32_t ssao_viewport_width_offset = 0x8, ssao_viewport_height_offset = 0xc;
inline constexpr uint32_t ssao_viewport_guard_rva = 0xd79284;    // int32 magic-static guard
// The first 0x46 bytes of set_viewport, up to and including `lea r8,[rip+viewport]`; the displacements at +0x36
// (guard, next instruction +0x3a) and +0x42 (viewport, next +0x46) prove both data addresses.
inline constexpr const char* ssao_set_viewport_code =
    "40534883ec408b0514704600488bd965488b1425580000000f297424300f28f2b9100002000f297c24200f28f9488b14c28b041139055afc47007f28488b034c8d059a553f00";
// The CRender::o word0 bits phase_combine tests before calling the SSAO passes (0x8bca75: test al,0x20 / 0x40 ->
// phase_hdao 0x8f9690; test al,4 -> phase_downsamp; test al,2 -> phase_ssao). Documentation and verifier only.
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
inline constexpr uint32_t backend_rva = 0xd63bc0;
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
}
