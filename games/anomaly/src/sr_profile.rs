// SPDX-License-Identifier: MIT
//! SR render-resolution split profiles for both exact Anomaly 1.5.3 DX11 executables.
//! Evidence: docs/research/anomaly-sr-seam.md and anomaly-avx-profile.md; tests/anomaly_sr_evidence.py re-checks
//! every value against the installed executables. tools/generate-anomaly-sr-profile.py projects these records
//! into sr_profile.h for the native consumers and the evidence tools.
// Every profiled value is projected into sr_profile.h and checked by the evidence tools; Rust reads a subset.
#![allow(dead_code)]

use crate::engine_guards::{NativeSite, UniqueGuard, site};
use std::ffi::CStr;

// ---- layout shared by both builds (sr_profile.h documents each value) ----
pub(crate) const SR_FUNCTION_SITE_COUNT: usize = 17;
pub(crate) const SITE_PHASE_SCENE_PREPARE: usize = 0;
pub(crate) const SITE_PHASE_COMBINE: usize = 1;
pub(crate) const SITE_PHASE_SUNSHAFTS: usize = 2;
pub(crate) const SITE_PHASE_BLUR: usize = 3;
pub(crate) const SITE_RM_NEAR: usize = 4;
pub(crate) const SITE_RM_FAR: usize = 5;
pub(crate) const SITE_SET_P: usize = 6;
pub(crate) const SITE_SET_V: usize = 7;
pub(crate) const SITE_SURFACE_SET: usize = 8;
pub(crate) const SITE_SET_MIP_LOD_BIAS: usize = 9;
pub(crate) const SITE_SET_MAX_ANISOTROPY: usize = 10;
pub(crate) const SITE_BACKEND_INVALIDATE: usize = 11;
pub(crate) const SITE_U_SETRT_WH: usize = 12;
pub(crate) const SITE_SUN_CASCADES: usize = 15;
pub(crate) const SITE_POSITION_SETUP: usize = 16;
pub(crate) const SR_CALL_SITE_COUNT: usize = 6;
pub(crate) const SR_FIRST_PERSON_SITE_COUNT: usize = 6;
pub(crate) const RM_NEAR_SLOT: u32 = 55;
pub(crate) const RM_FAR_SLOT: u32 = 56;
pub(crate) const RM_NORMAL_SLOT: u32 = 57;
pub(crate) const RENDER_TARGET_OFFSET: u32 = 0x768;
pub(crate) const MRTMIXDEPTH_BIT: u32 = 1 << 26;
pub(crate) const FP16_BLEND_BIT: u32 = 1 << 28;
pub(crate) const ADVANCEDPP_BIT: u32 = 1 << 12;
pub(crate) const DX10_MSAA_BIT: u32 = 1 << 14;
pub(crate) const SAMPLER_MIP_LOD_BIAS_OFFSET: u32 = 0x19c;
pub(crate) const TARGET_WIDTH_OFFSET: u32 = 0x8;
pub(crate) const TARGET_HEIGHT_OFFSET: u32 = 0xc;
pub(crate) const TARGET_LUM_SRC_TEXTURE_OFFSET: u32 = 0x4c8;
pub(crate) const TARGET_LUM_DEST_TEXTURE_OFFSET: u32 = 0x4d0;
pub(crate) const D3DFMT_A8R8G8B8: u32 = 21;
pub(crate) const D3DFMT_A16B16G16R16F: u32 = 113;
pub(crate) const D3DFMT_R16F: u32 = 111;
pub(crate) const D3DFMT_R32F: u32 = 114;
pub(crate) const SSAO_VIEWPORT_WIDTH_OFFSET: u32 = 0x8;
pub(crate) const SSAO_VIEWPORT_HEIGHT_OFFSET: u32 = 0xc;
pub(crate) const SSAO_BLUR_ON_BIT: u32 = 1 << 1;
pub(crate) const SSAO_OPT_DATA_BIT: u32 = 1 << 2;
pub(crate) const SSAO_HALF_DATA_BIT: u32 = 1 << 3;
pub(crate) const SSAO_HDAO_BIT: u32 = 1 << 5;
pub(crate) const SSAO_ULTRA_BIT: u32 = 1 << 6;
pub(crate) const CRT_SIZE: u32 = 0x50;
pub(crate) const CRT_REFERENCE_OFFSET: u32 = 0x0;
pub(crate) const CRT_SURFACE_OFFSET: u32 = 0x10;
pub(crate) const CRT_RTV_OFFSET: u32 = 0x18;
pub(crate) const CRT_DSV_OFFSET: u32 = 0x20;
pub(crate) const CRT_UAV_OFFSET: u32 = 0x28;
pub(crate) const CRT_TEXTURE_OFFSET: u32 = 0x30;
pub(crate) const CRT_WIDTH_OFFSET: u32 = 0x38;
pub(crate) const CRT_HEIGHT_OFFSET: u32 = 0x3c;
pub(crate) const CRT_FORMAT_OFFSET: u32 = 0x40;
pub(crate) const CRT_ORDER_OFFSET: u32 = 0x48;
pub(crate) const TEXTURE_REFERENCE_OFFSET: u32 = 0x8;
pub(crate) const TEXTURE_SURFACE_OFFSET: u32 = 0x50;
pub(crate) const TEXTURE_DESC_CACHE_OFFSET: u32 = 0x70;
pub(crate) const TEXTURE_DESC_OFFSET: u32 = 0x78;
pub(crate) const TEXTURE_SRV_OFFSET: u32 = 0xa8;
pub(crate) const BACKEND_XFORMS_OFFSET: u32 = 0x58;
pub(crate) const BACKEND_RT_OFFSET: u32 = 0x578;
pub(crate) const BACKEND_ZB_OFFSET: u32 = 0x598;
pub(crate) const BACKEND_CTABLE_OFFSET: u32 = 0x5c8;
pub(crate) const BACKEND_TEXTURE_LIST_OFFSET: u32 = 0x640;
pub(crate) const BACKEND_TEXTURES_PS_OFFSET: u32 = 0x658;
pub(crate) const BACKEND_TEXTURES_PS_COUNT: u32 = 16;
pub(crate) const BACKEND_TEXTURES_VS_OFFSET: u32 = 0x6d8;
pub(crate) const BACKEND_TEXTURES_VS_COUNT: u32 = 4;
pub(crate) const BACKEND_TEXTURES_GS_OFFSET: u32 = 0x6f8;
pub(crate) const BACKEND_TEXTURES_HS_OFFSET: u32 = 0x778;
pub(crate) const BACKEND_TEXTURES_DS_OFFSET: u32 = 0x7f8;
pub(crate) const BACKEND_TEXTURES_CS_OFFSET: u32 = 0x878;
pub(crate) const BACKEND_TEXTURES_STAGE_COUNT: u32 = 16;
pub(crate) const BACKEND_RT_CHANGED_OFFSET: u32 = 0x980;
pub(crate) const XFORMS_VIEW_OFFSET: u32 = 0x80;
pub(crate) const XFORMS_PROJECT_OFFSET: u32 = 0xc0;

