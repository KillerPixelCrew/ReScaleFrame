// SPDX-License-Identifier: GPL-3.0-only
#pragma once
#include "native_profile.h"
// Motion MRT sites (design notes in motion_stage.h). STANDARD 1.5.3 DX11 executable only. Data only: nothing here
// installs a hook. tests/anomaly_motion_evidence.py --verify re-derives every value from the installed executable
// with a disassembler. All functions are x64 __fastcall. Line numbers refer to xray-monolith 028ccf5.
namespace rsf::anomaly::motion {

// Detours. expected = the first 16 bytes at the RVA (same convention as native_sites).
inline constexpr std::array<NativeSite, 3> motion_sites{{
    // static HRESULT create_shader(LPCSTR target, DWORD const* buffer, u32 size, LPCSTR file_name, void*& result,
    // bool disasm) (r4.cpp:963). The only place the engine turns bytecode into a shader: it calls
    // CreatePixel/Vertex/GeometryShader, D3DReflect + R_constant_table::parse and, for VS, D3DGetInputSignatureBlob on
    // the same buffer. Both shader_compile paths (cache hit and fresh compile) call it, nothing else does.
    {"create_shader", 0x838de0, signature("48895c2410488974241848897c242055")},
    // void CRenderTarget::phase_scene_begin() (r4_rendertarget_phase_scene.cpp:31). u_setrt(rt_Position,
    // rt_Color or rt_Accumulator, zb): the G-buffer bind. Called from CRender::Render at 0x898083/0x8980e7/0x898356.
    {"CRenderTarget::phase_scene_begin", 0x8e3d20, signature("40534883ec508b154c7b4000488bd948")},
    // void CRenderTarget::phase_scene_end() (r4_rendertarget_phase_scene.cpp:64). Called at 0x8980e0/0x8983a6.
    {"CRenderTarget::phase_scene_end", 0x8e3df0, signature("405557488d6c24b14881ecb800000048")},
}};
inline constexpr std::size_t site_create_shader = 0, site_phase_scene_begin = 1, site_phase_scene_end = 2;

// HRESULT CRender::shader_compile(name, pSrcData, SrcDataLen, pFunctionName, pTarget, Flags, result) (r4.cpp:1210),
// the only caller of create_shader. Its prologue stores rcx/rbx/r8/r9 in their home slots, pushes 7 registers and
// subtracts 0x14A0, so at either call the caller's rsp is fixed and the values below are rsp-relative.
inline constexpr uint32_t shader_compile_rva = 0x83a610;
inline constexpr char shader_compile_prologue[] =
    "48895c241044894c24204c8944241848894c24085556574154415541564157488dac2460ecffffb8a0140000";
// The two `call create_shader` instructions (E8 rel32, 5 bytes). The detour accepts its caller frame only when its
// return address is one of these + 5.
inline constexpr std::array<NativeSite, 2> create_shader_calls{{
    {"shader_compile -> create_shader (cache file, r4.cpp:1867)", 0x83b9b1, signature("e82ad4ffff89442468488bcee83e6786")},
    {"shader_compile -> create_shader (D3DCompile output, r4.cpp:1903)", 0x83bb93, signature("e848d2ffffe9ea00000048837b50104c")},
}};
inline constexpr uint32_t call_length = 5;
// Caller frame, relative to the caller's rsp at the call (= detour's return-address slot + 8). These are the same
// operands the engine's own D3DCompile call reads (0x83b9ce..0x83ba40): pSrcData [rbp+13F0h], SrcDataLen
// [rbp+13F8h], defines lea [rbp+150h], pFunctionName [rbp+1400h], Flags [rbp+1410h], with rbp = rsp+100h.
inline constexpr uint32_t frame_source_offset = 0x14f0;   // DWORD const* pSrcData (home slot of r8)
inline constexpr uint32_t frame_length_offset = 0x14f8;   // UINT SrcDataLen (home slot of r9d)
inline constexpr uint32_t frame_entry_offset = 0x1500;    // LPCSTR pFunctionName (5th argument)
inline constexpr uint32_t frame_flags_offset = 0x1510;    // DWORD Flags (7th argument)
inline constexpr uint32_t frame_defines_offset = 0x250;   // D3D_SHADER_MACRO defines[128], terminated by Name == 0
inline constexpr uint32_t frame_defines_capacity = 128;
// The D3DCompile call setup the offsets are read from (guarded so a rebuilt frame refuses instead of misreading).
inline constexpr uint32_t d3dcompile_setup_rva = 0x83b9ce;
inline constexpr char d3dcompile_setup[] =
    "8b95f8130000488d05bdb93200488b8df01300004c8d8d50010000488985a00000004c8d051938210033f6488d45804889442450"
    "488d44246848894424488b85101400008974244089442438488b85001400004c896c24304889442428488d85a00000004889442420"
    "488974246848897580e8d5810e00";
// The engine passes "" as D3DCompile's pSourceName (lea r8, 0x140a4f210). RSF compiles with the same name.
inline constexpr uint32_t source_name_rva = 0xa4f210;
// The engine's shader compiler: AnomalyDX11.exe imports D3DCompile, D3DReflect, D3DGetInputSignatureBlob and
// D3DDisassemble from d3dcompiler_43.dll (June 2010 redistributable), not _47. Variants and the proving recompile must
// use the same module, or the original bytecode is not reproduced.
inline constexpr wchar_t engine_compiler_module[] = L"d3dcompiler_43.dll";
// SPS/SVS/SGS: the D3D11 shader interface pointer is at +0x10 (create_shader: lea rax, [r15+10h] as the
// CreatePixel/VertexShader out-parameter, 0x838e38/0x838f02).
inline constexpr uint32_t shader_object_offset = 0x10;

// Hull and domain shaders (r4_enable_tessellation: uber_deffer.cpp r_TessPass, "DX11\tess(...)"). CreateShader<SHS/SDS>
// (ShaderResourceTraits.h) calls the same shader_compile, which calls the same create_shader detour above with target
// "hs_5_0"/"ds_5_0", so the caller frame, the define list and the byte-exact recompile proof are the ones of VS/PS and no
// further detour is installed. create_shader dispatches 'h' to create_shader<SHS> and anything else past 'c' to
// create_shader<SDS> (r4.cpp:934 template, r4.cpp:1139-1146). Guarded so a rebuilt executable refuses instead of the
// detour reading a different object layout:
//   dispatch: cmp al,'h'; [rsp+28h] = disasm; [rsp+20h] = &result; jne -> call create_shader<SDS>, else call
//   create_shader<SHS> (0x839123..0x83914a).
inline constexpr uint32_t tess_dispatch_rva = 0x839123;
inline constexpr char tess_dispatch[] =
    "3c680fb685a801000088442428488b85a001000048894424207507e86dceffffeb05e8b6cdffff";
// The templates: CreateHWShader (a wrapper calling ID3D11Device::CreateHullShader +0x80 / CreateDomainShader +0x88), then
// `mov rcx, [rsi]; mov [rcx+10h], rax`: SHS::sh and SDS::sh are at shader_object_offset like SPS::ps/SVS::vs. Then
// D3DReflect and R_constant_table::parse with RC_dest_hull 0x10 / RC_dest_domain 0x20.
struct TessCreateSite {
    const char* name;
    uint32_t rva;
    const char* prologue;        // function start through the shader-object store
    uint32_t wrapper_rva;        // ShaderTypeTraits<T>::CreateHWShader
    const char* wrapper;         // wrapper start through the ID3D11Device call
};
inline constexpr std::array<TessCreateSite, 2> tess_create_sites{{
    {"create_shader<SHS> (r4.cpp:934, hull)", 0x835fb0,
        "48895c240848896c24104889742418574883ec30488bfa418bd8488bcf418bd0498be9e888140000488b7424604c8d4c24204c8d05af0633008bd3"
        "488b0e48894110",
        0x837460, "4883ec484c8bd148c744246000000000488b0d59444b004c8d4424604c894424204533c94c8bc2498bd2488b01ff9080000000"},
    {"create_shader<SDS> (r4.cpp:934, domain)", 0x835f00,
        "48895c240848896c24104889742418574883ec30488bfa418bd8488bcf418bd0498be9e8a8140000488b7424604c8d4c24204c8d055f0733008bd3"
        "488b0e48894110",
        0x8373d0, "4883ec484c8bd148c744246000000000488b0de9444b004c8d4424604c894424204533c94c8bc2498bd2488b01ff9088000000"},
}};
inline constexpr std::size_t tess_create_hull = 0, tess_create_domain = 1;
}
