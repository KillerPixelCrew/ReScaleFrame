// SPDX-License-Identifier: GPL-3.0-only
#pragma once
#include "native_profile.h"
// Motion MRT sites (design notes in motion_stage.h). One table per exact executable (native_profile.h, build_profile.h).
// Data only: nothing here installs a hook. tests/anomaly_motion_evidence.py --verify [--exe avx] re-derives every value
// from the installed executable with a disassembler. All functions are x64 __fastcall. Line numbers refer to
// xray-monolith 028ccf5.
namespace rsf::anomaly::motion {

// Detours (motion_sites). expected = the first 16 bytes at the RVA (same convention as native_sites).
//   create_shader: static HRESULT create_shader(LPCSTR target, DWORD const* buffer, u32 size, LPCSTR file_name,
//     void*& result, bool disasm) (r4.cpp:963). The only place the engine turns bytecode into a shader: it calls
//     CreatePixel/Vertex/GeometryShader, D3DReflect + R_constant_table::parse and, for VS, D3DGetInputSignatureBlob on
//     the same buffer. Both shader_compile paths (cache hit and fresh compile) call it, nothing else does.
//   phase_scene_begin: void CRenderTarget::phase_scene_begin() (r4_rendertarget_phase_scene.cpp:31). u_setrt(rt_Position,
//     rt_Color or rt_Accumulator, zb): the G-buffer bind. Called three times from CRender::Render.
//   phase_scene_end: void CRenderTarget::phase_scene_end() (r4_rendertarget_phase_scene.cpp:64). Called twice from
//     CRender::Render.
inline constexpr std::size_t motion_site_count = 3;
inline constexpr std::size_t site_create_shader = 0, site_phase_scene_begin = 1, site_phase_scene_end = 2;

// HRESULT CRender::shader_compile(name, pSrcData, SrcDataLen, pFunctionName, pTarget, Flags, result) (r4.cpp:1210),
// the only caller of create_shader. Its prologue stores rcx/rbx/r8/r9 in their home slots, pushes 7 registers and
// subtracts 0x14A0, so at either call the caller's rsp is fixed and the frame offsets are rsp-relative.
// create_shader_calls: the two `call create_shader` instructions (E8 rel32, 5 bytes), cache file (r4.cpp:1867) and
// D3DCompile output (r4.cpp:1903). The detour accepts its caller frame only when its return address is one of these + 5.
inline constexpr uint32_t call_length = 5;
// Caller frame, relative to the caller's rsp at the call (= detour's return-address slot + 8). These are the same
// operands the engine's own D3DCompile call reads: pSrcData [rbp+13F0h], SrcDataLen [rbp+13F8h], defines lea [rbp+150h],
// pFunctionName [rbp+1400h], Flags [rbp+1410h], with rbp = rsp+100h. They depend on codegen, so each build states and
// proves its own (both builds use the same frame).
struct CallerFrame {
    uint32_t source_offset;   // DWORD const* pSrcData (home slot of r8)
    uint32_t length_offset;   // UINT SrcDataLen (home slot of r9d)
    uint32_t entry_offset;    // LPCSTR pFunctionName (5th argument)
    uint32_t flags_offset;    // DWORD Flags (7th argument)
    uint32_t defines_offset;  // D3D_SHADER_MACRO defines[128], terminated by Name == 0
};
inline constexpr uint32_t frame_defines_capacity = 128;
// The engine's shader compiler: both executables import D3DCompile, D3DReflect, D3DGetInputSignatureBlob and
// D3DDisassemble from d3dcompiler_43.dll (June 2010 redistributable), not _47. Variants and the proving recompile must
// use the same module, or the original bytecode is not reproduced.
inline constexpr wchar_t engine_compiler_module[] = L"d3dcompiler_43.dll";
// SPS/SVS/SGS: the D3D11 shader interface pointer is at +0x10 (create_shader: lea rax, [r15+10h] as the
// CreatePixel/VertexShader out-parameter).
inline constexpr uint32_t shader_object_offset = 0x10;

// Hull and domain shaders (r4_enable_tessellation: uber_deffer.cpp r_TessPass, "DX11\tess(...)"). CreateShader<SHS/SDS>
// (ShaderResourceTraits.h) calls the same shader_compile, which calls the same create_shader detour above with target
// "hs_5_0"/"ds_5_0", so the caller frame, the define list and the byte-exact recompile proof are the ones of VS/PS and no
// further detour is installed. create_shader dispatches 'h' to create_shader<SHS> and anything else past 'c' to
// create_shader<SDS> (r4.cpp:934 template, r4.cpp:1139-1146). Guarded so a rebuilt executable refuses instead of the
// detour reading a different object layout:
//   tess_dispatch: cmp al,'h'; [rsp+28h] = disasm; [rsp+20h] = &result; jne -> call create_shader<SDS>, else call
//   create_shader<SHS>.
// tess_create_sites: CreateHWShader (a wrapper calling ID3D11Device::CreateHullShader +0x80 / CreateDomainShader +0x88),
// then `mov rcx, [rsi]; mov [rcx+10h], rax`: SHS::sh and SDS::sh are at shader_object_offset like SPS::ps/SVS::vs. Then
// D3DReflect and R_constant_table::parse with RC_dest_hull 0x10 / RC_dest_domain 0x20.
struct TessCreateSite {
    const char* name;
    uint32_t rva;
    const char* prologue;        // function start through the shader-object store
    uint32_t wrapper_rva;        // ShaderTypeTraits<T>::CreateHWShader
    const char* wrapper;         // wrapper start through the ID3D11Device call
};
inline constexpr std::size_t tess_create_hull = 0, tess_create_domain = 1;

struct MotionProfile {
    std::array<NativeSite, motion_site_count> motion_sites;
    std::span<const UniqueGuard> motion_unique_guards;
    uint32_t shader_compile_rva;
    const char* shader_compile_prologue;
    std::array<NativeSite, 2> create_shader_calls;
    CallerFrame frame;
    // The D3DCompile call setup the offsets are read from (guarded so a rebuilt frame refuses instead of misreading).
    uint32_t d3dcompile_setup_rva;
    const char* d3dcompile_setup;
    // The engine passes "" as D3DCompile's pSourceName (lea r8 in the setup). RSF compiles with the same name.
    uint32_t source_name_rva;
    uint32_t tess_dispatch_rva;
    const char* tess_dispatch;
    std::array<TessCreateSite, 2> tess_create_sites;
};

// ---- Standard 1.5.3 DX11 (AnomalyDX11.exe) -------------------------------------------------------------------------
inline constexpr std::array<UniqueGuard, 1> motion_standard_unique{{
    {"create_shader", 0x838de0, "48895c2410488974241848897c2420554154415541564157488dac24b0"},
}};
inline constexpr MotionProfile motion_standard{
    .motion_sites = {{
        {"create_shader", 0x838de0, signature("48895c2410488974241848897c242055")},
        {"CRenderTarget::phase_scene_begin", 0x8e3d20, signature("40534883ec508b154c7b4000488bd948")},
        {"CRenderTarget::phase_scene_end", 0x8e3df0, signature("405557488d6c24b14881ecb800000048")},
    }},
    .motion_unique_guards = motion_standard_unique,
    .shader_compile_rva = 0x83a610,
    .shader_compile_prologue =
        "48895c241044894c24204c8944241848894c24085556574154415541564157488dac2460ecffffb8a0140000",
    .create_shader_calls = {{
        {"shader_compile -> create_shader (cache file, r4.cpp:1867)", 0x83b9b1, signature("e82ad4ffff89442468488bcee83e6786")},
        {"shader_compile -> create_shader (D3DCompile output, r4.cpp:1903)", 0x83bb93, signature("e848d2ffffe9ea00000048837b50104c")},
    }},
    .frame = {.source_offset = 0x14f0, .length_offset = 0x14f8, .entry_offset = 0x1500, .flags_offset = 0x1510,
        .defines_offset = 0x250},
    .d3dcompile_setup_rva = 0x83b9ce,
    .d3dcompile_setup =
        "8b95f8130000488d05bdb93200488b8df01300004c8d8d50010000488985a00000004c8d051938210033f6488d45804889442450"
        "488d44246848894424488b85101400008974244089442438488b85001400004c896c24304889442428488d85a00000004889442420"
        "488974246848897580e8d5810e00",
    .source_name_rva = 0xa4f210,
    .tess_dispatch_rva = 0x839123,
    .tess_dispatch = "3c680fb685a801000088442428488b85a001000048894424207507e86dceffffeb05e8b6cdffff",
    .tess_create_sites = {{
        {"create_shader<SHS> (r4.cpp:934, hull)", 0x835fb0,
            "48895c240848896c24104889742418574883ec30488bfa418bd8488bcf418bd0498be9e888140000488b7424604c8d4c24204c8d05af0633008bd3"
            "488b0e48894110",
            0x837460, "4883ec484c8bd148c744246000000000488b0d59444b004c8d4424604c894424204533c94c8bc2498bd2488b01ff9080000000"},
        {"create_shader<SDS> (r4.cpp:934, domain)", 0x835f00,
            "48895c240848896c24104889742418574883ec30488bfa418bd8488bcf418bd0498be9e8a8140000488b7424604c8d4c24204c8d055f0733008bd3"
            "488b0e48894110",
            0x8373d0, "4883ec484c8bd148c744246000000000488b0de9444b004c8d4424604c894424204533c94c8bc2498bd2488b01ff9088000000"},
    }},
};

// ---- AVX 1.5.3 DX11 (AnomalyDX11AVX.exe) ---------------------------------------------------------------------------
// shader_compile has the same prologue, frame and D3DCompile operand layout as the standard build; only rip-relative and
// rel32 displacements differ.
inline constexpr std::array<UniqueGuard, 1> motion_avx_unique{{
    {"create_shader", 0x830080, "48895c2410488974241848897c2420554154415541564157488dac24b0"},
}};
inline constexpr MotionProfile motion_avx{
    .motion_sites = {{
        {"create_shader", 0x830080, signature("48895c2410488974241848897c242055")},
        {"CRenderTarget::phase_scene_begin", 0x8d8b90, signature("40534883ec508b15dc0c4000488bd948")},
        {"CRenderTarget::phase_scene_end", 0x8d8c60, signature("405557488d6c24b14881ecb800000048")},
    }},
    .motion_unique_guards = motion_avx_unique,
    .shader_compile_rva = 0x831900,
    .shader_compile_prologue =
        "48895c241044894c24204c8944241848894c24085556574154415541564157488dac2460ecffffb8a0140000",
    .create_shader_calls = {{
        {"shader_compile -> create_shader (cache file, r4.cpp:1867)", 0x832cb1, signature("e8cad3ffff89442468488bcee8fee086")},
        {"shader_compile -> create_shader (D3DCompile output, r4.cpp:1903)", 0x832e93, signature("e8e8d1ffffe9ea00000048837b50104c")},
    }},
    .frame = {.source_offset = 0x14f0, .length_offset = 0x14f8, .entry_offset = 0x1500, .flags_offset = 0x1510,
        .defines_offset = 0x250},
    .d3dcompile_setup_rva = 0x832cce,
    .d3dcompile_setup =
        "8b95f8130000488d052d183200488b8df01300004c8d8d50010000488985a00000004c8d054995200033f6488d45804889442450"
        "488d44246848894424488b85101400008974244089442438488b85001400004c896c24304889442428488d85a00000004889442420"
        "488974246848897580e845490e00",
    .source_name_rva = 0xa3c240,
    .tess_dispatch_rva = 0x8303c3,
    .tess_dispatch = "3c680fb685a801000088442428488b85a001000048894424207507e81dceffffeb05e866cdffff",
    .tess_create_sites = {{
        {"create_shader<SHS> (r4.cpp:934, hull)", 0x82d200,
            "48895c240848896c24104889742418574883ec30488bfa418bd8488bcf418bd0498be9e888140000488b7424604c8d4c24204c8d05cf6532008bd3"
            "488b0e48894110",
            0x82e6b0, "4883ec484c8bd148c744246000000000488b0d09b24a004c8d4424604c894424204533c94c8bc2498bd2488b01ff9080000000"},
        {"create_shader<SDS> (r4.cpp:934, domain)", 0x82d150,
            "48895c240848896c24104889742418574883ec30488bfa418bd8488bcf418bd0498be9e8a8140000488b7424604c8d4c24204c8d057f6632008bd3"
            "488b0e48894110",
            0x82e620, "4883ec484c8bd148c744246000000000488b0d99b24a004c8d4424604c894424204533c94c8bc2498bd2488b01ff9088000000"},
    }},
};
}