pub(crate) struct TargetSlot {
    pub(crate) name: &'static CStr,
    pub(crate) offset: u32,
    pub(crate) d3d_format: u32,
}

const fn slot(name: &'static CStr, offset: u32, d3d_format: u32) -> TargetSlot {
    TargetSlot {
        name,
        offset,
        d3d_format,
    }
}

pub(crate) static TARGET_SLOTS: [TargetSlot; 26] = [
    slot(c"$user$position", 0x378, 113),
    slot(c"$user$msaadepth", 0x358, 75),
    slot(c"$user$albedo", 0x380, 21),
    slot(c"$user$accum", 0x388, 113),
    slot(c"$user$accum_temp", 0x390, 113),
    slot(c"$user$generic0", 0x3a8, 21),
    slot(c"$user$generic1", 0x3b0, 21),
    slot(c"$user$generic", 0x370, 21),
    slot(c"$user$generic_temp", 0x3b8, 21),
    slot(c"$user$viewport2", 0x3c0, 21),
    slot(c"$user$dof", 0x3c8, 21),
    slot(c"$user$ui", 0x3d0, 21),
    slot(c"$user$sun_shafts0", 0x398, 21),
    slot(c"$user$sun_shafts1", 0x3a0, 21),
    slot(c"$user$blur_h_2", 0x3d8, 21),
    slot(c"$user$blur_2", 0x3e0, 21),
    slot(c"$user$blur_h_4", 0x3e8, 21),
    slot(c"$user$blur_4", 0x3f0, 21),
    slot(c"$user$blur_h_8", 0x3f8, 21),
    slot(c"$user$blur_8", 0x400, 21),
    slot(c"$user$pp_bloom", 0x408, 21),
    slot(c"$user$generic0_r", 0x360, 21),
    slot(c"$user$generic1_r", 0x368, 21),
    slot(c"$user$generic2", 0x420, 113),
    slot(c"$user$half_depth", 0x590, 0),
    slot(c"$user$ssao_temp", 0x588, 0),
];

pub(crate) struct WorldTarget {
    pub(crate) name: &'static CStr,
    pub(crate) offset: u32,
    pub(crate) d3d_format: u32,
    pub(crate) optional: bool,
    pub(crate) half_allowed: bool,
}

const fn world(
    name: &'static CStr,
    offset: u32,
    d3d_format: u32,
    optional: bool,
    half_allowed: bool,
) -> WorldTarget {
    WorldTarget {
        name,
        offset,
        d3d_format,
        optional,
        half_allowed,
    }
}

pub(crate) static WORLD_TARGETS: [WorldTarget; 9] = [
    world(c"$user$generic0", 0x3a8, 21, false, false),
    world(c"$user$generic_temp", 0x3b8, 21, false, false),
    world(c"$user$position", 0x378, 113, false, false),
    world(c"$user$albedo", 0x380, 21, false, false),
    world(c"$user$accum", 0x388, 113, false, false),
    world(c"$user$generic1", 0x3b0, 21, false, false),
    world(c"$user$generic2", 0x420, 113, true, false),
    world(c"$user$half_depth", 0x590, 0, true, true),
    world(c"$user$ssao_temp", 0x588, 0, true, false),
];
pub(crate) const WORLD_ABSENT_OFFSETS: [u32; 4] = [0x358, 0x360, 0x368, 0x390];

