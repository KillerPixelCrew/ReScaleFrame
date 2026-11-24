// SPDX-License-Identifier: MIT
//! Motion MRT sites (design notes in motion_stage.h), built from the running executable's discovered sites
//! (crate::discovery); the two 1.5.3 DX11 builds' records stay as RECORDED for the tests and fixtures. Data only:
//! nothing here installs a hook. tools/anomaly_motion_evidence.py --verify [--exe avx] checks every discovered value
//! against the installed executable with a disassembler. All
//! functions are x64 __fastcall. Line numbers refer to xray-monolith 028ccf5.
//!
//! Detours (`sites`, expected = the first 16 bytes at the RVA, as the native sites):
//! * create_shader: static HRESULT create_shader(LPCSTR target, DWORD const* buffer, u32 size, LPCSTR
//!   file_name, void*& result, bool disasm) (r4.cpp:963). The only place the engine turns bytecode into a
//!   shader: it calls CreatePixel/Vertex/GeometryShader, D3DReflect + R_constant_table::parse and, for VS,
//!   D3DGetInputSignatureBlob on the same buffer. Both shader_compile paths (cache hit and fresh compile) call
//!   it, nothing else does.
//! * phase_scene_begin: void CRenderTarget::phase_scene_begin() (r4_rendertarget_phase_scene.cpp:31).
//!   u_setrt(rt_Position, rt_Color or rt_Accumulator, zb): the G-buffer bind. Called three times from
//!   CRender::Render.
//! * phase_scene_end: void CRenderTarget::phase_scene_end() (r4_rendertarget_phase_scene.cpp:64). Called
//!   twice from CRender::Render.
//!
//! HRESULT CRender::shader_compile(name, pSrcData, SrcDataLen, pFunctionName, pTarget, Flags, result)
//! (r4.cpp:1210) is the only caller of create_shader. Its prologue stores rcx/rbx/r8/r9 in their home slots,
//! pushes 7 registers and subtracts 0x14A0, so at either call the caller's rsp is fixed and the frame offsets
//! are rsp-relative. `create_shader_calls` are the two `call create_shader` instructions (E8 rel32, 5
//! bytes), cache file (r4.cpp:1867) and D3DCompile output (r4.cpp:1903). The detour accepts its caller frame
//! only when its return address is one of these + 5.
//!
//! Hull and domain shaders (r4_enable_tessellation: uber_deffer.cpp r_TessPass, "DX11\tess(...)").
//! CreateShader<SHS/SDS> (ShaderResourceTraits.h) calls the same shader_compile, which calls the same
//! create_shader detour with target "hs_5_0"/"ds_5_0", so the caller frame, the define list and the
//! byte-exact recompile proof are the ones of VS/PS and no further detour is installed. create_shader
//! dispatches 'h' to create_shader<SHS> and anything else past 'c' to create_shader<SDS> (r4.cpp:934
//! template, r4.cpp:1139-1146). Guarded so a rebuilt executable refuses instead of the detour reading a
//! different object layout:
//! * tess_dispatch: cmp al,'h'; [rsp+28h] = disasm; [rsp+20h] = &result; jne -> call create_shader<SDS>,
//!   else call create_shader<SHS>.
//! * tess_create_sites: CreateHWShader (a wrapper calling ID3D11Device::CreateHullShader +0x80 /
//!   CreateDomainShader +0x88), then `mov rcx, [rsi]; mov [rcx+10h], rax`: SHS::sh and SDS::sh are at
//!   SHADER_OBJECT_OFFSET like SPS::ps/SVS::vs. Then D3DReflect and R_constant_table::parse with
//!   RC_dest_hull 0x10 / RC_dest_domain 0x20.
use crate::discovery::{Found, Site, records};
#[cfg(any(test, feature = "recorded-profiles"))]
use crate::engine_guards::site;
use crate::engine_guards::{NativeSite, UniqueGuard};
use crate::shader_loader::{EngineShaderProfile, FrameLayout};
use std::ffi::CStr;
use windows_sys::w;

pub(crate) const SITE_COUNT: usize = 3;
pub(crate) const SITE_CREATE_SHADER: usize = 0;
pub(crate) const SITE_PHASE_SCENE_BEGIN: usize = 1;
pub(crate) const SITE_PHASE_SCENE_END: usize = 2;
// The native leaf fills the detours in this order (RSF_ANOMALY_MOTION_SITE_* in motion_stage_platform.h).
const _: () = assert!(
    SITE_CREATE_SHADER == 0
        && SITE_PHASE_SCENE_BEGIN == 1
        && SITE_PHASE_SCENE_END == 2
        && SITE_COUNT == 3
);
/// `call rel32`.
pub(crate) const CALL_LENGTH: u32 = 5;
/// D3D_SHADER_MACRO defines[128], terminated by Name == 0.
pub(crate) const FRAME_DEFINES_CAPACITY: u32 = 128;
/// SPS/SVS/SGS: the D3D11 shader interface pointer is at +0x10 (create_shader: lea rax, [r15+10h] as the
/// CreatePixel/VertexShader out-parameter).
pub(crate) const SHADER_OBJECT_OFFSET: usize = 0x10;
/// Both executables import D3DCompile, D3DReflect, D3DGetInputSignatureBlob and D3DDisassemble from
/// d3dcompiler_43.dll (June 2010 redistributable), not _47. Variants and the proving recompile must use the
/// same module, or the original bytecode is not reproduced.
pub(crate) const ENGINE_COMPILER: *const u16 = w!("d3dcompiler_43.dll");