#[derive(Clone, Copy)]
pub(crate) struct FirstPersonSite {
    pub(crate) name: &'static CStr,
    pub(crate) function_rva: u32,
    pub(crate) set_p_call: u32,
    pub(crate) rm_near_call: u32,
}

const fn first_person(
    name: &'static CStr,
    function_rva: u32,
    set_p_call: u32,
    rm_near_call: u32,
) -> FirstPersonSite {
    FirstPersonSite {
        name,
        function_rva,
        set_p_call,
        rm_near_call,
    }
}

#[derive(Clone, Copy)]
pub(crate) struct Profile {
    pub(crate) label: &'static CStr,
    pub(crate) function_sites: [NativeSite; SR_FUNCTION_SITE_COUNT],
    pub(crate) unique_guards: &'static [UniqueGuard],
    pub(crate) call_sites: [NativeSite; SR_CALL_SITE_COUNT],
    pub(crate) first_person_sites: [FirstPersonSite; SR_FIRST_PERSON_SITE_COUNT],
    pub(crate) crender_vtable_rva: u32,
    pub(crate) device_view_rva: u32,
    pub(crate) device_project_rva: u32,
    pub(crate) device_full_transform_rva: u32,
    pub(crate) device_precache_frame_rva: u32,
    pub(crate) load_screen_registered_rva: u32,
    pub(crate) hw_device_rva: u32,
    pub(crate) hw_context_rva: u32,
    pub(crate) hw_swap_chain_rva: u32,
    pub(crate) hw_base_rt_rva: u32,
    pub(crate) hw_base_zb_rva: u32,
    pub(crate) render_implementation_rva: u32,
    pub(crate) render_options_word0_rva: u32,
    pub(crate) render_options_word3_rva: u32,
    pub(crate) sunshafts_mode_rva: u32,
    pub(crate) sampler_cache_rva: u32,
    pub(crate) user_mip_bias_rva: u32,
    pub(crate) user_anisotropy_rva: u32,
    pub(crate) ssao_set_viewport_rva: u32,
    pub(crate) ssao_viewport_rva: u32,
    pub(crate) ssao_viewport_guard_rva: u32,
    pub(crate) ssao_set_viewport_code: &'static CStr,
    pub(crate) backend_rva: u32,
    pub(crate) position_setup_vtable_rva: u32,
    pub(crate) get_constant_buffer_rva: u32,
    pub(crate) reconstruction_guards: [UniqueGuard; 4],
}

// SAFETY: the raw pointers in the immutable records point to static C string literals.
unsafe impl Sync for Profile {}
unsafe impl Sync for TargetSlot {}
unsafe impl Sync for WorldTarget {}

const STANDARD_UNIQUE: [UniqueGuard; 7] = [
    UniqueGuard::new(c"CRender::render_sun_cascades", 0x815a30, c"48895c240848896c2410488974241848897c242041564883ec20488bd948"),
    UniqueGuard::new(c"CRenderTarget::phase_blur", 0x8f2590, c"48895c242055565741564157488bec4881"),
    UniqueGuard::new(c"CRender::rmNear", 0x83a450, c"40534883ec40488b01ff507848c744242000000000488bc8488bd8488b10ff52508bc80f57c0f3480f2ac1488bcbf30f11442428488b13ff5258488b0d47"),
    UniqueGuard::new(c"CRender::rmFar", 0x83a3c0, c"40534883ec40488b01ff507848c744242000000000488bc8488bd8488b10ff52508bc80f57c0f3480f2ac1488bcbf30f11442428488b13ff5258488b0dd7"),
    UniqueGuard::new(c"R_xforms::set_P", 0x83ecd0, c"48895c240848896c2410488974241848897c242041564883ec208b02"),
    UniqueGuard::new(c"R_xforms::set_V", 0x83f970, c"48895c240848896c2410488974241857415641574883ec208b02"),
    UniqueGuard::new(c"CBackend::Invalidate", 0x842c50, c"48895c240848896c24104889742418574883ec2033ed488bd94889a9"),
];
const AVX_UNIQUE: [UniqueGuard; 6] = [
    UniqueGuard::new(c"CRender::render_sun_cascades", 0x80dfd0, c"48895c240848896c2410488974241848897c242041564883ec20488bd948"),
    UniqueGuard::new(c"CRenderTarget::phase_blur", 0x8e6e40, c"488bc44889582055565741564157488d68a14881eca0"),
    UniqueGuard::new(c"CRender::rmNear", 0x831700, c"40534883ec50488b01c5f829742440ff5078c5c857f6c5fa11742420c5fa11742424488b10488bc8488bd8ff52508bc8c5f857c0c4e1fa2ac1c5fa11442428488b13488bcbff5258488b0d89"),
    UniqueGuard::new(c"R_xforms::set_P", 0x835fc0, c"48895c240848896c2410488974241848897c242041564883ec208b02"),
    UniqueGuard::new(c"R_xforms::set_V", 0x836c60, c"48895c240848896c2410488974241857415641574883ec208b02"),
    UniqueGuard::new(c"CBackend::Invalidate", 0x839f80, c"48895c240848896c24104889742418574883ec2033ed488bd94889a9"),
];

/// In native_profile.rs order (the build index Rust admits).
pub(crate) static PROFILES: [Profile; 2] = [
    Profile {
        label: c"standard",
        function_sites: [
            site(c"CRenderTarget::phase_scene_prepare", 0x8e4310, b"40534883ec50f7058c0f480000400000"),
            site(c"CRenderTarget::phase_combine", 0x8bc980, b"488bc45556415441554156488da888fe"),
            site(c"CRenderTarget::phase_sunshafts", 0x8f07c0, b"4c8bdc554156498d6ba14881ece80000"),
            site(c"CRenderTarget::phase_blur", 0x8f2590, b"48895c242055565741564157488bec48"),
            site(c"CRender::rmNear", 0x83a450, b"40534883ec40488b01ff507848c74424"),
            site(c"CRender::rmFar", 0x83a3c0, b"40534883ec40488b01ff507848c74424"),
            site(c"R_xforms::set_P", 0x83ecd0, b"48895c240848896c2410488974241848"),
            site(c"R_xforms::set_V", 0x83f970, b"48895c240848896c2410488974241857"),
            site(c"CTexture::surface_set", 0x87ac30, b"48895c24185556574883ec40488bfa48"),
            site(c"dx10SamplerStateCache::SetMipLODBias", 0x7fe170, b"40574881eca0000000f30f10819c0100"),
            site(c"dx10SamplerStateCache::SetMaxAnisotropy", 0x7fe060, b"40574881eca0000000488bf983fa0173"),
            site(c"CBackend::Invalidate", 0x842c50, b"48895c240848896c2410488974241857"),
            site(c"CRenderTarget::u_setrt(W,H)", 0x859cb0, b"4883ec288951084489410c4c3b0d76a4"),
            site(c"PS::CParticleEffect::Render", 0x8d8420, b"40534881ec80010000488bd9e87fa40f"),
            site(c"CRenderTarget::accum_point", 0x90dca0, b"488bc45553488da8d8feffff4881ec18"),
            site(c"CRender::render_sun_cascades", 0x815a30, b"48895c240848896c2410488974241848"),
            site(c"cl_pos_decompress_params::setup", 0x834f70, b"40574883ec30f30f1005d6285000488b"),
        ],
        unique_guards: &STANDARD_UNIQUE,
        call_sites: [
            site(c"CRender::Render -> phase_scene_prepare (z-prepass, r4_R_render.cpp:278)", 0x897d2c, b"e8dfc5040033d2488d0d76414500e861"),
            site(c"CRender::Render -> phase_scene_prepare (r4_R_render.cpp:285)", 0x897d66, b"e8a5c50400488b1d76fd4900833db77b"),
            site(c"CRender::Render -> phase_combine (r4_R_render.cpp:538)", 0x8985bf, b"e8bc430200eb39488b05233345004889"),
            site(c"phase_combine -> phase_sunshafts (phase_combine.cpp:374)", 0x8bf273, b"e848150300498bcee810330300498bce"),
            site(c"phase_combine -> phase_blur (phase_combine.cpp:378)", 0x8bf27b, b"e810330300498bcee858550300f70502"),
            site(c"CRender::Render -> render_sun_cascades (r4_R_render.cpp:492)", 0x8984a8, b"e883d5f7ffeb15e8ecd6f7ff488bcee8"),
        ],
        first_person_sites: [
            first_person(c"r_dsgraph_render_hud (r__dsgraph_render.cpp:502-520)", 0x84ffa0, 0x850410, 0x85041b),
            first_person(c"r_dsgraph_render_hud_ui (r__dsgraph_render.cpp:538-583)", 0x8505a0, 0x8509b5, 0x850ab4),
            first_person(c"r_dsgraph_render_sorted (r__dsgraph_render.cpp:595-613)", 0x850cc0, 0x851135, 0x851140),
            first_person(c"r_dsgraph_render_emissive (r__dsgraph_render.cpp:626-645)", 0x84ec80, 0x84f0f5, 0x84f100),
            first_person(c"CParticleEffect::Render HUD mode (ParticleEffect.cpp:599-611)", 0x8d8420, 0x8d8a8e, 0x8d8a9a),
            first_person(c"CRenderTarget::accum_point HUD light (r4_rendertarget_accum_point.cpp:19-31)", 0x90dca0, 0x90e16c, 0x90e178),
        ],
        crender_vtable_rva: 0xb669f8,
        device_view_rva: 0xd376c8,
        device_project_rva: 0xd37708,
        device_full_transform_rva: 0xd37748,
        device_precache_frame_rva: 0xd37670,
        load_screen_registered_rva: 0xce41b0,
        hw_device_rva: 0xceb8d0,
        hw_context_rva: 0xceb8d8,
        hw_swap_chain_rva: 0xceb8e0,
        hw_base_rt_rva: 0xceb8e8,
        hw_base_zb_rva: 0xceb8f0,
        render_implementation_rva: 0xd64e40,
        render_options_word0_rva: 0xd652a0,
        render_options_word3_rva: 0xd652ac,
        sunshafts_mode_rva: 0xceb36c,
        sampler_cache_rva: 0xd63a20,
        user_mip_bias_rva: 0xd607a0,
        user_anisotropy_rva: 0xceb878,
        ssao_set_viewport_rva: 0x8f95f0,
        ssao_viewport_rva: 0xceebd0,
        ssao_viewport_guard_rva: 0xd79284,
        ssao_set_viewport_code: c"40534883ec408b0514704600488bd965488b1425580000000f297424300f28f2b9100002000f297c24200f28f9488b14c28b041139055afc47007f28488b034c8d059a553f00",
        backend_rva: 0xd63bc0,
        position_setup_vtable_rva: 0xb665f8,
        get_constant_buffer_rva: 0x84b820,
        reconstruction_guards: [
            UniqueGuard::new(c"position binder body", 0x834f70, c"40574883ec30f30f1005d6285000488bfaf30f590513a12100f30f5905cba22100f30f5e05cba22100e84eed0e000f28d0f30f59157baf21000f28caf30f5e0da42850000f570d05a121004885ff0f8482020000f64710010f57c08b0597265000f30f114c2420f30f58c9f30f11542424f30f58d248895c2440bb04000000f3480f2ac08b0572265000f30f5ec80f57c0f3480f2ac0f30f114c2428f30f5ed0f30f1154242c74564533c0488d0d5ef15200488bd7e8f6670100440fb74f14448bc34c034860c64068010fb74f1683e901741683e901740b83f901750c448d43feeb0641b8030000004e8d048500000000498bc9488d542420e83cec0e00f6471002745941b801000000488d0dfff05200488bd7e897670100440fb74f184c8bc34c034860c64068010fb74f1a83e901741683e901740b83f901750c448d4101eb0641b8030000004e8d048500000000498bc9488d542420e8ddeb0e00f6471008745941b802000000488d0da0f05200488bd7e838670100440fb74f1c4c8bc34c034860c64068010fb74f1e83e901741683e901740b83f901750c448d4101eb0641b8030000004e8d048500000000498bc9488d542420e87eeb0e00f6471010745941b803000000488d0d41f05200488bd7e8d9660100440fb74f204c8bc34c034860c64068010fb74f2283e901741683e901740b83f901750c448d4101eb0641b8030000004e8d048500000000498bc9488d542420e81feb0e00f64710207456448bc3488d0de5ef5200488bd7e87d660100440fb74f244c8bc34c034860c64068010fb74f2683e901741683e901740b83f901750c448d4101eb0641b8030000004e8d048500000000498bc9488d542420e8c3ea0e00f6471040745441b805000000488d0d86ef5200488bd7e81e660100440fb74f284c034860c64068010fb74f2a83e901741483e901740a83f901750a8d5901eb05bb030000004c8d049d00000000498bc9488d542420e869ea0e00488b5c24404883c4305fc3"),
            UniqueGuard::new(c"R_constants::GetCBuffer", 0x84b820, c"4883ec384585c0751b0fb74212488d0d8c83510083e00f488b84c1280300004883c438c34183f801751e8b4210488d0d6c83510048c1e80c83e00f488b84c1b80200004883c438c34183f802751e8b4210488d0d4883510048c1e80883e00f488b84c1980300004883c438c34183f803751e8b4210488d0d2483510048c1e81483e00f488b84c1080400004883c438c34183f804751b0fb64213488d0dff82510083e00f488b84c1780400004883c438c34183f805751b8b4210488d0ddf82510048c1e81c488b84c1e80400004883c438c3"),
            UniqueGuard::new(c"position binder registration", 0x838b4f, c"4c8d05ca324b00488bcb488d15c0e43200e83bedffff"),
            UniqueGuard::new(c"constant buffer reflected size", 0x83db90, c"488d5424404c896b10498bcc4c896b204c896b284c896b304c896b384c896b404c896b48c6436801498b0424ff10488b542440488b0dfe1b5000e8011186ff4885c07402ff00488b4b104885c9740fff09488b4b1044392975044c896b108b54244c488d4b20488943108b4424488943188b442450894358"),
        ],
    },
    Profile {
        label: c"avx",
        function_sites: [
            site(c"CRenderTarget::phase_scene_prepare", 0x8d9160, b"40534883ec50f7053ca1470000400000"),
            site(c"CRenderTarget::phase_combine", 0x8b2950, b"488bc45556415441554156488da888fe"),
            site(c"CRenderTarget::phase_sunshafts", 0x8e5090, b"4c8bdc554156498d6ba14881ecf80000"),
            site(c"CRenderTarget::phase_blur", 0x8e6e40, b"488bc44889582055565741564157488d"),
            site(c"CRender::rmNear", 0x831700, b"40534883ec50488b01c5f829742440ff"),
            site(c"CRender::rmFar", 0x831670, b"40534883ec40488b01ff5078c5f857c0"),
            site(c"R_xforms::set_P", 0x835fc0, b"48895c240848896c2410488974241848"),
            site(c"R_xforms::set_V", 0x836c60, b"48895c240848896c2410488974241857"),
            site(c"CTexture::surface_set", 0x870870, b"48895c24185556574883ec40488bfa48"),
            site(c"dx10SamplerStateCache::SetMipLODBias", 0x7f7b80, b"40574881eca0000000c5fa10819c0100"),
            site(c"dx10SamplerStateCache::SetMaxAnisotropy", 0x7f7a70, b"40574881eca0000000488bf983fa0173"),
            site(c"CBackend::Invalidate", 0x839f80, b"48895c240848896c2410488974241857"),
            site(c"CRenderTarget::u_setrt(W,H)", 0x8502c0, b"4883ec288951084489410c4c3b0d661e"),
            site(c"PS::CParticleEffect::Render", 0x8cd840, b"40534881ec80010000488bd9e87f470f"),
            site(c"CRenderTarget::accum_point", 0x9021d0, b"488bc45553488da8c8feffff4881ec28"),
            site(c"CRender::render_sun_cascades", 0x80dfd0, b"48895c240848896c2410488974241848"),
            site(c"cl_pos_decompress_params::setup", 0x82c1c0, b"40574883ec30c5fa100586964f00c5fa"),
        ],
        unique_guards: &AVX_UNIQUE,
        call_sites: [
            site(c"CRender::Render -> phase_scene_prepare (z-prepass, r4_R_render.cpp:278)", 0x88d957, b"e804b8040033d2488d0d4bc54400e866"),
            site(c"CRender::Render -> phase_scene_prepare (r4_R_render.cpp:285)", 0x88d991, b"e8cab70400488b1d4b814900833d8cff"),
            site(c"CRender::Render -> phase_combine (r4_R_render.cpp:538)", 0x88e1ed, b"e85e470200eb39488b05f5b644004889"),
            site(c"phase_combine -> phase_sunshafts (phase_combine.cpp:374)", 0x8b5100, b"e88bff0200498bcee8331d0300498bce"),
            site(c"phase_combine -> phase_blur (phase_combine.cpp:378)", 0x8b5108, b"e8331d0300498bcee8ab3f0300f70575"),
            site(c"CRender::Render -> render_sun_cascades (r4_R_render.cpp:492)", 0x88e0d6, b"e8f5fef7ffeb15e85e00f8ff488bcee8"),
        ],
        first_person_sites: [
            first_person(c"r_dsgraph_render_hud (r__dsgraph_render.cpp:502-520)", 0x846aa0, 0x846e95, 0x846ea0),
            first_person(c"r_dsgraph_render_hud_ui (r__dsgraph_render.cpp:538-583)", 0x847000, 0x8473ac, 0x8474ab),
            first_person(c"r_dsgraph_render_sorted (r__dsgraph_render.cpp:595-613)", 0x8476a0, 0x847ab3, 0x847abe),
            first_person(c"r_dsgraph_render_emissive (r__dsgraph_render.cpp:626-645)", 0x8457e0, 0x845bf3, 0x845bfe),
            first_person(c"CParticleEffect::Render HUD mode (ParticleEffect.cpp:599-611)", 0x8cd840, 0x8cddcb, 0x8cddd7),
            first_person(c"CRenderTarget::accum_point HUD light (r4_rendertarget_accum_point.cpp:19-31)", 0x9021d0, 0x90260d, 0x902619),
        ],
        crender_vtable_rva: 0xb53b68,
        device_view_rva: 0xd256c8,
        device_project_rva: 0xd25708,
        device_full_transform_rva: 0xd25748,
        device_precache_frame_rva: 0xd25670,
        load_screen_registered_rva: 0xcd21b0,
        hw_device_rva: 0xcd98d0,
        hw_context_rva: 0xcd98d8,
        hw_swap_chain_rva: 0xcd98e0,
        hw_base_rt_rva: 0xcd98e8,
        hw_base_zb_rva: 0xcd98f0,
        render_implementation_rva: 0xd52e40,
        render_options_word0_rva: 0xd532a0,
        render_options_word3_rva: 0xd532ac,
        sunshafts_mode_rva: 0xcd936c,
        sampler_cache_rva: 0xd51a20,
        user_mip_bias_rva: 0xd4e7a0,
        user_anisotropy_rva: 0xcd9878,
        ssao_set_viewport_rva: 0x8eded0,
        ssao_viewport_rva: 0xcdcbd0,
        ssao_viewport_guard_rva: 0xd67284,
        ssao_set_viewport_code: c"40534883ec408b0534074600488bd965488b142558000000c5f829742430b910000200c5f8297c2420488b14c2c5f828f2c5f828f98b04113905769347007f2a488b034c8d05b6ec3e00",
        backend_rva: 0xd51bc0,
        position_setup_vtable_rva: 0xb53768,
        get_constant_buffer_rva: 0x842380,
        reconstruction_guards: [
            UniqueGuard::new(c"position binder body", 0x82c1c0, c"40574883ec30c5fa100586964f00c5fa590dc6fe2000c5f25915ae002100c5ea5e05ae002100488bfae86eb50e00c5fa5915720d2100c5ea5e0d5a964f00c5f05705bafe20004885ff0f8484020000f64710018b054f944f00c5fa58c8c5fa11442420c5f857c0c4e1fa2ac08b053a944f00c5f25ec8c5f857c0c5fa11542424c4e1fa2ac0c5ea58d2c5fa114c2428c5ea5ec848895c2440bb04000000c5fa114c242c74564533c0488d0d115f5200488bd7e809610100440fb74f14448bc34c034860c64068010fb74f1683e901741683e901740b83f901750c448d43feeb0641b8030000004e8d048500000000498bc9488d542420e85fb40e00f6471002745941b801000000488d0db25e5200488bd7e8aa600100440fb74f184c8bc34c034860c64068010fb74f1a83e901741683e901740b83f901750c448d4101eb0641b8030000004e8d048500000000498bc9488d542420e800b40e00f6471008745941b802000000488d0d535e5200488bd7e84b600100440fb74f1c4c8bc34c034860c64068010fb74f1e83e901741683e901740b83f901750c448d4101eb0641b8030000004e8d048500000000498bc9488d542420e8a1b30e00f6471010745941b803000000488d0df45d5200488bd7e8ec5f0100440fb74f204c8bc34c034860c64068010fb74f2283e901741683e901740b83f901750c448d4101eb0641b8030000004e8d048500000000498bc9488d542420e842b30e00f64710207456448bc3488d0d985d5200488bd7e8905f0100440fb74f244c8bc34c034860c64068010fb74f2683e901741683e901740b83f901750c448d4101eb0641b8030000004e8d048500000000498bc9488d542420e8e6b20e00f6471040745441b805000000488d0d395d5200488bd7e8315f0100440fb74f284c034860c64068010fb74f2a83e901741483e901740a83f901750a8d5901eb05bb030000004c8d049d00000000498bc9488d542420e88cb20e00488b5c24404883c4305fc3"),
            UniqueGuard::new(c"R_constants::GetCBuffer", 0x842380, c"4883ec384585c0751b0fb74212488d0d2cf8500083e00f488b84c1280300004883c438c34183f801751e8b4210488d0d0cf8500048c1e80c83e00f488b84c1b80200004883c438c34183f802751e8b4210488d0de8f7500048c1e80883e00f488b84c1980300004883c438c34183f803751e8b4210488d0dc4f7500048c1e81483e00f488b84c1080400004883c438c34183f804751b0fb64213488d0d9ff7500083e00f488b84c1780400004883c438c34183f805751b8b4210488d0d7ff7500048c1e81c488b84c1e80400004883c438c3"),
            UniqueGuard::new(c"position binder registration", 0x82fdf0, c"4c8d0529a04a00488bcb488d158f433200e8eaecffff"),
            UniqueGuard::new(c"constant buffer reflected size", 0x834e80, c"488d5424404c896b10498bcc4c896b204c896b284c896b304c896b384c896b404c896b48c6436801498b0424ff10488b542440488b0d0e894f00e8d18986ff4885c07402ff00488b4b104885c9740fff09488b4b1044392975044c896b108b54244c488d4b20488943108b4424488943188b442450894358"),
        ],
    },
];