/// Caller frame, relative to the caller's rsp at the call (= the detour's return-address slot + 8). These are
/// the operands the engine's own D3DCompile call reads: pSrcData [rbp+13F0h], SrcDataLen [rbp+13F8h], defines
/// lea [rbp+150h], pFunctionName [rbp+1400h], Flags [rbp+1410h], with rbp = rsp+100h. They depend on codegen:
/// the discovered shader_compile prologue and D3DCompile setup patterns fix every one of them byte for byte.
pub(crate) struct CallerFrame {
    /// DWORD const* pSrcData (home slot of r8).
    pub(crate) source_offset: u32,
    /// UINT SrcDataLen (home slot of r9d).
    pub(crate) length_offset: u32,
    /// LPCSTR pFunctionName (5th argument).
    pub(crate) entry_offset: u32,
    /// DWORD Flags (7th argument).
    pub(crate) flags_offset: u32,
    pub(crate) defines_offset: u32,
}

pub(crate) struct TessCreateSite {
    pub(crate) name: &'static CStr,
    pub(crate) rva: u32,
    /// Function start through the shader-object store.
    pub(crate) prologue: &'static CStr,
    /// ShaderTypeTraits<T>::CreateHWShader.
    pub(crate) wrapper_rva: u32,
    /// Wrapper start through the ID3D11Device call.
    pub(crate) wrapper: &'static CStr,
}

pub(crate) struct MotionProfile {
    pub(crate) sites: [NativeSite; SITE_COUNT],
    pub(crate) unique_guards: [UniqueGuard; 1],
    pub(crate) shader_compile_rva: u32,
    pub(crate) shader_compile_prologue: &'static CStr,
    pub(crate) create_shader_calls: [NativeSite; 2],
    pub(crate) frame: CallerFrame,
    /// The D3DCompile call setup the offsets are read from (guarded so a rebuilt frame refuses).
    pub(crate) d3dcompile_setup_rva: u32,
    pub(crate) d3dcompile_setup: &'static CStr,
    /// The engine passes "" as D3DCompile's pSourceName (lea r8 in the setup). RSF compiles with the same.
    pub(crate) source_name_rva: u32,
    pub(crate) tess_dispatch_rva: u32,
    pub(crate) tess_dispatch: &'static CStr,
    pub(crate) tess_create_sites: [TessCreateSite; 2],
}

// SAFETY: the raw pointers in the two immutable records point to static C string literals and are never
// written through.
unsafe impl Sync for MotionProfile {}

impl MotionProfile {
    /// The motion record of profile `index` (crate::discovery::records), unless its sites refused.
    pub(crate) fn of(index: usize) -> Option<&'static Self> {
        records().get(index)?.motion.ok()
    }

    pub(crate) fn frame_layout(&self) -> FrameLayout {
        let frame = &self.frame;
        FrameLayout {
            calls: self.create_shader_calls.each_ref().map(|call| call.rva),
            call_length: CALL_LENGTH,
            source: frame.source_offset,
            length: frame.length_offset,
            entry: frame.entry_offset,
            flags: frame.flags_offset,
            defines: frame.defines_offset,
            define_capacity: FRAME_DEFINES_CAPACITY,
        }
    }

    pub(crate) fn shader_profile(&self) -> EngineShaderProfile {
        EngineShaderProfile {
            compiler_name: ENGINE_COMPILER,
            setup_rva: self.d3dcompile_setup_rva,
        }
    }
}

const fn tess_site(
    name: &'static CStr,
    rva: u32,
    prologue: &'static CStr,
    wrapper_rva: u32,
    wrapper: &'static CStr,
) -> TessCreateSite {
    TessCreateSite {
        name,
        rva,
        prologue,
        wrapper_rva,
        wrapper,
    }
}

/// The caller frame the discovered prologue (`lea rbp,[rsp-13A0h]` after `sub rsp,14A0h`) and D3DCompile setup
/// operands fix: rbp = rsp+100h and the [rbp+...] operands above.
pub(crate) const FRAME: CallerFrame = CallerFrame {
    source_offset: 0x14f0,
    length_offset: 0x14f8,
    entry_offset: 0x1500,
    flags_offset: 0x1510,
    defines_offset: 0x250,
};