// ---- the swap owner's layout, shared by both builds --------------------------------------------------------------
use crate::sys::{
    RSF_ANOMALY_SWAP_DESC_SIZE, rsf_anomaly_swap_layout as SwapLayout,
    rsf_anomaly_swap_range as Range, rsf_anomaly_swap_world_target as SwapWorld,
};

const fn same(a: &CStr, b: &CStr) -> bool {
    let (a, b) = (a.to_bytes(), b.to_bytes());
    if a.len() != b.len() {
        return false;
    }
    let mut i = 0;
    while i < a.len() {
        if a[i] != b[i] {
            return false;
        }
        i += 1;
    }
    true
}

// Every world target is the verified CRenderTarget slot with the same offset and format, in sr_swap.h slot order.
const fn world_targets_match_slots() -> bool {
    let mut w = 0;
    while w < WORLD_TARGETS.len() {
        let world = &WORLD_TARGETS[w];
        let mut found = false;
        let mut s = 0;
        while s < TARGET_SLOTS.len() {
            let slot = &TARGET_SLOTS[s];
            if same(slot.name, world.name) {
                found = slot.offset == world.offset && slot.d3d_format == world.d3d_format;
            }
            s += 1;
        }
        if !found {
            return false;
        }
        w += 1;
    }
    same(WORLD_TARGETS[0].name, c"$user$generic0")
        && same(WORLD_TARGETS[1].name, c"$user$generic_temp")
        && same(WORLD_TARGETS[2].name, c"$user$position")
        && same(WORLD_TARGETS[5].name, c"$user$generic1")
        && WORLD_TARGETS[0].d3d_format == D3DFMT_A8R8G8B8
        && WORLD_TARGETS[1].d3d_format == D3DFMT_A8R8G8B8
}

const _: () = {
    assert!(world_targets_match_slots());
    assert!(TEXTURE_DESC_OFFSET + RSF_ANOMALY_SWAP_DESC_SIZE <= TEXTURE_SRV_OFFSET);
    assert!(CRT_FORMAT_OFFSET + 4 <= CRT_ORDER_OFFSET);
    assert!(SSAO_VIEWPORT_HEIGHT_OFFSET == SSAO_VIEWPORT_WIDTH_OFFSET + 4);
    // CRT field order follows SH_RT.h; pTexture is a ref_texture (one pointer) and the record ends after _order.
    assert!(CRT_SURFACE_OFFSET < CRT_RTV_OFFSET && CRT_RTV_OFFSET < CRT_DSV_OFFSET);
    assert!(CRT_DSV_OFFSET < CRT_UAV_OFFSET && CRT_UAV_OFFSET < CRT_TEXTURE_OFFSET);
    assert!(
        CRT_TEXTURE_OFFSET + 8 == CRT_WIDTH_OFFSET && CRT_WIDTH_OFFSET + 4 == CRT_HEIGHT_OFFSET
    );
    assert!(CRT_HEIGHT_OFFSET + 4 == CRT_FORMAT_OFFSET && CRT_ORDER_OFFSET + 8 == CRT_SIZE);
    // RCache texture caches: ps[16], vs[4], then gs/hs/ds/cs[16] contiguous; T precedes them (STextureList*, M, C).
    assert!(
        BACKEND_TEXTURES_VS_OFFSET == BACKEND_TEXTURES_PS_OFFSET + 8 * BACKEND_TEXTURES_PS_COUNT
    );
    assert!(
        BACKEND_TEXTURES_GS_OFFSET == BACKEND_TEXTURES_VS_OFFSET + 8 * BACKEND_TEXTURES_VS_COUNT
    );
    assert!(
        BACKEND_TEXTURES_HS_OFFSET == BACKEND_TEXTURES_GS_OFFSET + 8 * BACKEND_TEXTURES_STAGE_COUNT
    );
    assert!(
        BACKEND_TEXTURES_DS_OFFSET == BACKEND_TEXTURES_HS_OFFSET + 8 * BACKEND_TEXTURES_STAGE_COUNT
    );
    assert!(
        BACKEND_TEXTURES_CS_OFFSET == BACKEND_TEXTURES_DS_OFFSET + 8 * BACKEND_TEXTURES_STAGE_COUNT
    );
    assert!(BACKEND_TEXTURE_LIST_OFFSET + 24 == BACKEND_TEXTURES_PS_OFFSET);
    assert!(XFORMS_PROJECT_OFFSET == XFORMS_VIEW_OFFSET + 64);
};