/// The motion profile of the discovered sites.
pub(crate) fn discovered(found: &Found) -> MotionProfile {
    MotionProfile {
        sites: [
            Site::CreateShader,
            Site::PhaseSceneBegin,
            Site::PhaseSceneEnd,
        ]
        .map(|site| found.site(site)),
        unique_guards: [found.guard(Site::CreateShader)],
        shader_compile_rva: found.rva(Site::ShaderCompile),
        shader_compile_prologue: found.code(Site::ShaderCompile),
        create_shader_calls: [Site::CacheCall, Site::CompileCall].map(|site| found.site(site)),
        frame: FRAME,
        d3dcompile_setup_rva: found.rva(Site::CompileSetup),
        d3dcompile_setup: found.code(Site::CompileSetup),
        source_name_rva: found.rva(Site::SourceName),
        tess_dispatch_rva: found.rva(Site::TessDispatch),
        tess_dispatch: found.code(Site::TessDispatch),
        tess_create_sites: [
            (Site::HullCreate, Site::HullWrapper),
            (Site::DomainCreate, Site::DomainWrapper),
        ]
        .map(|(create, wrapper)| {
            tess_site(
                found.name(create),
                found.rva(create),
                found.code(create),
                found.rva(wrapper),
                found.code(wrapper),
            )
        }),
    }
}