const fn range(offset: u32, count: u32) -> Range {
    Range { offset, count }
}

const fn engine_layout() -> SwapLayout {
    let mut world = [SwapWorld {
        offset: 0,
        d3d_format: 0,
        optional: false,
        half_allowed: false,
    }; WORLD_TARGETS.len()];
    let mut i = 0;
    while i < world.len() {
        let target = &WORLD_TARGETS[i];
        world[i] = SwapWorld {
            offset: target.offset,
            d3d_format: target.d3d_format,
            optional: target.optional,
            half_allowed: target.half_allowed,
        };
        i += 1;
    }
    SwapLayout {
        struct_size: size_of::<SwapLayout>() as u32,
        crt_surface: CRT_SURFACE_OFFSET,
        crt_rtv: CRT_RTV_OFFSET,
        crt_dsv: CRT_DSV_OFFSET,
        crt_uav: CRT_UAV_OFFSET,
        crt_texture: CRT_TEXTURE_OFFSET,
        crt_width: CRT_WIDTH_OFFSET,
        crt_height: CRT_HEIGHT_OFFSET,
        crt_format: CRT_FORMAT_OFFSET,
        texture_surface: TEXTURE_SURFACE_OFFSET,
        texture_desc_cache: TEXTURE_DESC_CACHE_OFFSET,
        texture_desc: TEXTURE_DESC_OFFSET,
        texture_srv: TEXTURE_SRV_OFFSET,
        target_width: TARGET_WIDTH_OFFSET,
        target_height: TARGET_HEIGHT_OFFSET,
        backend_texture_list: BACKEND_TEXTURE_LIST_OFFSET,
        backend_ctable: BACKEND_CTABLE_OFFSET,
        backend_textures: [
            range(BACKEND_TEXTURES_PS_OFFSET, BACKEND_TEXTURES_PS_COUNT),
            range(BACKEND_TEXTURES_VS_OFFSET, BACKEND_TEXTURES_VS_COUNT),
            range(BACKEND_TEXTURES_GS_OFFSET, BACKEND_TEXTURES_STAGE_COUNT),
            range(BACKEND_TEXTURES_HS_OFFSET, BACKEND_TEXTURES_STAGE_COUNT),
            range(BACKEND_TEXTURES_DS_OFFSET, BACKEND_TEXTURES_STAGE_COUNT),
            range(BACKEND_TEXTURES_CS_OFFSET, BACKEND_TEXTURES_STAGE_COUNT),
        ],
        ssao_viewport_width: SSAO_VIEWPORT_WIDTH_OFFSET,
        format_a16b16g16r16f: D3DFMT_A16B16G16R16F,
        format_r16f: D3DFMT_R16F,
        format_r32f: D3DFMT_R32F,
        world,
        absent: WORLD_ABSENT_OFFSETS,
    }
}

static ENGINE_LAYOUT: SwapLayout = engine_layout();

/// The verified engine layout shared by both executable profiles, in the swap owner's form.
#[unsafe(no_mangle)]
pub(crate) extern "C" fn rsf_anomaly_sr_engine_layout() -> &'static SwapLayout {
    &ENGINE_LAYOUT
}