/// The records of the two 1.5.3 DX11 builds, Standard then AVX: test and fixture data.
#[cfg(any(test, feature = "recorded-profiles"))]
pub(crate) static RECORDED: [MotionProfile; 2] = [
    // ---- Standard 1.5.3 DX11 (AnomalyDX11.exe) ----
    MotionProfile {
        sites: [
            site(c"create_shader", 0x838de0, b"48895c2410488974241848897c242055"),
            site(
                c"CRenderTarget::phase_scene_begin",
                0x8e3d20,
                b"40534883ec508b154c7b4000488bd948",
            ),
            site(
                c"CRenderTarget::phase_scene_end",
                0x8e3df0,
                b"405557488d6c24b14881ecb800000048",
            ),
        ],
        unique_guards: [UniqueGuard::new(
            c"create_shader",
            0x838de0,
            c"48895c2410488974241848897c2420554154415541564157488dac24b0",
        )],
        shader_compile_rva: 0x83a610,
        shader_compile_prologue:
            c"48895c241044894c24204c8944241848894c24085556574154415541564157488dac2460ecffffb8a0140000",
        create_shader_calls: [
            site(
                c"shader_compile -> create_shader (cache file, r4.cpp:1867)",
                0x83b9b1,
                b"e82ad4ffff89442468488bcee83e6786",
            ),
            site(
                c"shader_compile -> create_shader (D3DCompile output, r4.cpp:1903)",
                0x83bb93,
                b"e848d2ffffe9ea00000048837b50104c",
            ),
        ],
        frame: CallerFrame {
            source_offset: 0x14f0,
            length_offset: 0x14f8,
            entry_offset: 0x1500,
            flags_offset: 0x1510,
            defines_offset: 0x250,
        },
        d3dcompile_setup_rva: 0x83b9ce,
        d3dcompile_setup: c"8b95f8130000488d05bdb93200488b8df01300004c8d8d50010000488985a00000004c8d051938210033f6488d45804889442450\
488d44246848894424488b85101400008974244089442438488b85001400004c896c24304889442428488d85a00000004889442420\
488974246848897580e8d5810e00",
        source_name_rva: 0xa4f210,
        tess_dispatch_rva: 0x839123,
        tess_dispatch: c"3c680fb685a801000088442428488b85a001000048894424207507e86dceffffeb05e8b6cdffff",
        tess_create_sites: [
            tess_site(
                c"create_shader<SHS> (r4.cpp:934, hull)",
                0x835fb0,
                c"48895c240848896c24104889742418574883ec30488bfa418bd8488bcf418bd0498be9e888140000488b7424604c8d4c24204c8d05af0633008bd3\
488b0e48894110",
                0x837460,
                c"4883ec484c8bd148c744246000000000488b0d59444b004c8d4424604c894424204533c94c8bc2498bd2488b01ff9080000000",
            ),
            tess_site(
                c"create_shader<SDS> (r4.cpp:934, domain)",
                0x835f00,
                c"48895c240848896c24104889742418574883ec30488bfa418bd8488bcf418bd0498be9e8a8140000488b7424604c8d4c24204c8d055f0733008bd3\
488b0e48894110",
                0x8373d0,
                c"4883ec484c8bd148c744246000000000488b0de9444b004c8d4424604c894424204533c94c8bc2498bd2488b01ff9088000000",
            ),
        ],
    },
    // ---- AVX 1.5.3 DX11 (AnomalyDX11AVX.exe) ----
    MotionProfile {
        sites: [
            site(c"create_shader", 0x830080, b"48895c2410488974241848897c242055"),
            site(
                c"CRenderTarget::phase_scene_begin",
                0x8d8b90,
                b"40534883ec508b15dc0c4000488bd948",
            ),
            site(
                c"CRenderTarget::phase_scene_end",
                0x8d8c60,
                b"405557488d6c24b14881ecb800000048",
            ),
        ],
        unique_guards: [UniqueGuard::new(
            c"create_shader",
            0x830080,
            c"48895c2410488974241848897c2420554154415541564157488dac24b0",
        )],
        shader_compile_rva: 0x831900,
        shader_compile_prologue:
            c"48895c241044894c24204c8944241848894c24085556574154415541564157488dac2460ecffffb8a0140000",
        create_shader_calls: [
            site(
                c"shader_compile -> create_shader (cache file, r4.cpp:1867)",
                0x832cb1,
                b"e8cad3ffff89442468488bcee8fee086",
            ),
            site(
                c"shader_compile -> create_shader (D3DCompile output, r4.cpp:1903)",
                0x832e93,
                b"e8e8d1ffffe9ea00000048837b50104c",
            ),
        ],
        frame: CallerFrame {
            source_offset: 0x14f0,
            length_offset: 0x14f8,
            entry_offset: 0x1500,
            flags_offset: 0x1510,
            defines_offset: 0x250,
        },
        d3dcompile_setup_rva: 0x832cce,
        d3dcompile_setup: c"8b95f8130000488d052d183200488b8df01300004c8d8d50010000488985a00000004c8d054995200033f6488d45804889442450\
488d44246848894424488b85101400008974244089442438488b85001400004c896c24304889442428488d85a00000004889442420\
488974246848897580e845490e00",
        source_name_rva: 0xa3c240,
        tess_dispatch_rva: 0x8303c3,
        tess_dispatch: c"3c680fb685a801000088442428488b85a001000048894424207507e81dceffffeb05e866cdffff",
        tess_create_sites: [
            tess_site(
                c"create_shader<SHS> (r4.cpp:934, hull)",
                0x82d200,
                c"48895c240848896c24104889742418574883ec30488bfa418bd8488bcf418bd0498be9e888140000488b7424604c8d4c24204c8d05cf6532008bd3\
488b0e48894110",
                0x82e6b0,
                c"4883ec484c8bd148c744246000000000488b0d09b24a004c8d4424604c894424204533c94c8bc2498bd2488b01ff9080000000",
            ),
            tess_site(
                c"create_shader<SDS> (r4.cpp:934, domain)",
                0x82d150,
                c"48895c240848896c24104889742418574883ec30488bfa418bd8488bcf418bd0498be9e8a8140000488b7424604c8d4c24204c8d057f6632008bd3\
488b0e48894110",
                0x82e620,
                c"4883ec484c8bd148c744246000000000488b0d99b24a004c8d4424604c894424204533c94c8bc2498bd2488b01ff9088000000",
            ),
        ],
    },
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn profiles_follow_the_native_build_order() {
        assert_eq!(RECORDED.len(), crate::native_profile::RECORDED.len());
        for (index, motion) in RECORDED.iter().enumerate() {
            assert!(std::ptr::eq(MotionProfile::of(index).unwrap(), motion));
            let layout = motion.frame_layout();
            assert_eq!(layout.call_length, 5);
            assert_eq!(layout.define_capacity, 128);
            assert_eq!(layout.calls[0], motion.create_shader_calls[0].rva);
        }
        assert!(MotionProfile::of(RECORDED.len()).is_none());
    }

    #[test]
    fn hex_guards_are_even_and_extend_their_sites() {
        for profile in &RECORDED {
            for hex in [
                profile.shader_compile_prologue,
                profile.d3dcompile_setup,
                profile.tess_dispatch,
            ]
            .into_iter()
            .chain(
                profile
                    .tess_create_sites
                    .iter()
                    .flat_map(|s| [s.prologue, s.wrapper]),
            ) {
                let text = hex.to_bytes();
                assert!(text.len() % 2 == 0 && text.iter().all(u8::is_ascii_hexdigit));
            }
            let unique = &profile.unique_guards[0];
            let create = &profile.sites[SITE_CREATE_SHADER];
            assert_eq!(unique.rva, create.rva);
            let prefix: String = create.expected.iter().map(|b| format!("{b:02x}")).collect();
            let extended = unsafe { CStr::from_ptr(unique.expected_hex) }
                .to_str()
                .unwrap();
            assert!(extended.starts_with(&prefix) && extended.len() > prefix.len());
        }
    }
}
